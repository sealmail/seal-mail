//! 新邮件监听：每个账户一个后台线程。
//! - IMAP：常驻连接 + RFC 2177 IDLE，服务器实时推送收件箱变化（Exchange/Gmail/QQ 等都支持）
//! - POP3：协议没有推送，定时 STAT 轮询邮件数
//! 检测到新邮件后向前端 emit "new-mail" 事件，前端自动刷新列表。
//! 账户被删除后线程在下一轮自行退出；连接断开/出错按固定间隔重连。

use crate::models::*;
use crate::store::AppState;
use crate::{imap_client, mail, oauth, pop3_client};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{path::BaseDirectory, AppHandle, Emitter, Manager};

/// 单轮 IDLE 等待时长；到时会重新 EXAMINE 校对一次再继续
const IDLE_ROUND: Duration = Duration::from_secs(4 * 60);
/// 一条连接最多跑的 IDLE 轮数，之后重建连接（顺带刷新 OAuth 令牌）
const IDLE_ROUNDS_PER_CONN: u32 = 6;
const POP3_POLL: Duration = Duration::from_secs(120);
const RETRY_DELAY: Duration = Duration::from_secs(30);
const NOTIFICATION_OPEN_TTL: Duration = Duration::from_secs(24 * 60 * 60);

fn running() -> &'static Mutex<HashSet<String>> {
    static R: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashSet::new()))
}

/// 为所有还没有监听线程的账户启动监听（启动时与新增账户后调用）
pub fn ensure_watchers(app: &AppHandle) {
    let accounts: Vec<(String, IncomingProtocol)> = {
        let state = app.state::<AppState>();
        let s = state.inner.lock().unwrap();
        s.accounts
            .iter()
            .map(|a| (a.id.clone(), a.protocol.clone()))
            .collect()
    };
    for (id, protocol) in accounts {
        if !running().lock().unwrap().insert(id.clone()) {
            continue; // 已有线程在跑
        }
        let app = app.clone();
        std::thread::spawn(move || {
            match protocol {
                IncomingProtocol::Imap => watch_imap(&app, &id),
                IncomingProtocol::Pop3 => watch_pop3(&app, &id),
            }
            running().lock().unwrap().remove(&id);
        });
    }
}

#[derive(Clone, Debug)]
struct MailNotice {
    uid: Option<u32>,
    message_id: Option<String>,
    from_name: String,
    from_addr: String,
    subject: String,
    preview: String,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationMailTarget {
    account_id: String,
    folder: String,
    uid: Option<u32>,
    message_id: Option<String>,
}

fn pending_notification_target() -> &'static Mutex<Option<(Instant, NotificationMailTarget)>> {
    static PENDING: OnceLock<Mutex<Option<(Instant, NotificationMailTarget)>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(None))
}

/// 写入待打开通知目标（带时间戳）。运行期只在 notify_new_mail 内通过 set 触发；
/// 这里提取成函数是为了让测试也能注入一个目标。
fn set_pending_notification_target(created_at: Instant, target: NotificationMailTarget) {
    *pending_notification_target().lock().unwrap() = Some((created_at, target));
}

fn emit_new_mail(app: &AppHandle, account_id: &str, new_count: u32, notices: Vec<MailNotice>) {
    let _ = app.emit("new-mail", serde_json::json!({ "accountId": account_id }));
    notify_new_mail(app, account_id, new_count, &notices);
}

/// 窗口未聚焦且偏好开启时弹系统通知横幅
fn notify_new_mail(app: &AppHandle, account_id: &str, new_count: u32, notices: &[MailNotice]) {
    use tauri_plugin_notification::NotificationExt;
    let (enabled, email) = {
        let state = app.state::<AppState>();
        let s = state.inner.lock().unwrap();
        let email = match s.accounts.iter().find(|a| a.id == account_id) {
            Some(a) => a.email.clone(),
            None => return,
        };
        (s.prefs.notify_new_mail, email)
    };
    if !enabled {
        return;
    }
    let focused = app
        .webview_windows()
        .values()
        .next()
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false);
    if focused {
        return; // 正在看着应用就不用打扰了
    }
    let (title, body) = if notices.len() == 1 {
        let n = &notices[0];
        (
            truncate_for_notice(&format_sender(n), 80),
            format!(
                "标题：{}\n正文：{}",
                truncate_for_notice(&n.subject, 80),
                truncate_for_notice(&n.preview, 140)
            ),
        )
    } else if !notices.is_empty() {
        let mut lines = notices
            .iter()
            .take(3)
            .map(|n| {
                format!(
                    "{}｜{}：{}",
                    truncate_for_notice(&format_sender(n), 24),
                    truncate_for_notice(&n.subject, 28),
                    truncate_for_notice(&n.preview, 36)
                )
            })
            .collect::<Vec<_>>();
        if new_count as usize > lines.len() {
            lines.push(format!("还有 {} 封", new_count as usize - lines.len()));
        }
        (format!("收到 {} 封新邮件", new_count), lines.join("\n"))
    } else if new_count > 1 {
        (format!("收到 {} 封新邮件", new_count), email)
    } else {
        ("收到新邮件".to_string(), email)
    };
    if let Some(n) = notices.first() {
        set_pending_notification_target(
            Instant::now(),
            NotificationMailTarget {
                account_id: account_id.to_string(),
                folder: "INBOX".to_string(),
                uid: n.uid,
                message_id: n.message_id.clone(),
            },
        );
    }
    let mut builder = app.notification().builder().title(title).body(body);
    if let Some(icon) = notification_icon_path(app) {
        builder = builder.icon(icon);
    }
    if let Err(e) = builder.show() {
        eprintln!("[watcher] 系统通知发送失败: {}", e);
    }
}

fn notification_icon_path(app: &AppHandle) -> Option<String> {
    app.path()
        .resolve("icons/icon.png", BaseDirectory::Resource)
        .ok()
        .filter(|p| p.exists())
        .or_else(|| {
            let dev_icon = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("icons/icon.png");
            dev_icon.exists().then_some(dev_icon)
        })
        .and_then(|p| p.to_str().map(str::to_owned))
}

/// 是否还有未过期的待打开通知目标（只读，不消费）。
pub fn has_pending_notification_target() -> bool {
    pending_notification_target()
        .lock()
        .unwrap()
        .as_ref()
        .map(|(created_at, _)| created_at.elapsed() <= NOTIFICATION_OPEN_TTL)
        .unwrap_or(false)
}

/// 取出并清除待打开通知目标（带 TTL 校验）。
/// 只有真正把目标交给前端的那一次拉取才会消费它——避免在窗口隐藏/监听器未就绪时
/// 被一次性 emit 白白吃掉，从而导致点击通知后既不弹窗也不定位。
pub fn take_pending_notification_target() -> Option<NotificationMailTarget> {
    pending_notification_target()
        .lock()
        .unwrap()
        .take()
        .and_then(|(created_at, target)| (created_at.elapsed() <= NOTIFICATION_OPEN_TTL).then_some(target))
}

/// 应用被激活（点击通知 / Dock / 窗口聚焦）后调用：若存在待打开通知，
/// 唤起主窗口并发一个无副作用的 poke，由前端主动来拉取目标（不在这里消费）。
pub fn poke_pending_notification_open(app: &AppHandle) {
    if !has_pending_notification_target() {
        return;
    }
    reveal_main_window(app);
    let _ = app.emit("notification-activated", ());
}

pub fn reveal_main_window(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.show();

    if let Some(window) = app.get_webview_window("main").or_else(|| app.webview_windows().values().next().cloned()) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn truncate_for_notice(s: &str, max_chars: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn format_sender(n: &MailNotice) -> String {
    let name = n.from_name.trim();
    let addr = n.from_addr.trim();
    if name.is_empty() || name == addr {
        addr.to_string()
    } else {
        format!("{} <{}>", name, addr)
    }
}

fn notice_from_raw(raw: &[u8], uid: Option<u32>) -> Option<MailNotice> {
    let meta = mail::parse_email(raw, uid.unwrap_or(0), "", "INBOX", true, false, &[])
        .ok()?
        .meta;
    Some(MailNotice {
        uid,
        message_id: meta.message_id,
        from_name: meta.from_name,
        from_addr: meta.from_addr,
        subject: meta.subject,
        preview: meta.preview,
    })
}

/// 取账户凭据（OAuth 令牌临近过期则阻塞刷新并回写）。
/// Ok(None) = 账户已删除（线程应退出）；Err = 临时失败（稍后重试）
fn creds(app: &AppHandle, account_id: &str) -> Result<Option<(Account, AccountSecret)>, String> {
    let state = app.state::<AppState>();
    let (account, secret) = {
        let s = state.inner.lock().unwrap();
        match (s.account(account_id), s.secret(account_id)) {
            (Ok(a), Ok(sec)) => (a, sec),
            _ => return Ok(None),
        }
    };
    let Some(tokens) = secret.oauth.clone() else {
        return Ok(Some((account, secret)));
    };
    if !tokens.needs_refresh() {
        return Ok(Some((account, secret)));
    }
    let refreshed = oauth::refresh_tokens_blocking(&tokens)?;
    let s2 = app.state::<AppState>();
    let mut s = s2.inner.lock().unwrap();
    let Some(entry) = s.secrets.get_mut(account_id) else {
        return Ok(None);
    };
    entry.oauth = Some(refreshed);
    let updated = entry.clone();
    s.save_secrets()?;
    Ok(Some((account, updated)))
}

fn account_exists(app: &AppHandle, account_id: &str) -> bool {
    let state = app.state::<AppState>();
    let s = state.inner.lock().unwrap();
    s.accounts.iter().any(|a| a.id == account_id)
}

fn watch_imap(app: &AppHandle, account_id: &str) {
    let mut last_exists: Option<u32> = None;
    loop {
        let (account, secret) = match creds(app, account_id) {
            Ok(Some(c)) => c,
            Ok(None) => return,
            Err(e) => {
                eprintln!("[watcher] {} 取凭据失败: {}", account_id, e);
                std::thread::sleep(RETRY_DELAY);
                continue;
            }
        };
        if let Err(e) = idle_session(app, account_id, &account, &secret, &mut last_exists) {
            eprintln!("[watcher] {} IDLE 连接中断: {}", account_id, e);
            std::thread::sleep(RETRY_DELAY);
        }
    }
}

/// 一条 IMAP 连接上的多轮 IDLE；正常跑满轮数后返回 Ok 由外层重连
fn idle_session(
    app: &AppHandle,
    account_id: &str,
    account: &Account,
    secret: &AccountSecret,
    last_exists: &mut Option<u32>,
) -> Result<(), String> {
    let mut sess = imap_client::connect(account, secret)?;
    let mb = sess.examine("INBOX").map_err(|e| e.to_string())?;
    let new_count = mb.exists.saturating_sub(last_exists.unwrap_or(mb.exists));
    check_exists(app, account_id, mb.exists, last_exists, || {
        fetch_imap_notices(account, secret, new_count)
    });
    for _ in 0..IDLE_ROUNDS_PER_CONN {
        if !account_exists(app, account_id) {
            let _ = sess.logout();
            return Ok(());
        }
        // 阻塞直到服务器推送变化或本轮超时（Handle drop 时自动发 DONE）
        sess.idle()
            .map_err(|e| e.to_string())?
            .wait_with_timeout(IDLE_ROUND)
            .map_err(|e| e.to_string())?;
        let mb = sess.examine("INBOX").map_err(|e| e.to_string())?;
        let new_count = mb.exists.saturating_sub(last_exists.unwrap_or(mb.exists));
        check_exists(app, account_id, mb.exists, last_exists, || {
            fetch_imap_notices(account, secret, new_count)
        });
    }
    let _ = sess.logout();
    Ok(())
}

fn check_exists<F>(
    app: &AppHandle,
    account_id: &str,
    exists: u32,
    last: &mut Option<u32>,
    fetch_notices: F,
) where
    F: FnOnce() -> Vec<MailNotice>,
{
    if let Some(prev) = *last {
        if exists > prev {
            emit_new_mail(app, account_id, exists - prev, fetch_notices());
        }
    }
    *last = Some(exists);
}

fn fetch_imap_notices(
    account: &Account,
    secret: &AccountSecret,
    new_count: u32,
) -> Vec<MailNotice> {
    match imap_client::fetch_latest(account, secret, "INBOX", new_count.min(3)) {
        Ok(mails) => {
            let mut notices = mails
                .iter()
                .filter_map(|m| notice_from_raw(&m.raw, Some(m.uid)))
                .collect::<Vec<_>>();
            notices.reverse();
            notices
        }
        Err(e) => {
            eprintln!("[watcher] {} 拉取通知邮件详情失败: {}", account.id, e);
            Vec::new()
        }
    }
}

fn watch_pop3(app: &AppHandle, account_id: &str) {
    let mut last: Option<u32> = None;
    loop {
        match creds(app, account_id) {
            Ok(None) => return,
            Err(e) => eprintln!("[watcher] {} 取凭据失败: {}", account_id, e),
            Ok(Some((account, secret))) => {
                match pop3_client::Pop3Client::connect(&account, &secret) {
                    Ok(mut c) => {
                        match c.message_count() {
                            Ok(n) => {
                                let notices = if let Some(prev) = last {
                                    if n > prev {
                                        let start = prev + 1;
                                        (start..=n)
                                            .rev()
                                            .take(3)
                                            .filter_map(|seq| match c.retrieve(seq) {
                                                Ok(raw) => notice_from_raw(&raw, None),
                                                Err(e) => {
                                                    eprintln!(
                                                        "[watcher] {} 拉取 POP3 通知邮件详情失败 seq={}: {}",
                                                        account_id, seq, e
                                                    );
                                                    None
                                                }
                                            })
                                            .collect()
                                    } else {
                                        Vec::new()
                                    }
                                } else {
                                    Vec::new()
                                };
                                check_exists(app, account_id, n, &mut last, || notices);
                            }
                            Err(e) => eprintln!("[watcher] {} POP3 STAT 失败: {}", account_id, e),
                        }
                        c.quit();
                    }
                    Err(e) => eprintln!("[watcher] {} POP3 连接失败: {}", account_id, e),
                }
            }
        }
        std::thread::sleep(POP3_POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_target(uid: u32) -> NotificationMailTarget {
        NotificationMailTarget {
            account_id: "acc-1".to_string(),
            folder: "INBOX".to_string(),
            uid: Some(uid),
            message_id: Some(format!("<msg-{uid}@example.com>")),
        }
    }

    fn clear_pending() {
        *pending_notification_target().lock().unwrap() = None;
    }

    // 单个测试串行覆盖待打开目标的生命周期：用进程级 static，拆成多个 #[test]
    // 会被 cargo 并行跑而互相踩，所以合并成一个顺序执行。
    //
    // 通知点击改为「前端主动拉取」后的核心不变量：
    // 观察（has / poke）不能消费目标，只有真正取走（take）才消费，且只消费一次。
    // 旧实现里 emit 一次就把目标 take 掉，事件在监听器未就绪时丢失，目标也白白没了——
    // 正是这一点导致点击通知既不弹窗也不定位。这里锁死「观察不消费、取走只一次、过期取不到」。
    #[test]
    fn pending_notification_target_lifecycle() {
        clear_pending();

        // 没有目标时：观察为空，取走为 None
        assert!(!has_pending_notification_target());
        assert!(take_pending_notification_target().is_none());

        // —— 观察不消费、取走只消费一次 ——
        set_pending_notification_target(Instant::now(), sample_target(42));
        assert!(has_pending_notification_target());
        assert!(has_pending_notification_target(), "多次观察不应把目标消费掉");

        let taken = take_pending_notification_target().expect("应取到待打开目标");
        assert_eq!(taken.uid, Some(42));
        assert_eq!(taken.account_id, "acc-1");

        assert!(!has_pending_notification_target(), "取走后应已被消费");
        assert!(take_pending_notification_target().is_none(), "目标只能被取走一次");

        // —— 超过 TTL 的目标视为过期：既不可见也取不到 ——
        let expired_at = Instant::now()
            .checked_sub(NOTIFICATION_OPEN_TTL + Duration::from_secs(1))
            .expect("测试基准时间应可回退");
        set_pending_notification_target(expired_at, sample_target(7));
        assert!(!has_pending_notification_target(), "过期目标不应可见");
        assert!(take_pending_notification_target().is_none(), "过期目标不应被取走");

        clear_pending();
    }
}
