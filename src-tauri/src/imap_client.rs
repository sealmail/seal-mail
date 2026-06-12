use crate::models::*;
use native_tls::TlsConnector;
use std::cell::Cell;

type ImapSession = imap::Session<native_tls::TlsStream<std::net::TcpStream>>;

/// XOAUTH2 SASL：首次挑战回初始响应，服务器若再发挑战（携带错误详情）则回空串
/// 以拿到最终 NO 响应，避免协议卡死。
struct XOAuth2<'a> {
    user: &'a str,
    token: &'a str,
    sent: Cell<bool>,
}

impl imap::Authenticator for XOAuth2<'_> {
    type Response = String;
    fn process(&self, _challenge: &[u8]) -> String {
        if self.sent.replace(true) {
            String::new()
        } else {
            crate::oauth::xoauth2_string(self.user, self.token)
        }
    }
}

// ── IMAP modified UTF-7（RFC 3501 §5.1.3）──
// 服务器目录名里的非 ASCII 字符（如中文「已发送」）以 &<改良base64>- 形式传输，
// 显示时解码、创建目录时编码；与服务器交互一律用原始（编码后）名字。

/// 改良 base64：用 ',' 代替 '/'，无填充，内容为 UTF-16BE
fn decode_mutf7(s: &str) -> String {
    use base64::engine::general_purpose::STANDARD_NO_PAD;
    use base64::Engine;
    let mut out = String::new();
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '&' {
            out.push(c);
            continue;
        }
        let mut seg = String::new();
        let mut closed = false;
        for n in it.by_ref() {
            if n == '-' {
                closed = true;
                break;
            }
            seg.push(n);
        }
        if seg.is_empty() {
            // "&-" 表示字面量 '&'
            out.push('&');
            continue;
        }
        let b64: String = seg.chars().map(|c| if c == ',' { '/' } else { c }).collect();
        let decoded = STANDARD_NO_PAD.decode(b64.as_bytes()).ok().and_then(|bytes| {
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|p| u16::from_be_bytes([p[0], p[1]]))
                .collect();
            String::from_utf16(&units).ok()
        });
        match decoded {
            Some(t) => out.push_str(&t),
            None => {
                // 不是合法编码段：原样保留
                out.push('&');
                out.push_str(&seg);
                if closed {
                    out.push('-');
                }
            }
        }
    }
    out
}

fn encode_mutf7(s: &str) -> String {
    use base64::engine::general_purpose::STANDARD_NO_PAD;
    use base64::Engine;
    fn flush(out: &mut String, buf: &mut Vec<u16>) {
        if buf.is_empty() {
            return;
        }
        let bytes: Vec<u8> = buf.iter().flat_map(|u| u.to_be_bytes()).collect();
        let b64 = STANDARD_NO_PAD.encode(&bytes).replace('/', ",");
        out.push('&');
        out.push_str(&b64);
        out.push('-');
        buf.clear();
    }
    let mut out = String::new();
    let mut buf: Vec<u16> = Vec::new();
    for c in s.chars() {
        if c == '&' {
            flush(&mut out, &mut buf);
            out.push_str("&-");
        } else if (' '..='~').contains(&c) {
            flush(&mut out, &mut buf);
            out.push(c);
        } else {
            let mut u = [0u16; 2];
            buf.extend_from_slice(c.encode_utf16(&mut u));
        }
    }
    flush(&mut out, &mut buf);
    out
}

pub fn connect(account: &Account, secret: &AccountSecret) -> Result<ImapSession, String> {
    let tls = TlsConnector::builder()
        .build()
        .map_err(|e| format!("TLS 初始化失败: {}", e))?;
    let client = imap::connect(
        (account.incoming_host.as_str(), account.incoming_port),
        account.incoming_host.as_str(),
        &tls,
    )
    .map_err(|e| format!("无法连接 {}:{} — {}", account.incoming_host, account.incoming_port, e))?;
    if let Some(oauth) = &secret.oauth {
        let auth = XOAuth2 { user: &account.username, token: &oauth.access_token, sent: Cell::new(false) };
        client
            .authenticate("XOAUTH2", &auth)
            .map_err(|(e, _)| format!("IMAP OAuth2 登录失败（授权可能已失效，请重新授权）: {}", e))
    } else {
        client
            .login(&account.username, &secret.password)
            .map_err(|(e, _)| format!("IMAP 登录失败（请检查用户名/密码或应用专用密码）: {}", e))
    }
}

/// 回收站目录的常见名字（去层级后的末段，小写比较）
const TRASH_NAMES: &[&str] = &["trash", "deleted items", "deleted messages", "已删除", "已删除邮件", "垃圾箱", "废件箱"];
const ARCHIVE_NAMES: &[&str] = &["archive", "archives", "all mail", "all mails", "归档", "所有邮件"];

fn name_is_trash(n: &imap::types::Name) -> bool {
    // RFC 6154 special-use（Gmail 等在 LIST 中直接返回 \Trash）
    if n.attributes()
        .iter()
        .any(|a| matches!(a, imap::types::NameAttribute::Custom(c) if c.eq_ignore_ascii_case("\\Trash")))
    {
        return true;
    }
    let name = n.name();
    let last = name.rsplit(n.delimiter().unwrap_or("/")).next().unwrap_or(name);
    TRASH_NAMES.contains(&decode_mutf7(last).to_lowercase().as_str())
}

fn name_is_archive(n: &imap::types::Name) -> bool {
    if n.attributes()
        .iter()
        .any(|a| matches!(a, imap::types::NameAttribute::Custom(c) if c.eq_ignore_ascii_case("\\Archive")))
    {
        return true;
    }
    let name = n.name();
    let last = name.rsplit(n.delimiter().unwrap_or("/")).next().unwrap_or(name);
    ARCHIVE_NAMES.contains(&decode_mutf7(last).to_lowercase().as_str())
}

/// 找到服务器上的回收站目录；不存在则创建 "Trash"
fn find_or_create_trash(sess: &mut ImapSession) -> Result<String, String> {
    let names = sess
        .list(None, Some("*"))
        .map_err(|e| format!("获取目录失败: {}", e))?;
    if let Some(n) = names.iter().find(|n| name_is_trash(n)) {
        return Ok(n.name().to_string());
    }
    sess.create("Trash")
        .map_err(|e| format!("创建回收站目录失败: {}", e))?;
    Ok("Trash".into())
}

/// 找到服务器上的归档目录；不存在则创建 "Archive"
fn find_or_create_archive(sess: &mut ImapSession) -> Result<String, String> {
    let names = sess
        .list(None, Some("*"))
        .map_err(|e| format!("获取目录失败: {}", e))?;
    if let Some(n) = names.iter().find(|n| name_is_archive(n)) {
        return Ok(n.name().to_string());
    }
    sess.create("Archive")
        .map_err(|e| format!("创建归档目录失败: {}", e))?;
    Ok("Archive".into())
}

pub fn list_folders(account: &Account, secret: &AccountSecret) -> Result<Vec<FolderInfo>, String> {
    let mut sess = connect(account, secret)?;
    let names = sess
        .list(None, Some("*"))
        .map_err(|e| format!("获取目录失败: {}", e))?;
    let mut out: Vec<FolderInfo> = names
        .iter()
        .filter(|n| !n.attributes().iter().any(|a| matches!(a, imap::types::NameAttribute::NoSelect)))
        .map(|n| {
            let name = n.name().to_string();
            let last = name
                .rsplit(n.delimiter().unwrap_or("/"))
                .next()
                .unwrap_or(&name);
            FolderInfo {
                display: decode_mutf7(last),
                role: if name_is_trash(n) {
                    Some("trash".into())
                } else if name_is_archive(n) {
                    Some("archive".into())
                } else {
                    None
                },
                name,
            }
        })
        .collect();
    let _ = sess.logout();
    // INBOX 永远排第一
    out.sort_by_key(|f| if f.name.eq_ignore_ascii_case("INBOX") { 0 } else { 1 });
    Ok(out)
}

pub struct RawMail {
    pub uid: u32,
    pub unread: bool,
    pub flagged: bool,
    pub raw: Vec<u8>,
}

/// 一次增量同步从服务器带回的所有信息
pub struct SyncFetch {
    pub uidvalidity: u32,
    /// true = 本地该目录缓存需清空重建（UIDVALIDITY 变化或首次同步）
    pub reset: bool,
    pub new_mails: Vec<RawMail>,
    /// 回扫窗口内服务器现存邮件的 (uid, unread, flagged)，用于同步已读/星标并检测删除
    pub server_flags: Vec<(u32, bool, bool)>,
    /// 回扫窗口下界 uid（reset 时无意义）
    pub flags_low: u32,
}

fn flags_of(f: &imap::types::Fetch) -> (bool, bool) {
    let unread = !f.flags().iter().any(|fl| matches!(fl, imap::types::Flag::Seen));
    let flagged = f.flags().iter().any(|fl| matches!(fl, imap::types::Flag::Flagged));
    (unread, flagged)
}

/// 增量同步：
/// - UIDVALIDITY 变化/首次 → 抓最近 initial_window 封全文
/// - 否则只抓 uid > max_uid 的新邮件全文，并对最近窗口做 FLAGS 回扫（已读/星标/删除）
pub fn sync_fetch(
    account: &Account,
    secret: &AccountSecret,
    folder: &str,
    stored_validity: Option<u32>,
    max_uid: Option<u32>,
    flags_low: Option<u32>,
    initial_window: u32,
) -> Result<SyncFetch, String> {
    let mut sess = connect(account, secret)?;
    let mailbox = sess
        .select(folder)
        .map_err(|e| format!("无法打开目录 {}: {}", folder, e))?;
    let uidvalidity = mailbox.uid_validity.unwrap_or(0);
    let reset = stored_validity != Some(uidvalidity) || max_uid.is_none();

    let mut new_mails = Vec::new();
    if reset {
        if mailbox.exists > 0 {
            let start = mailbox.exists.saturating_sub(initial_window.saturating_sub(1)).max(1);
            let fetches = sess
                .fetch(format!("{}:{}", start, mailbox.exists), "(UID FLAGS BODY.PEEK[])")
                .map_err(|e| format!("拉取邮件失败: {}", e))?;
            for f in fetches.iter() {
                let (Some(uid), Some(raw)) = (f.uid, f.body()) else { continue };
                let (unread, flagged) = flags_of(f);
                new_mails.push(RawMail { uid, unread, flagged, raw: raw.to_vec() });
            }
        }
        let _ = sess.logout();
        return Ok(SyncFetch { uidvalidity, reset: true, new_mails, server_flags: vec![], flags_low: 0 });
    }

    let max = max_uid.expect("checked above");
    // 先只探测新 UID（UID FETCH n:* 在没有新邮件时也会返回最大 uid 那封，需过滤），
    // 有新邮件才拉全文，避免每轮同步重复下载最新一封
    let probe = sess
        .uid_fetch(format!("{}:*", max + 1), "UID")
        .map_err(|e| format!("探测新邮件失败: {}", e))?;
    let new_uids: Vec<u32> = probe.iter().filter_map(|f| f.uid).filter(|u| *u > max).collect();
    if !new_uids.is_empty() {
        let set = new_uids.iter().map(|u| u.to_string()).collect::<Vec<_>>().join(",");
        let fetches = sess
            .uid_fetch(set, "(UID FLAGS BODY.PEEK[])")
            .map_err(|e| format!("拉取新邮件失败: {}", e))?;
        for f in fetches.iter() {
            let (Some(uid), Some(raw)) = (f.uid, f.body()) else { continue };
            let (unread, flagged) = flags_of(f);
            new_mails.push(RawMail { uid, unread, flagged, raw: raw.to_vec() });
        }
    }
    // FLAGS 回扫（轻量，不带正文）
    let low = flags_low.unwrap_or(1);
    let flag_fetches = sess
        .uid_fetch(format!("{}:*", low), "(UID FLAGS)")
        .map_err(|e| format!("同步邮件状态失败: {}", e))?;
    let mut server_flags = Vec::new();
    for f in flag_fetches.iter() {
        let Some(uid) = f.uid else { continue };
        let (unread, flagged) = flags_of(f);
        server_flags.push((uid, unread, flagged));
    }
    let _ = sess.logout();
    Ok(SyncFetch { uidvalidity, reset: false, new_mails, server_flags, flags_low: low })
}

/// 从当前本地最早 UID 之前继续回填更早邮件。返回值按服务器返回顺序排列。
pub fn fetch_older(
    account: &Account,
    secret: &AccountSecret,
    folder: &str,
    before_uid: Option<u32>,
    batch: u32,
) -> Result<Vec<RawMail>, String> {
    let Some(before) = before_uid else { return Ok(Vec::new()) };
    if before <= 1 || batch == 0 {
        return Ok(Vec::new());
    }
    let mut sess = connect(account, secret)?;
    sess.select(folder)
        .map_err(|e| format!("无法打开目录 {}: {}", folder, e))?;

    let probe = sess
        .uid_fetch(format!("1:{}", before - 1), "UID")
        .map_err(|e| format!("探测更早邮件失败: {}", e))?;
    let mut uids: Vec<u32> = probe.iter().filter_map(|f| f.uid).filter(|u| *u < before).collect();
    uids.sort_unstable_by(|a, b| b.cmp(a));
    uids.truncate(batch as usize);
    if uids.is_empty() {
        let _ = sess.logout();
        return Ok(Vec::new());
    }
    uids.sort_unstable();
    let set = uids.iter().map(|u| u.to_string()).collect::<Vec<_>>().join(",");
    let fetches = sess
        .uid_fetch(set, "(UID FLAGS BODY.PEEK[])")
        .map_err(|e| format!("拉取更早邮件失败: {}", e))?;
    let mut mails = Vec::new();
    for f in fetches.iter() {
        let (Some(uid), Some(raw)) = (f.uid, f.body()) else { continue };
        let (unread, flagged) = flags_of(f);
        mails.push(RawMail { uid, unread, flagged, raw: raw.to_vec() });
    }
    let _ = sess.logout();
    Ok(mails)
}

pub fn set_flagged(
    account: &Account,
    secret: &AccountSecret,
    folder: &str,
    uid: u32,
    flagged: bool,
) -> Result<(), String> {
    let mut sess = connect(account, secret)?;
    sess.select(folder).map_err(|e| e.to_string())?;
    let op = if flagged { "+FLAGS (\\Flagged)" } else { "-FLAGS (\\Flagged)" };
    sess.uid_store(uid.to_string(), op).map_err(|e| e.to_string())?;
    let _ = sess.logout();
    Ok(())
}

/// 拉取单封邮件原文（附件下载用）
pub fn fetch_raw(
    account: &Account,
    secret: &AccountSecret,
    folder: &str,
    uid: u32,
) -> Result<Vec<u8>, String> {
    let mut sess = connect(account, secret)?;
    sess.select(folder).map_err(|e| e.to_string())?;
    let fetches = sess
        .uid_fetch(uid.to_string(), "BODY.PEEK[]")
        .map_err(|e| format!("拉取邮件失败: {}", e))?;
    let raw = fetches
        .iter()
        .next()
        .and_then(|f| f.body().map(|b| b.to_vec()))
        .ok_or("邮件不存在（可能已被移动或删除）")?;
    let _ = sess.logout();
    Ok(raw)
}

pub fn create_folder(account: &Account, secret: &AccountSecret, name: &str) -> Result<(), String> {
    let mut sess = connect(account, secret)?;
    sess.create(encode_mutf7(name))
        .map_err(|e| format!("创建目录失败: {}", e))?;
    let _ = sess.logout();
    Ok(())
}

pub fn delete_folder(account: &Account, secret: &AccountSecret, name: &str) -> Result<(), String> {
    let mut sess = connect(account, secret)?;
    sess.delete(name)
        .map_err(|e| format!("删除目录失败: {}", e))?;
    let _ = sess.logout();
    Ok(())
}

pub fn move_message(
    account: &Account,
    secret: &AccountSecret,
    folder: &str,
    uid: u32,
    target: &str,
) -> Result<(), String> {
    let mut sess = connect(account, secret)?;
    sess.select(folder).map_err(|e| e.to_string())?;
    let uidset = uid.to_string();
    // 优先 MOVE，不支持则 COPY + 删除
    if sess.uid_mv(&uidset, target).is_err() {
        sess.uid_copy(&uidset, target)
            .map_err(|e| format!("移动失败（目标目录可能不存在）: {}", e))?;
        sess.uid_store(&uidset, "+FLAGS (\\Deleted)")
            .map_err(|e| e.to_string())?;
        sess.expunge().map_err(|e| e.to_string())?;
    }
    let _ = sess.logout();
    Ok(())
}

pub fn archive_message(
    account: &Account,
    secret: &AccountSecret,
    folder: &str,
    uid: u32,
) -> Result<String, String> {
    let mut sess = connect(account, secret)?;
    let archive = find_or_create_archive(&mut sess)?;
    if archive.eq_ignore_ascii_case(folder) {
        let _ = sess.logout();
        return Ok(archive);
    }
    sess.select(folder).map_err(|e| e.to_string())?;
    let uidset = uid.to_string();
    if sess.uid_mv(&uidset, &archive).is_err() {
        sess.uid_copy(&uidset, &archive)
            .map_err(|e| format!("归档失败: {}", e))?;
        sess.uid_store(&uidset, "+FLAGS (\\Deleted)")
            .map_err(|e| e.to_string())?;
        sess.expunge().map_err(|e| e.to_string())?;
    }
    let _ = sess.logout();
    Ok(archive)
}

pub fn set_read(
    account: &Account,
    secret: &AccountSecret,
    folder: &str,
    uid: u32,
    read: bool,
) -> Result<(), String> {
    set_read_many(account, secret, folder, &[uid], read)
}

/// 单条连接批量改已读标记（「全部已读」用）
pub fn set_read_many(
    account: &Account,
    secret: &AccountSecret,
    folder: &str,
    uids: &[u32],
    read: bool,
) -> Result<(), String> {
    if uids.is_empty() {
        return Ok(());
    }
    let mut sess = connect(account, secret)?;
    sess.select(folder).map_err(|e| e.to_string())?;
    let op = if read { "+FLAGS (\\Seen)" } else { "-FLAGS (\\Seen)" };
    let uidset = uids.iter().map(|u| u.to_string()).collect::<Vec<_>>().join(",");
    sess.uid_store(uidset, op).map_err(|e| e.to_string())?;
    let _ = sess.logout();
    Ok(())
}

/// permanent=false：移入回收站（默认，可恢复）；permanent=true：物理删除（仅回收站内使用）
pub fn delete_message(
    account: &Account,
    secret: &AccountSecret,
    folder: &str,
    uid: u32,
    permanent: bool,
) -> Result<(), String> {
    let mut sess = connect(account, secret)?;
    let trash = if permanent { None } else { Some(find_or_create_trash(&mut sess)?) };
    sess.select(folder).map_err(|e| e.to_string())?;
    let uidset = uid.to_string();
    match trash {
        // 已经在回收站里点删除（前端没要求 permanent 也兜底物理删，避免原地自移）
        Some(t) if !t.eq_ignore_ascii_case(folder) => {
            if sess.uid_mv(&uidset, &t).is_err() {
                sess.uid_copy(&uidset, &t)
                    .map_err(|e| format!("移入回收站失败: {}", e))?;
                sess.uid_store(&uidset, "+FLAGS (\\Deleted)")
                    .map_err(|e| e.to_string())?;
                sess.expunge().map_err(|e| e.to_string())?;
            }
        }
        _ => {
            sess.uid_store(&uidset, "+FLAGS (\\Deleted)")
                .map_err(|e| e.to_string())?;
            sess.expunge().map_err(|e| e.to_string())?;
        }
    }
    let _ = sess.logout();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{decode_mutf7, encode_mutf7};

    #[test]
    fn mutf7_rfc3501_examples() {
        // RFC 3501 §5.1.3 示例：~peter/mail/&U,BTFw-/&ZeVnLIqe-
        assert_eq!(decode_mutf7("&U,BTFw-"), "台北");
        assert_eq!(encode_mutf7("台北"), "&U,BTFw-");
        // "&-" 是字面量 '&'
        assert_eq!(decode_mutf7("a&-b"), "a&b");
        assert_eq!(encode_mutf7("a&b"), "a&-b");
        // 纯 ASCII 原样
        assert_eq!(decode_mutf7("INBOX/Sent"), "INBOX/Sent");
        assert_eq!(encode_mutf7("INBOX/Sent"), "INBOX/Sent");
    }

    #[test]
    fn mutf7_roundtrip() {
        for s in ["已发送", "重要客户 2026", "工作&生活", "收件箱/发票", "Ω≈ç√∫"] {
            assert_eq!(decode_mutf7(&encode_mutf7(s)), s, "roundtrip failed: {}", s);
        }
    }

    #[test]
    fn mutf7_invalid_segment_kept_verbatim() {
        // 非法 base64 段不应被吞掉
        assert_eq!(decode_mutf7("&!!!-x"), "&!!!-x");
    }
}
