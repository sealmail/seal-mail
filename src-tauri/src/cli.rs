use crate::core::{self, default_config_dir, Core};
use crate::models::*;
use serde::Deserialize;
use serde::Serialize;
use std::io::Read;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountJsonPayload {
    account: Account,
    secret: AccountSecret,
}

fn print_json<T: Serialize>(value: &T) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    println!("{json}");
    Ok(())
}

fn print_help() {
    println!(
        "SealMail CLI\n\nUSAGE:\n  sealmail-cli <command> [--json]\n\nCOMMANDS:\n  state                Show accounts, identity, trusted contacts, filters, and local folders\n  accounts             Show configured accounts without secrets\n  account add          Test and save a password/app-password account\n  account test         Test a password/app-password account without saving\n  account remove       Remove a saved account by id\n  folders              List folders for a saved account\n  folder create        Create a server folder for a saved account\n  folder delete        Delete a server folder for a saved account\n  sync                 Sync a folder from the server\n  sync-older           Backfill older cached messages for a folder\n  sync-status          Show per-folder sync progress (cached vs server total)\n  list                 List locally cached messages\n  read                 Read a locally cached message\n  locate               Find which folder a message is in by Message-ID\n  thread               List locally cached messages in a conversation\n  send                 Send mail from a saved account\n  move                 Move a message to another folder\n  archive              Archive a message\n  delete               Delete a message, soft by default\n  mark                 Mark messages read or unread\n  flag                 Flag or unflag a message\n  attachment save      Save an attachment from a cached message\n  attachment data      Read an attachment as base64 (for preview)\n  draft save           Save or overwrite a local draft\n  draft delete         Delete a local draft\n  drafts               Show local drafts\n  filter save          Save or overwrite a filter rule\n  filter delete        Delete a filter rule\n  filter apply         Apply filter rules to INBOX\n  filters              Show filter rules\n  trust add            Add or overwrite a trusted sender\n  trust remove         Remove a trusted sender\n  trusted              Show trusted contacts\n  real-test daily-flow Run a real IMAP/SMTP daily user flow against a saved test account\n  identity             Show active signing identity\n  identity use-local   Switch signing back to the local key\n  identity bind-ledger Bind a Ledger-derived signing address\n  prefs                Show local preferences\n  pref set             Update local preferences\n  contacts             Show collected contacts\n  config-dir           Show the config directory used by the CLI\n  help                 Show this help\n\nACCOUNT FLAGS:\n  --id <id>\n  --label <label>\n  --email <email>\n  --display-name <name>\n  --protocol <imap|pop3>\n  --incoming-host <host>\n  --incoming-port <port>\n  --smtp-host <host>\n  --smtp-port <port>\n  --smtp-security <ssl|starttls>\n  --username <username>\n\nCOMMON FLAGS:\n  --account <id>\n  --folder <name>\n  --uid <uid>\n  --target <folder>\n  --thread <thread-id>\n  --limit <n>\n  --offset <n>\n  --id <id>\n\nSEND/DRAFT FLAGS:\n  --to <a@b,c@d>\n  --cc <a@b,c@d>\n  --subject <subject>\n  --body <text>\n  --body-file <path>\n  --attach <path>  May be repeated\n  --no-sign        Send without SealMail signature\n\nFILTER FLAGS:\n  --name <name>\n  --field <from|to|subject|body>\n  --op <contains|not_contains|equals|starts_with|ends_with>\n  --value <text>\n  --target <folder>\n  --mark-read <true|false>\n  --enabled <true|false>\n\nTRUST/ATTACHMENT/IDENTITY/PREF FLAGS:\n  --name <name>\n  --email <email>\n  --fingerprint <fingerprint>\n  --org <org>\n  --index <n>\n  --path <file>\n  --ledger-path <path>\n  --address <0x...>\n  --close-behavior <hide|quit>\n  --notify-new-mail <true|false>\n  --language <system|zh|en>\n\nENV:\n  SEALMAIL_CONFIG_DIR      Override the default SealMail config directory\n  SEALMAIL_PASSWORD        Incoming password or app-specific password for account add/test\n  SEALMAIL_SMTP_PASSWORD   Optional SMTP password when different from incoming password"
    );
}

fn has_json_flag(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--json")
}

fn flag_value(args: &[String], name: &str) -> Result<Option<String>, String> {
    let Some(pos) = args.iter().position(|arg| arg == name) else {
        return Ok(None);
    };
    let Some(value) = args.get(pos + 1) else {
        return Err(format!("{name} 缺少值"));
    };
    if value.starts_with("--") {
        return Err(format!("{name} 缺少值"));
    }
    Ok(Some(value.clone()))
}

fn required_flag(args: &[String], name: &str) -> Result<String, String> {
    flag_value(args, name)?.ok_or_else(|| format!("缺少必填参数 {name}"))
}

fn parse_port(args: &[String], name: &str) -> Result<u16, String> {
    let value = required_flag(args, name)?;
    value
        .parse()
        .map_err(|_| format!("{name} 必须是 1-65535 的端口号"))
}

fn parse_u32_flag(args: &[String], name: &str) -> Result<u32, String> {
    let value = required_flag(args, name)?;
    value.parse().map_err(|_| format!("{name} 必须是非负整数"))
}

fn optional_u32_flag(args: &[String], name: &str, default: u32) -> Result<u32, String> {
    match flag_value(args, name)? {
        Some(value) => value.parse().map_err(|_| format!("{name} 必须是非负整数")),
        None => Ok(default),
    }
}

fn optional_usize_flag(args: &[String], name: &str, default: usize) -> Result<usize, String> {
    match flag_value(args, name)? {
        Some(value) => value.parse().map_err(|_| format!("{name} 必须是非负整数")),
        None => Ok(default),
    }
}

fn parse_bool_flag(args: &[String], name: &str, default: bool) -> Result<bool, String> {
    match flag_value(args, name)? {
        Some(value) => match value.as_str() {
            "true" | "yes" | "1" => Ok(true),
            "false" | "no" | "0" => Ok(false),
            _ => Err(format!("{name} 只能是 true/false")),
        },
        None => Ok(default),
    }
}

fn csv_values(value: Option<String>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn repeated_flag_values(args: &[String], name: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < args.len() {
        if args[i] == name {
            let Some(value) = args.get(i + 1) else {
                return Err(format!("{name} 缺少值"));
            };
            if value.starts_with("--") {
                return Err(format!("{name} 缺少值"));
            }
            out.push(value.clone());
            i += 2;
        } else {
            i += 1;
        }
    }
    Ok(out)
}

fn read_body(args: &[String]) -> Result<String, String> {
    if let Some(body) = flag_value(args, "--body")? {
        return Ok(body);
    }
    if let Some(path) = flag_value(args, "--body-file")? {
        return std::fs::read_to_string(&path).map_err(|e| format!("读取正文文件失败 {path}: {e}"));
    }
    let mut body = String::new();
    std::io::stdin()
        .read_to_string(&mut body)
        .map_err(|e| format!("读取 stdin 正文失败: {e}"))?;
    if body.is_empty() {
        return Err("缺少正文：请使用 --body、--body-file 或 stdin".into());
    }
    Ok(body)
}

fn read_stdin_string() -> Result<String, String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("读取 stdin 失败: {e}"))?;
    if input.trim().is_empty() {
        return Err("stdin JSON 不能为空".into());
    }
    Ok(input)
}

fn read_account_payload() -> Result<AccountJsonPayload, String> {
    let input = read_stdin_string()?;
    serde_json::from_str(&input).map_err(|e| format!("账户 JSON 无效: {e}"))
}

fn load_attachments(args: &[String]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut files = Vec::new();
    for path in repeated_flag_values(args, "--attach")? {
        let name = std::path::Path::new(&path)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("附件路径无效: {path}"))?
            .to_string();
        let data = std::fs::read(&path).map_err(|e| format!("读取附件失败 {path}: {e}"))?;
        files.push((name, data));
    }
    Ok(files)
}

/// 取账户凭据；OAuth 临近过期时阻塞刷新并写回 secrets.json（CLI 子进程无 watcher 时也要能续命）。
fn account_secret(core: &mut Core, account_id: &str) -> Result<AccountSecret, String> {
    let mut secret = core.data.secret(account_id)?;
    let Some(tokens) = secret.oauth.clone() else {
        return Ok(secret);
    };
    if !tokens.needs_refresh() {
        return Ok(secret);
    }
    let refreshed = crate::oauth::resolve_proactive_refresh(
        &tokens,
        crate::oauth::refresh_tokens_blocking(&tokens),
    )?;
    secret.oauth = Some(refreshed);
    core.data.update_secret(account_id, secret.clone())?;
    Ok(secret)
}

/// 网络操作 + OAuth 被服务器拒绝时强制刷新令牌并重试一次。
fn with_oauth_retry<T>(
    core: &mut Core,
    account_id: &str,
    mut op: impl FnMut(&mut Core, &AccountSecret) -> Result<T, String>,
) -> Result<T, String> {
    let secret = account_secret(core, account_id)?;
    let has_oauth = secret.oauth.is_some();
    match op(core, &secret) {
        Err(e) if has_oauth && crate::oauth::is_auth_rejected(&e) => {
            let mut secret = core.data.secret(account_id)?;
            let Some(tokens) = secret.oauth.clone() else {
                return Err(e);
            };
            let refreshed = crate::oauth::refresh_tokens_blocking(&tokens).map_err(|re| {
                format!("{e}；强制刷新令牌失败: {re}")
            })?;
            secret.oauth = Some(refreshed);
            core.data.update_secret(account_id, secret.clone())?;
            op(core, &secret)
        }
        other => other,
    }
}

fn parse_protocol(value: &str) -> Result<IncomingProtocol, String> {
    match value.to_ascii_lowercase().as_str() {
        "imap" => Ok(IncomingProtocol::Imap),
        "pop3" => Ok(IncomingProtocol::Pop3),
        _ => Err("--protocol 只能是 imap 或 pop3".into()),
    }
}

fn parse_uid_list(args: &[String]) -> Result<Vec<u32>, String> {
    if let Some(csv) = flag_value(args, "--uids")? {
        let mut out = Vec::new();
        for value in csv.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            out.push(
                value
                    .parse()
                    .map_err(|_| "--uids 必须是逗号分隔的非负整数".to_string())?,
            );
        }
        if out.is_empty() {
            return Err("--uids 不能为空".into());
        }
        return Ok(out);
    }
    Ok(vec![parse_u32_flag(args, "--uid")?])
}

fn build_account_secret(args: &[String]) -> Result<(Account, AccountSecret), String> {
    let email = required_flag(args, "--email")?;
    let protocol = parse_protocol(&required_flag(args, "--protocol")?)?;
    let password = std::env::var("SEALMAIL_PASSWORD")
        .map_err(|_| "请用环境变量 SEALMAIL_PASSWORD 提供密码或应用专用密码".to_string())?;
    if password.is_empty() {
        return Err("SEALMAIL_PASSWORD 不能为空".into());
    }
    let smtp_password = match std::env::var("SEALMAIL_SMTP_PASSWORD") {
        Ok(value) if !value.is_empty() => Some(value),
        Ok(_) => return Err("SEALMAIL_SMTP_PASSWORD 不能是空字符串".into()),
        Err(_) => None,
    };
    let username = match flag_value(args, "--username")? {
        Some(value) => value,
        None => email.clone(),
    };
    let label = match flag_value(args, "--label")? {
        Some(value) => value,
        None => email.clone(),
    };
    let display_name = match flag_value(args, "--display-name")? {
        Some(value) => value,
        None => email.clone(),
    };
    let smtp_security = required_flag(args, "--smtp-security")?;
    if smtp_security != "ssl" && smtp_security != "starttls" {
        return Err("--smtp-security 只能是 ssl 或 starttls".into());
    }

    Ok((
        Account {
            id: flag_value(args, "--id")?.unwrap_or_default(),
            label,
            email,
            display_name,
            protocol,
            incoming_host: required_flag(args, "--incoming-host")?,
            incoming_port: parse_port(args, "--incoming-port")?,
            smtp_host: required_flag(args, "--smtp-host")?,
            smtp_port: parse_port(args, "--smtp-port")?,
            smtp_security,
            username,
            auth: "password".into(),
        },
        AccountSecret {
            password,
            smtp_password,
            oauth: None,
        },
    ))
}

fn command_parts(args: &[String]) -> Vec<&str> {
    args.iter()
        .filter(|arg| !arg.starts_with("--"))
        .map(String::as_str)
        .collect()
}

pub fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let parts = command_parts(&args);
    let command = parts.first().copied().unwrap_or("help");

    if command == "help" || command == "-h" || command == "--help" {
        print_help();
        return Ok(());
    }

    let dir = default_config_dir()?;
    if command == "config-dir" {
        println!("{}", dir.display());
        return Ok(());
    }

    let mut core = Core::load(dir)?;
    let json = has_json_flag(&args);

    match command {
        "state" => {
            let view = core.state_view();
            if json {
                print_json(&view)
            } else {
                println!("Accounts: {}", view.accounts.len());
                println!("Trusted contacts: {}", view.trusted.len());
                println!("Filters: {}", view.filters.len());
                println!("Local folders: {}", view.local_folders.len());
                println!("Identity: {}", view.identity.fingerprint);
                println!("Mode: {}", view.identity.mode);
                Ok(())
            }
        }
        "accounts" => {
            if json {
                print_json(&core.data.accounts)
            } else {
                for account in &core.data.accounts {
                    println!("{}  {}  {}", account.id, account.email, account.label);
                }
                Ok(())
            }
        }
        "account" => match parts.get(1).copied().unwrap_or("help") {
            "add" => {
                let (account, secret) = build_account_secret(&args)?;
                let saved = core::add_account(&mut core.data, account, secret)?;
                if json {
                    print_json(&saved)
                } else {
                    println!("Saved account: {}  {}", saved.id, saved.email);
                    Ok(())
                }
            }
            "add-json" => {
                let payload = read_account_payload()?;
                let saved = core::add_account(&mut core.data, payload.account, payload.secret)?;
                if json {
                    print_json(&saved)
                } else {
                    println!("Saved account: {}  {}", saved.id, saved.email);
                    Ok(())
                }
            }
            "test" => {
                let (account, secret) = build_account_secret(&args)?;
                core::test_connection(&account, &secret)?;
                if json {
                    print_json(&serde_json::json!({ "ok": true }))
                } else {
                    println!("Connection OK: {}", account.email);
                    Ok(())
                }
            }
            "test-json" => {
                let payload = read_account_payload()?;
                core::test_connection(&payload.account, &payload.secret)?;
                if json {
                    print_json(&serde_json::json!({ "ok": true }))
                } else {
                    println!("Connection OK: {}", payload.account.email);
                    Ok(())
                }
            }
            "remove" => {
                let account_id = required_flag(&args, "--id")?;
                core::remove_account(&mut core.data, account_id.clone())?;
                if json {
                    print_json(&serde_json::json!({ "removed": account_id }))
                } else {
                    println!("Removed account: {account_id}");
                    Ok(())
                }
            }
            "help" => {
                print_help();
                Ok(())
            }
            other => Err(format!("未知 account 子命令: {other}")),
        },
        "folders" => {
            let account_id = required_flag(&args, "--account")?;
            // with_oauth_retry 会先刷新令牌写回 store；list_folders 从 store 取凭据
            let folders = with_oauth_retry(&mut core, &account_id, |core, _| {
                core::list_folders(&core.data, &account_id)
            })?;
            if json {
                print_json(&folders)
            } else {
                for folder in &folders {
                    match &folder.role {
                        Some(role) => {
                            println!("{}  {}  role={}", folder.name, folder.display, role)
                        }
                        None => println!("{}  {}", folder.name, folder.display),
                    }
                }
                Ok(())
            }
        }
        "folder" => match parts.get(1).copied().unwrap_or("help") {
            "create" => {
                let account_id = required_flag(&args, "--account")?;
                let folder = required_flag(&args, "--folder")?;
                with_oauth_retry(&mut core, &account_id, |core, _| {
                    core::create_folder(&mut core.data, &account_id, folder.clone())
                })?;
                if json {
                    print_json(&serde_json::json!({ "created": folder }))
                } else {
                    println!("Created folder: {folder}");
                    Ok(())
                }
            }
            "delete" => {
                let account_id = required_flag(&args, "--account")?;
                let folder = required_flag(&args, "--folder")?;
                with_oauth_retry(&mut core, &account_id, |core, _| {
                    core::delete_folder(&mut core.data, &account_id, &folder)
                })?;
                if json {
                    print_json(&serde_json::json!({ "deleted": folder }))
                } else {
                    println!("Deleted folder: {folder}");
                    Ok(())
                }
            }
            "help" => {
                print_help();
                Ok(())
            }
            other => Err(format!("未知 folder 子命令: {other}")),
        },
        "sync" => {
            let account_id = required_flag(&args, "--account")?;
            let folder = required_flag(&args, "--folder")?;
            let result = with_oauth_retry(&mut core, &account_id, |core, secret| {
                core::sync_messages(&mut core.data, &account_id, &folder, secret)
            })?;
            if json {
                print_json(&result)
            } else {
                println!(
                    "Synced {}/{}: added={}, total={}",
                    account_id, folder, result.added, result.total
                );
                Ok(())
            }
        }
        "sync-status" => {
            let entries = core::sync_status(&core.data)?;
            if json {
                print_json(&entries)
            } else {
                for e in &entries {
                    println!(
                        "{}/{}: cached={} serverTotal={}",
                        e.account_id, e.folder, e.cached, e.server_total
                    );
                }
                Ok(())
            }
        }
        "sync-older" => {
            let account_id = required_flag(&args, "--account")?;
            let folder = required_flag(&args, "--folder")?;
            let result = with_oauth_retry(&mut core, &account_id, |core, secret| {
                core::sync_older_messages(&mut core.data, &account_id, &folder, secret)
            })?;
            if json {
                print_json(&result)
            } else {
                println!(
                    "Synced older {}/{}: added={}, total={}",
                    account_id, folder, result.added, result.total
                );
                Ok(())
            }
        }
        "list" => {
            let account_id = required_flag(&args, "--account")?;
            let folder = required_flag(&args, "--folder")?;
            let offset = optional_u32_flag(&args, "--offset", 0)?;
            let limit = optional_u32_flag(&args, "--limit", 20)?;
            let list = core::list_cached(&mut core.data, &account_id, &folder, offset, limit)?;
            if json {
                print_json(&list)
            } else {
                for meta in &list.metas {
                    let unread = if meta.unread { "unread" } else { "read" };
                    let flagged = if meta.flagged { " flagged" } else { "" };
                    println!(
                        "{}  [{}{}]  {}  {}",
                        meta.uid, unread, flagged, meta.from_addr, meta.subject
                    );
                }
                println!("Total: {}", list.total);
                Ok(())
            }
        }
        "read" => {
            let account_id = required_flag(&args, "--account")?;
            let folder = required_flag(&args, "--folder")?;
            let uid = parse_u32_flag(&args, "--uid")?;
            let msg = core::get_message(&mut core.data, &account_id, &folder, uid)?;
            if json {
                print_json(&msg)
            } else {
                println!("From: {} <{}>", msg.meta.from_name, msg.meta.from_addr);
                println!("Subject: {}", msg.meta.subject);
                println!("Date: {}", msg.meta.date_display);
                println!("Trust: {}", msg.meta.trust);
                println!();
                println!("{}", msg.body_text);
                Ok(())
            }
        }
        "locate" => {
            let account_id = required_flag(&args, "--account")?;
            let message_id = required_flag(&args, "--message-id")?;
            let loc = with_oauth_retry(&mut core, &account_id, |core, secret| {
                core::locate_message(&mut core.data, &account_id, secret, &message_id)
            })?;
            if json {
                print_json(&loc)
            } else {
                match loc {
                    Some(l) => println!("{}  {}", l.folder, l.uid),
                    None => println!("not found"),
                }
                Ok(())
            }
        }
        "thread" => {
            let account_id = required_flag(&args, "--account")?;
            let folder = required_flag(&args, "--folder")?;
            let thread_id = required_flag(&args, "--thread")?;
            let metas = core::list_thread(&mut core.data, &account_id, &folder, &thread_id)?;
            if json {
                print_json(&metas)
            } else {
                for meta in &metas {
                    println!("{}  {}  {}", meta.uid, meta.from_addr, meta.subject);
                }
                Ok(())
            }
        }
        "send" => {
            let account_id = required_flag(&args, "--account")?;
            let to = csv_values(flag_value(&args, "--to")?);
            if to.is_empty() {
                return Err("缺少 --to 收件人".into());
            }
            let cc = csv_values(flag_value(&args, "--cc")?);
            let subject = required_flag(&args, "--subject")?;
            let body = read_body(&args)?;
            let attachments = load_attachments(&args)?;
            let sign = !args.iter().any(|arg| arg == "--no-sign");
            let result = with_oauth_retry(&mut core, &account_id, |core, secret| {
                core::send_mail(
                    &mut core.data,
                    &account_id,
                    secret,
                    to.clone(),
                    cc.clone(),
                    &subject,
                    &body,
                    sign,
                    attachments.clone(),
                )
            })?;
            if json {
                print_json(&result)
            } else {
                println!(
                    "Sent: signed={} method={} fingerprint={}",
                    result.signed, result.method, result.fingerprint
                );
                Ok(())
            }
        }
        "move" => {
            let account_id = required_flag(&args, "--account")?;
            let folder = required_flag(&args, "--folder")?;
            let target = required_flag(&args, "--target")?;
            let uid = parse_u32_flag(&args, "--uid")?;
            with_oauth_retry(&mut core, &account_id, |core, secret| {
                core::move_message(&mut core.data, &account_id, &folder, uid, &target, secret)
            })?;
            if json {
                print_json(&serde_json::json!({ "moved": uid, "target": target }))
            } else {
                println!("Moved UID {uid} -> {target}");
                Ok(())
            }
        }
        "archive" => {
            let account_id = required_flag(&args, "--account")?;
            let folder = required_flag(&args, "--folder")?;
            let uid = parse_u32_flag(&args, "--uid")?;
            with_oauth_retry(&mut core, &account_id, |core, secret| {
                core::archive_message(&mut core.data, &account_id, &folder, uid, secret)
            })?;
            if json {
                print_json(&serde_json::json!({ "archived": uid }))
            } else {
                println!("Archived UID {uid}");
                Ok(())
            }
        }
        "delete" => {
            let account_id = required_flag(&args, "--account")?;
            let folder = required_flag(&args, "--folder")?;
            let uid = parse_u32_flag(&args, "--uid")?;
            let permanent = args.iter().any(|arg| arg == "--permanent");
            with_oauth_retry(&mut core, &account_id, |core, secret| {
                core::delete_message(
                    &mut core.data,
                    &account_id,
                    &folder,
                    uid,
                    permanent,
                    secret,
                )
            })?;
            if json {
                print_json(&serde_json::json!({ "deleted": uid, "permanent": permanent }))
            } else {
                println!("Deleted UID {uid}");
                Ok(())
            }
        }
        "mark" => {
            let account_id = required_flag(&args, "--account")?;
            let folder = required_flag(&args, "--folder")?;
            let uids = parse_uid_list(&args)?;
            let read = parse_bool_flag(&args, "--read", true)?;
            with_oauth_retry(&mut core, &account_id, |core, secret| {
                core::set_read(&mut core.data, &account_id, &folder, &uids, read, secret)
            })?;
            if json {
                print_json(&serde_json::json!({ "uids": uids, "read": read }))
            } else {
                println!("Marked {} message(s) read={read}", uids.len());
                Ok(())
            }
        }
        "flag" => {
            let account_id = required_flag(&args, "--account")?;
            let folder = required_flag(&args, "--folder")?;
            let uid = parse_u32_flag(&args, "--uid")?;
            let flagged = parse_bool_flag(&args, "--flagged", true)?;
            with_oauth_retry(&mut core, &account_id, |core, secret| {
                core::set_flagged(&mut core.data, &account_id, &folder, uid, flagged, secret)
            })?;
            if json {
                print_json(&serde_json::json!({ "uid": uid, "flagged": flagged }))
            } else {
                println!("Flagged UID {uid} flagged={flagged}");
                Ok(())
            }
        }
        "attachment" => match parts.get(1).copied().unwrap_or("help") {
            "save" => {
                let account_id = required_flag(&args, "--account")?;
                let folder = required_flag(&args, "--folder")?;
                let uid = parse_u32_flag(&args, "--uid")?;
                let index = optional_usize_flag(&args, "--index", 0)?;
                let path = required_flag(&args, "--path")?;
                let result = with_oauth_retry(&mut core, &account_id, |core, secret| {
                    core::save_attachment(
                        &core.data,
                        &account_id,
                        &folder,
                        uid,
                        index,
                        &path,
                        Some(secret),
                    )
                })?;
                if json {
                    print_json(&result)
                } else {
                    println!(
                        "Saved attachment {} ({} bytes) -> {}",
                        result.filename, result.bytes, result.path
                    );
                    Ok(())
                }
            }
            "data" => {
                let account_id = required_flag(&args, "--account")?;
                let folder = required_flag(&args, "--folder")?;
                let uid = parse_u32_flag(&args, "--uid")?;
                let index = optional_usize_flag(&args, "--index", 0)?;
                let result = with_oauth_retry(&mut core, &account_id, |core, secret| {
                    core::read_attachment(
                        &core.data,
                        &account_id,
                        &folder,
                        uid,
                        index,
                        Some(secret),
                    )
                })?;
                if json {
                    print_json(&result)
                } else {
                    println!(
                        "Attachment {} ({}, {} bytes, base64 {} chars)",
                        result.filename,
                        result.mime,
                        result.bytes,
                        result.data_base64.len()
                    );
                    Ok(())
                }
            }
            "help" => {
                print_help();
                Ok(())
            }
            other => Err(format!("未知 attachment 子命令: {other}")),
        },
        "draft" => match parts.get(1).copied().unwrap_or("help") {
            "save" => {
                let draft = Draft {
                    id: flag_value(&args, "--id")?.unwrap_or_default(),
                    account_id: required_flag(&args, "--account")?,
                    to: flag_value(&args, "--to")?.unwrap_or_default(),
                    cc: flag_value(&args, "--cc")?.unwrap_or_default(),
                    subject: required_flag(&args, "--subject")?,
                    body: read_body(&args)?,
                    sign: !args.iter().any(|arg| arg == "--no-sign"),
                    attachment_paths: repeated_flag_values(&args, "--attach")?,
                    updated_at: 0,
                };
                let saved = core::save_draft(&mut core.data, draft)?;
                if json {
                    print_json(&saved)
                } else {
                    println!("Saved draft: {}  {}", saved.id, saved.subject);
                    Ok(())
                }
            }
            "delete" => {
                let id = required_flag(&args, "--id")?;
                core::delete_draft(&mut core.data, id.clone())?;
                if json {
                    print_json(&serde_json::json!({ "deleted": id }))
                } else {
                    println!("Deleted draft: {id}");
                    Ok(())
                }
            }
            "help" => {
                print_help();
                Ok(())
            }
            other => Err(format!("未知 draft 子命令: {other}")),
        },
        "filter" => match parts.get(1).copied().unwrap_or("help") {
            "save" => {
                let rule = FilterRule {
                    id: flag_value(&args, "--id")?.unwrap_or_default(),
                    name: required_flag(&args, "--name")?,
                    account_id: flag_value(&args, "--account")?,
                    field: required_flag(&args, "--field")?,
                    op: required_flag(&args, "--op")?,
                    value: required_flag(&args, "--value")?,
                    target_folder: required_flag(&args, "--target")?,
                    mark_read: parse_bool_flag(&args, "--mark-read", false)?,
                    enabled: parse_bool_flag(&args, "--enabled", true)?,
                };
                let rules = core::save_filter(&mut core.data, rule)?;
                if json {
                    print_json(&rules)
                } else {
                    println!("Saved filters: {}", rules.len());
                    Ok(())
                }
            }
            "delete" => {
                let id = required_flag(&args, "--id")?;
                let rules = core::delete_filter(&mut core.data, id.clone())?;
                if json {
                    print_json(&rules)
                } else {
                    println!("Deleted filter: {id}");
                    Ok(())
                }
            }
            "apply" => {
                let account_id = required_flag(&args, "--account")?;
                let result = with_oauth_retry(&mut core, &account_id, |core, secret| {
                    core::apply_filters(&mut core.data, &account_id, secret)
                })?;
                if json {
                    print_json(&result)
                } else {
                    println!("Applied filters: moved={}", result.moved);
                    for detail in &result.details {
                        println!("- {detail}");
                    }
                    Ok(())
                }
            }
            "help" => {
                print_help();
                Ok(())
            }
            other => Err(format!("未知 filter 子命令: {other}")),
        },
        "trust" => match parts.get(1).copied().unwrap_or("help") {
            "add" => {
                let trusted = core::trust_sender(
                    &mut core.data,
                    required_flag(&args, "--name")?,
                    required_flag(&args, "--email")?,
                    required_flag(&args, "--fingerprint")?,
                    flag_value(&args, "--org")?,
                )?;
                if json {
                    print_json(&trusted)
                } else {
                    println!("Trusted contacts: {}", trusted.len());
                    Ok(())
                }
            }
            "remove" => {
                let email = required_flag(&args, "--email")?;
                let trusted = core::remove_trusted(&mut core.data, email.clone())?;
                if json {
                    print_json(&trusted)
                } else {
                    println!("Removed trusted sender: {email}");
                    Ok(())
                }
            }
            "help" => {
                print_help();
                Ok(())
            }
            other => Err(format!("未知 trust 子命令: {other}")),
        },
        "real-test" => match parts.get(1).copied().unwrap_or("help") {
            "daily-flow" => {
                let account_id = required_flag(&args, "--account")?;
                let report = core::run_imap_daily_flow(&mut core.data, &account_id)?;
                if json {
                    print_json(&report)
                } else {
                    println!("Daily flow OK: {}", report.subject);
                    for step in &report.steps {
                        println!("- {step}");
                    }
                    Ok(())
                }
            }
            "help" => {
                print_help();
                Ok(())
            }
            other => Err(format!("未知 real-test 子命令: {other}")),
        },
        "identity" => match parts.get(1).copied().unwrap_or("show") {
            "show" => {
                let identity = core.identity_info();
                if json {
                    print_json(&identity)
                } else {
                    println!("Fingerprint: {}", identity.fingerprint);
                    println!("Mode: {}", identity.mode);
                    println!("Created: {}", identity.created);
                    if let Some(address) = identity.ledger_address {
                        println!("Ledger address: {address}");
                    }
                    Ok(())
                }
            }
            "use-local" => {
                let identity = core::use_local_key(&mut core.data)?;
                if json {
                    print_json(&identity)
                } else {
                    println!("Identity mode: {}", identity.mode);
                    Ok(())
                }
            }
            "bind-ledger" => {
                let path = required_flag(&args, "--ledger-path")?;
                let address = required_flag(&args, "--address")?;
                let identity = core::bind_ledger(&mut core.data, path, address)?;
                if json {
                    print_json(&identity)
                } else {
                    println!("Identity mode: {}", identity.mode);
                    if let Some(address) = identity.ledger_address {
                        println!("Ledger address: {address}");
                    }
                    Ok(())
                }
            }
            "help" => {
                print_help();
                Ok(())
            }
            other => Err(format!("未知 identity 子命令: {other}")),
        },
        "prefs" => {
            if json {
                print_json(&core.data.prefs)
            } else {
                println!("Close behavior: {}", core::get_close_behavior(&core.data));
                println!(
                    "Notify new mail: {}",
                    core::get_notify_new_mail(&core.data)
                );
                Ok(())
            }
        }
        "pref" => match parts.get(1).copied().unwrap_or("help") {
            "set" => {
                let mut changed = serde_json::Map::new();
                if let Some(behavior) = flag_value(&args, "--close-behavior")? {
                    let value = core::set_close_behavior(&mut core.data, behavior)?;
                    changed.insert("closeBehavior".into(), serde_json::Value::String(value));
                }
                if let Some(value) = flag_value(&args, "--notify-new-mail")? {
                    let enabled = match value.as_str() {
                        "true" | "yes" | "1" => true,
                        "false" | "no" | "0" => false,
                        _ => return Err("--notify-new-mail 只能是 true/false".into()),
                    };
                    let value = core::set_notify_new_mail(&mut core.data, enabled)?;
                    changed.insert("notifyNewMail".into(), serde_json::Value::Bool(value));
                }
                if let Some(language) = flag_value(&args, "--language")? {
                    let value = core::set_language(&mut core.data, language)?;
                    changed.insert("language".into(), serde_json::Value::String(value));
                }
                if let Some(theme) = flag_value(&args, "--theme")? {
                    let value = core::set_theme(&mut core.data, theme)?;
                    changed.insert("theme".into(), serde_json::Value::String(value));
                }
                if changed.is_empty() {
                    return Err(
                        "pref set 需要 --close-behavior、--notify-new-mail、--language 或 --theme"
                            .into(),
                    );
                }
                if json {
                    print_json(&serde_json::Value::Object(changed))
                } else {
                    println!("Updated preferences");
                    Ok(())
                }
            }
            "help" => {
                print_help();
                Ok(())
            }
            other => Err(format!("未知 pref 子命令: {other}")),
        },
        "contacts" => {
            let contacts = core.contacts(flag_value(&args, "--query")?);
            if json {
                print_json(&contacts)
            } else {
                for contact in &contacts {
                    println!(
                        "{}  {}  count={}  last_seen={}",
                        contact.email, contact.name, contact.count, contact.last_seen
                    );
                }
                Ok(())
            }
        }
        "drafts" => {
            let drafts = core.drafts();
            if json {
                print_json(&drafts)
            } else {
                for draft in &drafts {
                    println!("{}  {}  {}", draft.id, draft.account_id, draft.subject);
                }
                Ok(())
            }
        }
        "trusted" => {
            if json {
                print_json(&core.data.trusted)
            } else {
                for contact in &core.data.trusted {
                    println!(
                        "{}  {}  {}",
                        contact.email, contact.name, contact.fingerprint
                    );
                }
                Ok(())
            }
        }
        "filters" => {
            if json {
                print_json(&core.data.filters)
            } else {
                for rule in &core.data.filters {
                    println!(
                        "{}  {}  {} {} -> {}",
                        rule.id, rule.name, rule.field, rule.op, rule.target_folder
                    );
                }
                Ok(())
            }
        }
        other => Err(format!("未知命令: {other}")),
    }
}

pub fn main_entry() {
    if let Err(err) = run() {
        // 用户可见错误的唯一出口：GUI 的 cli_json 把 stderr 作为错误信息展示，
        // 双语翻译统一在这里做（见 i18n::tr_error）
        eprintln!("error: {}", crate::i18n::tr_error(&err));
        std::process::exit(1);
    }
}
