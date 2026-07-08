//! 用户可见文案的双语支持（错误信息、系统通知）。
//! gettext 风格：中文原文即 key，`tr()` 在英文环境查词典、缺条目回退中文。
//! 日志（logging::log / eprintln 调试输出）保持中文，不走 tr()。
//!
//! 错误信息不逐处改造：所有 GUI 可见的后端错误都经 CLI 出口（cli::main_entry 的
//! stderr）返回，`tr_error()` 在该出口按「整条 → 冒号前缀 → 起始短语」三级匹配翻译。
//!
//! 语言来源：prefs.language（"system" | "zh" | "en"），StoreData::load 时写入全局；
//! "system" 按系统 locale 解析。CLI 子进程每次启动都会重新加载，GUI 常驻进程在
//! pref set 成功后由 lib.rs 刷新。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// true = 英文界面。默认中文。
static ENGLISH: AtomicBool = AtomicBool::new(false);

pub fn set_lang_from_pref(pref: &str) {
    let english = match pref {
        "zh" => false,
        "en" => true,
        _ => system_is_english(),
    };
    ENGLISH.store(english, Ordering::Relaxed);
}

pub fn is_english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

fn system_is_english() -> bool {
    // 环境变量优先（CLI/终端场景）；GUI 进程没有 LANG 时问系统 locale
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(v) = std::env::var(var) {
            if !v.is_empty() {
                return !v.to_lowercase().starts_with("zh");
            }
        }
    }
    match sys_locale::get_locale() {
        Some(l) => !l.to_lowercase().starts_with("zh"),
        None => false,
    }
}

/// 翻译一条用户可见文案。英文环境查词典，缺条目回退中文原文。
pub fn tr(zh: &str) -> String {
    if !is_english() {
        return zh.to_string();
    }
    dict()
        .get(zh)
        .map(|s| s.to_string())
        .unwrap_or_else(|| zh.to_string())
}

/// 带一个 {n} 占位的文案
pub fn tr_n(zh_template: &str, n: impl std::fmt::Display) -> String {
    tr(zh_template).replace("{n}", &n.to_string())
}

/// 错误信息统一出口翻译：整条命中 → 「前缀: 动态内容」按第一个冒号拆分试译前缀
/// → 起始短语命中（无冒号的少数格式）。都不中则原样返回（回退中文）。
pub fn tr_error(msg: &str) -> String {
    if !is_english() {
        return msg.to_string();
    }
    let d = dict();
    if let Some(t) = d.get(msg) {
        return t.to_string();
    }
    for sep in [": ", "：", ":"] {
        if let Some((head, rest)) = msg.split_once(sep) {
            if let Some(t) = d.get(head) {
                return format!("{}: {}", t, rest);
            }
        }
    }
    for (zh, en) in PREFIX_ENTRIES {
        if let Some(rest) = msg.strip_prefix(zh) {
            return format!("{}{}", en, rest);
        }
    }
    msg.to_string()
}

fn dict() -> &'static HashMap<&'static str, &'static str> {
    static DICT: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    DICT.get_or_init(|| ENTRIES.iter().copied().collect())
}

/// 起始短语式错误（动态部分不以冒号分隔）
static PREFIX_ENTRIES: &[(&str, &str)] = &[
    ("无法连接 ", "Cannot connect to "),
    ("无法打开目录 ", "Cannot open folder "),
    ("账户不存在", "Account not found"),
    ("未知命令", "Unknown command"),
    ("缺少必填参数 ", "Missing required argument "),
    ("读取附件失败 ", "Failed to read attachment "),
    ("读取正文文件失败 ", "Failed to read body file "),
];

/// 中文 → 英文。只收录用户可见的文案（Err 返回值、系统通知），按模块归组。
static ENTRIES: &[(&str, &str)] = &[
    // ── prefs / core ──
    ("无效的语言", "Invalid language"),
    ("无效的关闭行为", "Invalid close behavior"),
    ("邮件不在本地缓存中，请刷新列表", "Message not in local cache; refresh the list"),
    ("邮件不在本地缓存中，请刷新列表后重试", "Message not in local cache; refresh the list and retry"),
    ("附件不存在（邮件可能已变化，请刷新后重试）", "Attachment not found (the message may have changed; refresh and retry)"),
    ("本地缓存缺少该邮件的服务器标识，请刷新后重试", "Local cache is missing this message's server ID; refresh and retry"),
    ("邮件已不在服务器上", "The message is no longer on the server"),
    ("附件路径无效", "Invalid attachment path"),
    ("保存附件失败", "Failed to save attachment"),
    ("Ledger 未绑定派生路径，请在「身份与密钥」重新绑定", "Ledger derivation path not bound; re-bind it in Identity & Keys"),
    ("Ledger 未绑定地址，请在「身份与密钥」重新绑定", "Ledger address not bound; re-bind it in Identity & Keys"),
    // ── IMAP ──
    ("获取目录失败", "Failed to list folders"),
    ("创建目录失败", "Failed to create folder"),
    ("删除目录失败", "Failed to delete folder"),
    ("创建回收站目录失败", "Failed to create the Trash folder"),
    ("创建归档目录失败", "Failed to create the Archive folder"),
    ("移动失败（目标目录可能不存在）", "Move failed (target folder may not exist)"),
    ("归档失败", "Archive failed"),
    ("发送失败", "Send failed"),
    ("同步失败", "Sync failed"),
    ("IMAP 登录失败（请检查用户名/密码或应用专用密码）", "IMAP sign-in failed (check username/password or app-specific password)"),
    ("IMAP OAuth2 登录失败（授权可能已失效，请重新授权）", "IMAP OAuth2 sign-in failed (authorization may have expired; re-authorize)"),
    ("IMAP 目录列表缺少 INBOX", "IMAP folder list has no INBOX"),
    ("TLS 握手失败", "TLS handshake failed"),
    ("TLS 初始化失败", "TLS initialization failed"),
    ("邮件不存在（可能已被移动或删除）", "Message not found (it may have been moved or deleted)"),
    ("探测新邮件失败", "Failed to check for new mail"),
    // ── SMTP ──
    ("SMTP 配置错误", "SMTP configuration error"),
    ("SMTP 连接失败", "SMTP connection failed"),
    ("构建邮件失败", "Failed to build the message"),
    ("收件地址格式错误", "Invalid recipient address"),
    ("发件地址格式错误", "Invalid sender address"),
    // ── POP3 ──
    ("POP3 登录失败（请检查密码或应用专用密码）", "POP3 sign-in failed (check password or app-specific password)"),
    ("POP3 OAuth2 登录失败（授权可能已失效，请重新授权）", "POP3 OAuth2 sign-in failed (authorization may have expired; re-authorize)"),
    ("STAT 响应异常", "Unexpected STAT response"),
    ("UIDL 响应异常", "Unexpected UIDL response"),
    // ── OAuth ──
    ("OAuth2 授权已失效，请在账户设置中重新授权", "OAuth2 authorization expired; re-authorize in account settings"),
    ("Google 授权失败", "Google authorization failed"),
    ("Google 登录地址无效", "Invalid Google sign-in URL"),
    ("Google 登录回调为空", "Empty Google sign-in callback"),
    ("Google 登录回调格式无效", "Invalid Google sign-in callback"),
    ("Google 登录回调缺少授权码", "Google sign-in callback is missing the authorization code"),
    ("Google 登录回调缺少 state", "Google sign-in callback is missing state"),
    ("Google 登录 state 校验失败，请重新授权", "Google sign-in state check failed; re-authorize"),
    ("Google 登录会话已失效，请重新开始", "Google sign-in session expired; start over"),
    ("等待 Google 登录回调超时，请重新授权", "Timed out waiting for the Google sign-in callback; re-authorize"),
    ("启动 Google 登录本机回调失败", "Failed to start the local Google sign-in callback"),
    ("设置 Google 登录超时失败", "Failed to set the Google sign-in timeout"),
    ("浏览器登录目前仅用于 Gmail / Google", "Browser sign-in is only for Gmail / Google"),
    ("Gmail OAuth2 需要填写 Google Cloud Desktop OAuth Client ID", "Gmail OAuth2 requires a Google Cloud Desktop OAuth Client ID"),
    ("Gmail OAuth2 需要填写 Google Cloud OAuth Client ID", "Gmail OAuth2 requires a Google Cloud OAuth Client ID"),
    ("Gmail OAuth2 需要填写 Google Cloud OAuth Client Secret", "Gmail OAuth2 requires a Google Cloud OAuth Client Secret"),
    ("不支持的 OAuth2 服务商", "Unsupported OAuth2 provider"),
    ("令牌响应缺少 access_token", "Token response is missing access_token"),
    ("令牌响应缺少 expires_in", "Token response is missing expires_in"),
    ("令牌响应缺少 refresh_token（请确认 offline_access 权限）", "Token response is missing refresh_token (check the offline_access scope)"),
    ("响应缺少 device_code", "Response is missing device_code"),
    ("响应缺少 user_code", "Response is missing user_code"),
    ("响应缺少 verification_uri", "Response is missing verification_uri"),
    ("生成 OAuth 随机数失败", "Failed to generate an OAuth nonce"),
    // ── Ledger ──
    ("未找到 Ledger。请插入设备、解锁并打开 Ethereum app。", "Ledger not found. Plug it in, unlock it, and open the Ethereum app."),
    ("请先解锁 Ledger 并打开 Ethereum app。", "Unlock the Ledger and open the Ethereum app first."),
    ("请在 Ledger 上打开 Ethereum app 后重试。", "Open the Ethereum app on the Ledger and retry."),
    ("已在 Ledger 设备上拒绝签名。", "Signature rejected on the Ledger device."),
    ("Ledger 超时——请在设备上确认或取消。", "Ledger timed out — confirm or cancel on the device."),
    ("Ledger 拒绝了数据——请在 Ethereum app 设置里开启 blind signing。", "Ledger rejected the data — enable blind signing in the Ethereum app settings."),
    ("无法打开 Ledger 设备", "Cannot open the Ledger device"),
    ("写入 Ledger 失败", "Failed to write to the Ledger"),
    ("HID 初始化失败", "HID initialization failed"),
    ("设备返回的签名与绑定地址不符（恢复出 {recovered}，期望 {address}）。请确认 Ledger 上选择的是绑定时的账户。",
     "The device signature doesn't match the bound address. Make sure the Ledger has the originally bound account selected."),
    // ── 存储 ──
    ("打开邮件缓存失败", "Failed to open the mail cache"),
    ("初始化邮件缓存失败", "Failed to initialize the mail cache"),
    ("邮件缓存读写失败", "Mail cache read/write failed"),
    // ── CLI 参数 ──
    ("缺少 --to 收件人", "Missing --to recipients"),
    ("缺少正文：请使用 --body、--body-file 或 stdin", "Missing body: use --body, --body-file, or stdin"),
    ("stdin JSON 不能为空", "stdin JSON must not be empty"),
    ("读取 stdin 失败", "Failed to read stdin"),
    ("读取 stdin 正文失败", "Failed to read body from stdin"),
    ("账户 JSON 无效", "Invalid account JSON"),
    ("--notify-new-mail 只能是 true/false", "--notify-new-mail must be true/false"),
    ("--protocol 只能是 imap 或 pop3", "--protocol must be imap or pop3"),
    ("--smtp-security 只能是 ssl 或 starttls", "--smtp-security must be ssl or starttls"),
    ("--uids 不能为空", "--uids must not be empty"),
    ("--uids 必须是逗号分隔的非负整数", "--uids must be comma-separated non-negative integers"),
    ("pref set 需要 --close-behavior、--notify-new-mail 或 --language", "pref set requires --close-behavior, --notify-new-mail, or --language"),
    ("请用环境变量 SEALMAIL_PASSWORD 提供密码或应用专用密码", "Provide the password or app-specific password via SEALMAIL_PASSWORD"),
    ("SEALMAIL_PASSWORD 不能为空", "SEALMAIL_PASSWORD must not be empty"),
    ("SEALMAIL_SMTP_PASSWORD 不能是空字符串", "SEALMAIL_SMTP_PASSWORD must not be an empty string"),
    // ── 通知（watcher）──
    ("收到新邮件", "New mail"),
    ("收到 {n} 封新邮件", "{n} new messages"),
    ("还有 {n} 封", "{n} more"),
    ("标题：{a}\n正文：{b}", "Subject: {a}\nBody: {b}"),
];

#[cfg(test)]
mod tests {
    use super::*;

    // ENGLISH 是进程级全局，测试合并成一个串行执行，避免并行互踩
    #[test]
    fn tr_and_tr_error_translate_by_level() {
        set_lang_from_pref("zh");
        assert_eq!(tr("收到新邮件"), "收到新邮件");
        assert_eq!(tr_error("获取目录失败: boom"), "获取目录失败: boom");

        set_lang_from_pref("en");
        // 整条命中
        assert_eq!(tr("收到新邮件"), "New mail");
        assert_eq!(tr_n("收到 {n} 封新邮件", 3), "3 new messages");
        assert_eq!(tr_error("SMTP 连接失败"), "SMTP connection failed");
        // 冒号前缀命中：动态部分保留
        assert_eq!(tr_error("获取目录失败: timed out"), "Failed to list folders: timed out");
        assert_eq!(
            tr_error("IMAP 登录失败（请检查用户名/密码或应用专用密码）: AUTH failed"),
            "IMAP sign-in failed (check username/password or app-specific password): AUTH failed"
        );
        // 起始短语命中（无冒号分隔的格式）
        assert_eq!(tr_error("无法连接 imap.example.com:993 — refused"), "Cannot connect to imap.example.com:993 — refused");
        // 未收录的保持原文（回退中文，不丢信息）
        assert_eq!(tr_error("某个没收录的错误: x"), "某个没收录的错误: x");

        set_lang_from_pref("zh");
    }
}
