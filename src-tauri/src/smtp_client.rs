use crate::crypto::{self, Identity};
use crate::models::*;
use lettre::address::Envelope;
use lettre::transport::smtp::authentication::{Credentials, Mechanism};
use lettre::{SmtpTransport, Transport};
use mail_builder::headers::raw::Raw;
use mail_builder::MessageBuilder;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendResult {
    pub signed: bool,
    pub method: String,
    pub fingerprint: String,
    pub short_fingerprint: String,
    pub sent_at: String,
}

/// 发送时使用的签名方式
pub enum Signer<'a> {
    None,
    Local(&'a Identity),
    Ledger { path: String, address: String },
}

fn short_addr(addr: &str) -> String {
    if addr.len() > 12 {
        format!("{}…{}", &addr[..6], &addr[addr.len() - 4..])
    } else {
        addr.to_string()
    }
}

fn transport(account: &Account, secret: &AccountSecret) -> Result<SmtpTransport, String> {
    let builder = if account.smtp_security == "starttls" {
        SmtpTransport::starttls_relay(&account.smtp_host)
    } else {
        SmtpTransport::relay(&account.smtp_host)
    }
    .map_err(|e| format!("SMTP 配置错误: {}", e))?;
    let builder = builder.port(account.smtp_port);
    if let Some(oauth) = &secret.oauth {
        // XOAUTH2：密码位传 access_token
        return Ok(builder
            .credentials(Credentials::new(
                account.username.clone(),
                oauth.access_token.clone(),
            ))
            .authentication(vec![Mechanism::Xoauth2])
            .build());
    }
    let password = secret
        .smtp_password
        .clone()
        .unwrap_or_else(|| secret.password.clone());
    Ok(builder
        .credentials(Credentials::new(account.username.clone(), password))
        .build())
}

/// 按扩展名猜常见 MIME 类型（附件用；未知类型回退 octet-stream）
fn guess_mime(name: &str) -> &'static str {
    match name
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "txt" | "log" | "md" => "text/plain",
        "html" | "htm" => "text/html",
        "csv" => "text/csv",
        "json" => "application/json",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    }
}

#[allow(clippy::too_many_arguments)]
pub fn send_mail(
    account: &Account,
    secret: &AccountSecret,
    signer: Signer<'_>,
    to: Vec<String>,
    cc: Vec<String>,
    subject: &str,
    body: &str,
    attachments: Vec<(String, Vec<u8>)>,
) -> Result<SendResult, String> {
    let (signed, method, fingerprint, short) = match &signer {
        Signer::None => (false, "无".to_string(), String::new(), String::new()),
        Signer::Local(id) => {
            let f = id.fingerprint();
            let s = crypto::short_fpr(&f);
            (true, "SealMail · Ed25519".to_string(), f, s)
        }
        Signer::Ledger { address, .. } => {
            let s = short_addr(address);
            (true, "Ledger · secp256k1".to_string(), address.clone(), s)
        }
    };

    // 签名证明只放在 X-SealMail-* 邮件头里；正文必须保持用户输入原样。
    let final_body = body.to_string();

    let mut builder = MessageBuilder::new()
        .from((account.display_name.as_str(), account.email.as_str()))
        .to(to
            .iter()
            .map(|a| ("", a.as_str()))
            .collect::<Vec<(&str, &str)>>())
        .subject(subject)
        .text_body(final_body.as_str());
    if !cc.is_empty() {
        builder = builder.cc(cc
            .iter()
            .map(|a| ("", a.as_str()))
            .collect::<Vec<(&str, &str)>>());
    }
    // 附件（注意：签名 canon 只覆盖纯文本正文，附件不在签名范围内）
    for (name, data) in &attachments {
        builder = builder.attachment(guess_mime(name), name.as_str(), &data[..]);
    }

    match &signer {
        Signer::None => {}
        Signer::Local(id) => {
            for (name, value) in crypto::sign_email(id, &account.email, &final_body).headers {
                builder = builder.header(name, Raw::new(value));
            }
        }
        Signer::Ledger { path, address } => {
            // 这里会阻塞等待用户在 Ledger 设备上确认
            let headers = crypto::sign_email_eth(address, &account.email, &final_body, |msg| {
                crate::ledger::sign_personal_message(path, msg)
            })?;
            for (name, value) in headers.headers {
                builder = builder.header(name, Raw::new(value));
            }
        }
    }

    let raw = builder
        .write_to_vec()
        .map_err(|e| format!("构建邮件失败: {}", e))?;

    let from_addr = account
        .email
        .parse()
        .map_err(|_| "发件地址格式错误".to_string())?;
    let mut rcpt = Vec::new();
    for a in to.iter().chain(cc.iter()) {
        rcpt.push(
            a.trim()
                .parse()
                .map_err(|_| format!("收件地址格式错误: {}", a))?,
        );
    }
    let envelope = Envelope::new(Some(from_addr), rcpt).map_err(|e| e.to_string())?;

    let mailer = transport(account, secret)?;
    mailer
        .send_raw(&envelope, &raw)
        .map_err(|e| format!("发送失败: {}", e))?;

    Ok(SendResult {
        signed,
        method,
        fingerprint,
        short_fingerprint: short,
        sent_at: chrono::Local::now().format("%H:%M").to_string(),
    })
}

pub fn test_smtp(account: &Account, secret: &AccountSecret) -> Result<(), String> {
    let mailer = transport(account, secret)?;
    mailer
        .test_connection()
        .map_err(|e| format!("SMTP 连接失败: {}", e))
        .and_then(|ok| {
            if ok {
                Ok(())
            } else {
                Err("SMTP 连接失败".into())
            }
        })
}
