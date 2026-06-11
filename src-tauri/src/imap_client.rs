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
            FolderInfo { display: decode_mutf7(last), name }
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
    pub raw: Vec<u8>,
}

pub fn fetch_window(
    account: &Account,
    secret: &AccountSecret,
    folder: &str,
    limit: u32,
) -> Result<Vec<RawMail>, String> {
    let mut sess = connect(account, secret)?;
    let mailbox = sess
        .select(folder)
        .map_err(|e| format!("无法打开目录 {}: {}", folder, e))?;
    if mailbox.exists == 0 {
        let _ = sess.logout();
        return Ok(vec![]);
    }
    let start = mailbox.exists.saturating_sub(limit.saturating_sub(1)).max(1);
    let range = format!("{}:{}", start, mailbox.exists);
    let fetches = sess
        .fetch(&range, "(UID FLAGS BODY.PEEK[])")
        .map_err(|e| format!("拉取邮件失败: {}", e))?;
    let mut out = Vec::new();
    for f in fetches.iter() {
        let uid = match f.uid {
            Some(u) => u,
            None => continue,
        };
        let raw = match f.body() {
            Some(b) => b.to_vec(),
            None => continue,
        };
        let unread = !f.flags().iter().any(|fl| matches!(fl, imap::types::Flag::Seen));
        out.push(RawMail { uid, unread, raw });
    }
    let _ = sess.logout();
    out.reverse(); // 最新的在前
    Ok(out)
}

pub fn create_folder(account: &Account, secret: &AccountSecret, name: &str) -> Result<(), String> {
    let mut sess = connect(account, secret)?;
    sess.create(encode_mutf7(name))
        .map_err(|e| format!("创建目录失败: {}", e))?;
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

pub fn delete_message(
    account: &Account,
    secret: &AccountSecret,
    folder: &str,
    uid: u32,
) -> Result<(), String> {
    let mut sess = connect(account, secret)?;
    sess.select(folder).map_err(|e| e.to_string())?;
    sess.uid_store(uid.to_string(), "+FLAGS (\\Deleted)")
        .map_err(|e| e.to_string())?;
    sess.expunge().map_err(|e| e.to_string())?;
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
