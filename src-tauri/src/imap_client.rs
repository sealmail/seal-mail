use crate::models::*;
use native_tls::TlsConnector;

type ImapSession = imap::Session<native_tls::TlsStream<std::net::TcpStream>>;

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
    client
        .login(&account.username, &secret.password)
        .map_err(|(e, _)| format!("IMAP 登录失败（请检查用户名/密码或应用专用密码）: {}", e))
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
            let display = name
                .rsplit(n.delimiter().unwrap_or("/"))
                .next()
                .unwrap_or(&name)
                .to_string();
            FolderInfo { name, display }
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
    sess.create(name).map_err(|e| format!("创建目录失败: {}", e))?;
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
    let mut sess = connect(account, secret)?;
    sess.select(folder).map_err(|e| e.to_string())?;
    let op = if read { "+FLAGS (\\Seen)" } else { "-FLAGS (\\Seen)" };
    sess.uid_store(uid.to_string(), op).map_err(|e| e.to_string())?;
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
