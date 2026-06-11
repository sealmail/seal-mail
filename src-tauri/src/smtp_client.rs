use crate::crypto::{self, Identity};
use crate::models::*;
use lettre::address::Envelope;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{SmtpTransport, Transport};
use mail_builder::headers::raw::Raw;
use mail_builder::MessageBuilder;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendResult {
    pub signed: bool,
    pub fingerprint: String,
    pub short_fingerprint: String,
    pub sent_at: String,
}

fn transport(account: &Account, secret: &AccountSecret) -> Result<SmtpTransport, String> {
    let builder = if account.smtp_security == "starttls" {
        SmtpTransport::starttls_relay(&account.smtp_host)
    } else {
        SmtpTransport::relay(&account.smtp_host)
    }
    .map_err(|e| format!("SMTP 配置错误: {}", e))?;
    let password = secret
        .smtp_password
        .clone()
        .unwrap_or_else(|| secret.password.clone());
    Ok(builder
        .port(account.smtp_port)
        .credentials(Credentials::new(account.username.clone(), password))
        .build())
}

pub fn send_mail(
    account: &Account,
    secret: &AccountSecret,
    identity: &Identity,
    to: Vec<String>,
    cc: Vec<String>,
    subject: &str,
    body: &str,
    sign: bool,
) -> Result<SendResult, String> {
    let fingerprint = identity.fingerprint();
    let short = crypto::short_fpr(&fingerprint);

    // 签名时附加一行低调的签名说明：用标准 "-- " 分隔符，普通客户端会按签名档弱化显示
    let final_body = if sign {
        format!(
            "{}\n\n-- \n{} · 已用 SealMail 数字签名（指纹 {}）\n",
            body.trim_end(),
            account.display_name,
            short
        )
    } else {
        body.to_string()
    };

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

    if sign {
        let signed = crypto::sign_email(identity, &account.email, &final_body);
        for (name, value) in signed.headers {
            builder = builder.header(name, Raw::new(value));
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
        rcpt.push(a.trim().parse().map_err(|_| format!("收件地址格式错误: {}", a))?);
    }
    let envelope = Envelope::new(Some(from_addr), rcpt).map_err(|e| e.to_string())?;

    let mailer = transport(account, secret)?;
    mailer
        .send_raw(&envelope, &raw)
        .map_err(|e| format!("发送失败: {}", e))?;

    Ok(SendResult {
        signed: sign,
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
        .and_then(|ok| if ok { Ok(()) } else { Err("SMTP 连接失败".into()) })
}
