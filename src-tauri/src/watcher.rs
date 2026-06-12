//! 新邮件监听：每个账户一个后台线程。
//! - IMAP：常驻连接 + RFC 2177 IDLE，服务器实时推送收件箱变化（Exchange/Gmail/QQ 等都支持）
//! - POP3：协议没有推送，定时 STAT 轮询邮件数
//! 检测到新邮件后向前端 emit "new-mail" 事件，前端自动刷新列表。
//! 账户被删除后线程在下一轮自行退出；连接断开/出错按固定间隔重连。

use crate::models::*;
use crate::store::AppState;
use crate::{imap_client, oauth, pop3_client};
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// 单轮 IDLE 等待时长；到时会重新 EXAMINE 校对一次再继续
const IDLE_ROUND: Duration = Duration::from_secs(4 * 60);
/// 一条连接最多跑的 IDLE 轮数，之后重建连接（顺带刷新 OAuth 令牌）
const IDLE_ROUNDS_PER_CONN: u32 = 6;
const POP3_POLL: Duration = Duration::from_secs(120);
const RETRY_DELAY: Duration = Duration::from_secs(30);

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

fn emit_new_mail(app: &AppHandle, account_id: &str, new_count: u32) {
    let _ = app.emit("new-mail", serde_json::json!({ "accountId": account_id }));
    notify_new_mail(app, account_id, new_count);
}

/// 窗口未聚焦且偏好开启时弹系统通知横幅
fn notify_new_mail(app: &AppHandle, account_id: &str, new_count: u32) {
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
    let body = if new_count > 1 {
        format!("{} 收到 {} 封新邮件", email, new_count)
    } else {
        format!("{} 收到新邮件", email)
    };
    if let Err(e) = app
        .notification()
        .builder()
        .title("SealMail 信印")
        .body(body)
        .show()
    {
        eprintln!("[watcher] 系统通知发送失败: {}", e);
    }
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
    check_exists(app, account_id, mb.exists, last_exists);
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
        check_exists(app, account_id, mb.exists, last_exists);
    }
    let _ = sess.logout();
    Ok(())
}

fn check_exists(app: &AppHandle, account_id: &str, exists: u32, last: &mut Option<u32>) {
    if let Some(prev) = *last {
        if exists > prev {
            emit_new_mail(app, account_id, exists - prev);
        }
    }
    *last = Some(exists);
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
                            Ok(n) => check_exists(app, account_id, n, &mut last),
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
