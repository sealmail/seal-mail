use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum IncomingProtocol {
    Imap,
    Pop3,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub label: String,
    pub email: String,
    pub display_name: String,
    pub protocol: IncomingProtocol,
    pub incoming_host: String,
    pub incoming_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    /// "ssl" (implicit TLS) or "starttls" — applies to SMTP; incoming is implicit TLS
    pub smtp_security: String,
    pub username: String,
    /// "password" | "oauth2"（Exchange Online / Outlook.com 已强制 OAuth2）
    #[serde(default = "default_auth")]
    pub auth: String,
}

fn default_auth() -> String {
    "password".into()
}

/// Secrets are kept out of accounts.json, in a 0600 file in the app config dir.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSecret {
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub smtp_password: Option<String>,
    /// OAuth2 令牌（auth = "oauth2" 时使用；access_token 过期会自动刷新回写）
    #[serde(default)]
    pub oauth: Option<crate::oauth::OAuthTokens>,
}

/// 草稿（drafts.json，本地）。写信时自动保存，发送成功后删除。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub id: String,
    pub account_id: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub body: String,
    pub sign: bool,
    /// unix 秒
    pub updated_at: i64,
}

/// 自动从收发记录里收集的联系人（写信时自动补全用；contacts.json）
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contact {
    pub name: String,
    pub email: String,
    /// 最近一次往来（unix 秒）
    pub last_seen: i64,
    /// 往来次数（排序权重）
    pub count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedContact {
    pub name: String,
    pub email: String,
    pub fingerprint: String,
    pub org: Option<String>,
    pub since: String,
    pub verified_count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterRule {
    pub id: String,
    pub name: String,
    /// None = applies to all accounts
    pub account_id: Option<String>,
    /// from | to | subject | body
    pub field: String,
    /// contains | not_contains | equals | starts_with | ends_with
    pub op: String,
    pub value: String,
    pub target_folder: String,
    #[serde(default)]
    pub mark_read: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiskInfo {
    /// fund | account | contract
    pub kind: String,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum VerifyDetail {
    /// Valid signature, fingerprint matches a trusted contact
    #[serde(rename_all = "camelCase")]
    Verified {
        fingerprint: String,
        method: String,
        contact_name: String,
        since: String,
        verified_count: u64,
    },
    /// Valid signature but the key is not (yet) in the trusted list
    #[serde(rename_all = "camelCase")]
    SignedUnknown { fingerprint: String, method: String },
    Unsigned,
    /// Signature present but body hash or signature check failed
    #[serde(rename_all = "camelCase")]
    Tampered {
        signed_hash: String,
        got_hash: String,
        fingerprint: String,
        method: String,
    },
    /// Sender claims the identity of a trusted contact but key/domain mismatch
    #[serde(rename_all = "camelCase")]
    Impersonation {
        claimed: String,
        got_fingerprint: Option<String>,
        real_fingerprint: String,
        got_domain: String,
        real_domain: String,
    },
}

impl VerifyDetail {
    pub fn trust_tag(&self) -> &'static str {
        match self {
            VerifyDetail::Verified { .. } => "verified",
            VerifyDetail::SignedUnknown { .. } => "signedUnknown",
            VerifyDetail::Unsigned => "unsigned",
            VerifyDetail::Tampered { .. } => "tampered",
            VerifyDetail::Impersonation { .. } => "impersonation",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentMeta {
    pub name: String,
    pub size: usize,
    pub mime: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailMeta {
    pub uid: u32,
    pub account_id: String,
    pub folder: String,
    pub from_name: String,
    pub from_addr: String,
    pub subject: String,
    pub preview: String,
    pub date_display: String,
    pub timestamp: i64,
    pub unread: bool,
    #[serde(default)]
    pub flagged: bool,
    pub lang: String,
    pub trust: String,
    pub risk: Option<RiskInfo>,
    pub has_attach: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailFull {
    pub meta: EmailMeta,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    pub body_text: String,
    pub body_html: Option<String>,
    pub attachments: Vec<AttachmentMeta>,
    pub verify: VerifyDetail,
}

/// 应用偏好（prefs.json）。close_behavior: "hide" | "quit"——
/// macOS 默认点关闭按钮隐藏窗口（从程序坞重新打开），其他平台默认退出。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppPrefs {
    pub close_behavior: String,
    /// 新邮件系统通知（窗口未聚焦时弹横幅）
    pub notify_new_mail: bool,
}

fn default_close_behavior() -> String {
    if cfg!(target_os = "macos") {
        "hide".into()
    } else {
        "quit".into()
    }
}

impl Default for AppPrefs {
    fn default() -> Self {
        AppPrefs {
            close_behavior: default_close_behavior(),
            notify_new_mail: true,
        }
    }
}

/// 签名身份配置（identity.json）：本地 Ed25519 密钥或 Ledger 硬件密钥
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityConfig {
    /// "local" | "ledger"
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub ledger_path: Option<String>,
    #[serde(default)]
    pub ledger_address: Option<String>,
}

fn default_mode() -> String {
    "local".into()
}

impl Default for IdentityConfig {
    fn default() -> Self {
        IdentityConfig { mode: default_mode(), ledger_path: None, ledger_address: None }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityInfo {
    pub fingerprint: String,
    pub public_key: String,
    pub created: String,
    pub mode: String,
    pub ledger_path: Option<String>,
    pub ledger_address: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderInfo {
    pub name: String,
    pub display: String,
    /// 特殊目录角色："trash" 等；普通目录为 None
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}
