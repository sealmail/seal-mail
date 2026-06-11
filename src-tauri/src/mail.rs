use crate::crypto;
use crate::models::*;
use chrono::{Datelike, Local, TimeZone};
use mail_parser::{MessageParser, MimeHeaders};

fn domain_of(addr: &str) -> String {
    addr.rsplit('@').next().unwrap_or("").to_lowercase()
}

fn header_text(msg: &mail_parser::Message, name: &str) -> Option<String> {
    msg.header(name).and_then(|h| h.as_text()).map(|s| s.to_string())
}

pub fn detect_lang(text: &str) -> String {
    let mut has_kana = false;
    let mut has_cjk = false;
    let mut has_hangul = false;
    for c in text.chars().take(600) {
        let u = c as u32;
        if (0x3040..=0x30FF).contains(&u) {
            has_kana = true;
        } else if (0x4E00..=0x9FFF).contains(&u) {
            has_cjk = true;
        } else if (0xAC00..=0xD7AF).contains(&u) {
            has_hangul = true;
        }
    }
    if has_kana {
        "JA".into()
    } else if has_hangul {
        "KO".into()
    } else if has_cjk {
        "ZH".into()
    } else {
        "EN".into()
    }
}

const FUND_KW: &[&str] = &[
    "usdc", "usdt", "transfer", "wire", "settlement", "payment", "invoice", "remittance",
    "转账", "汇款", "付款", "收款", "打款", "结算",
];
const ACCOUNT_KW: &[&str] = &[
    "seed phrase", "recovery phrase", "private key", "verify your account", "reset your password",
    "助记词", "私钥", "验证码", "重置密码", "恢复短语",
];
const CONTRACT_KW: &[&str] = &[
    "contract", "agreement", "clause", "indemnity", "counter-sign", "msa",
    "合同", "条款", "协议", "签署", "盖章",
];
const URGENT_KW: &[&str] = &[
    "urgent", "immediately", "asap", "end of day", "within 2 hours", "right away", "short notice",
    "紧急", "尽快", "立即", "限时", "今天之内", "马上",
];

pub fn detect_risk(subject: &str, body: &str) -> Option<RiskInfo> {
    let text = format!("{}\n{}", subject, body).to_lowercase();
    let hit = |kws: &[&str]| -> Vec<String> {
        kws.iter().filter(|k| text.contains(&***k)).map(|k| k.to_string()).collect()
    };
    let urgent = hit(URGENT_KW);
    let account = hit(ACCOUNT_KW);
    if !account.is_empty() {
        let mut reasons = vec!["邮件要求提供凭据 / 密钥类敏感信息".to_string()];
        reasons.extend(account.iter().map(|k| format!("命中关键词：{}", k)));
        return Some(RiskInfo { kind: "account".into(), reasons });
    }
    let fund = hit(FUND_KW);
    if !fund.is_empty() && !urgent.is_empty() {
        let mut reasons = vec!["涉及资金操作且带有紧急措辞".to_string()];
        reasons.extend(fund.iter().take(3).map(|k| format!("资金相关：{}", k)));
        reasons.extend(urgent.iter().take(2).map(|k| format!("施压话术：{}", k)));
        return Some(RiskInfo { kind: "fund".into(), reasons });
    }
    let contract = hit(CONTRACT_KW);
    if !contract.is_empty() && !urgent.is_empty() {
        let mut reasons = vec!["涉及合同条款且带有时限要求".to_string()];
        reasons.extend(contract.iter().take(3).map(|k| format!("合同相关：{}", k)));
        return Some(RiskInfo { kind: "contract".into(), reasons });
    }
    None
}

pub fn verify_message(
    msg: &mail_parser::Message,
    body_text: &str,
    from_name: &str,
    from_addr: &str,
    trusted: &[TrustedContact],
) -> VerifyDetail {
    let by_addr = trusted.iter().find(|t| t.email.eq_ignore_ascii_case(from_addr));
    let by_name = trusted
        .iter()
        .find(|t| !t.name.is_empty() && t.name.eq_ignore_ascii_case(from_name));

    let sig = header_text(msg, crypto::H_SIGNATURE);
    let method = header_text(msg, crypto::H_METHOD).unwrap_or_else(|| "ed25519".into());
    let id_source = if method == "eth-personal" {
        header_text(msg, crypto::H_ADDRESS)
    } else {
        header_text(msg, crypto::H_PUBKEY)
    };

    let (sig, id_source) = match (sig, id_source) {
        (Some(s), Some(p)) => (s, p),
        _ => {
            // 未签名：如果显示名与某个可信联系人一致但地址不同 → 疑似冒充
            if let Some(t) = by_name {
                if !t.email.eq_ignore_ascii_case(from_addr) {
                    return VerifyDetail::Impersonation {
                        claimed: t.name.clone(),
                        got_fingerprint: None,
                        real_fingerprint: t.fingerprint.clone(),
                        got_domain: domain_of(from_addr),
                        real_domain: domain_of(&t.email),
                    };
                }
            }
            return VerifyDetail::Unsigned;
        }
    };

    let signed_from = header_text(msg, crypto::H_FROM).unwrap_or_else(|| from_addr.to_lowercase());
    let signed_date = header_text(msg, crypto::H_DATE).unwrap_or_default();
    let signed_body_hash = header_text(msg, crypto::H_BODY_HASH).unwrap_or_default();

    // 签名头声明的发件人必须与实际 From 一致，否则是头部重放/冒充
    if !signed_from.eq_ignore_ascii_case(from_addr) {
        return VerifyDetail::Impersonation {
            claimed: signed_from.clone(),
            got_fingerprint: None,
            real_fingerprint: by_name.map(|t| t.fingerprint.clone()).unwrap_or_default(),
            got_domain: domain_of(from_addr),
            real_domain: domain_of(&signed_from),
        };
    }

    // 按签名方案校验，统一产出 (指纹/地址, 正文是否一致, 签名时哈希, 收到时哈希)
    let (method, outcome): (String, Result<(String, bool, String, String), String>) =
        if method == "eth-personal" {
            let res = (|| {
                let sig_bytes = hex::decode(sig.trim()).map_err(|_| "签名格式错误".to_string())?;
                let rsv: [u8; 65] = sig_bytes.try_into().map_err(|_| "签名长度错误".to_string())?;
                let canon = crypto::canon_string(&signed_from, &signed_date, &signed_body_hash);
                let recovered = crypto::eth_personal_recover(canon.as_bytes(), &rsv)?;
                if !recovered.eq_ignore_ascii_case(&id_source) {
                    return Err("签名与声明地址不符".to_string());
                }
                let got = crypto::body_hash_hex(body_text);
                let ok = got == signed_body_hash.trim().to_lowercase();
                Ok((recovered, ok, signed_body_hash.clone(), got))
            })();
            ("Ledger · secp256k1".to_string(), res)
        } else {
            let h = crypto::SealHeaders {
                pubkey: id_source.clone(),
                signature: sig.clone(),
                from: signed_from.clone(),
                date: signed_date.clone(),
                body_hash: signed_body_hash.clone(),
            };
            ("SealMail · Ed25519".to_string(), crypto::verify_headers(&h, body_text))
        };

    match outcome {
        Ok((fingerprint, body_ok, signed_hash, got_hash)) => {
            if !body_ok {
                return VerifyDetail::Tampered {
                    signed_hash: format!("{}…", &signed_hash[..12.min(signed_hash.len())]),
                    got_hash: format!("{}…", &got_hash[..12.min(got_hash.len())]),
                    fingerprint,
                    method,
                };
            }
            if let Some(t) = by_addr {
                if t.fingerprint.eq_ignore_ascii_case(&fingerprint) {
                    return VerifyDetail::Verified {
                        fingerprint,
                        method,
                        contact_name: t.name.clone(),
                        since: t.since.clone(),
                        verified_count: t.verified_count,
                    };
                }
                // 地址匹配可信联系人但密钥不符 → 冒充/密钥被换
                return VerifyDetail::Impersonation {
                    claimed: t.name.clone(),
                    got_fingerprint: Some(fingerprint),
                    real_fingerprint: t.fingerprint.clone(),
                    got_domain: domain_of(from_addr),
                    real_domain: domain_of(&t.email),
                };
            }
            if let Some(t) = by_name {
                // 显示名匹配可信联系人、地址不同、密钥不同 → 冒充
                if !t.fingerprint.eq_ignore_ascii_case(&fingerprint) {
                    return VerifyDetail::Impersonation {
                        claimed: t.name.clone(),
                        got_fingerprint: Some(fingerprint),
                        real_fingerprint: t.fingerprint.clone(),
                        got_domain: domain_of(from_addr),
                        real_domain: domain_of(&t.email),
                    };
                }
            }
            VerifyDetail::SignedUnknown { fingerprint, method }
        }
        Err(_) => VerifyDetail::Tampered {
            signed_hash: "签名校验失败".into(),
            got_hash: crypto::body_hash_hex(body_text)[..12].to_string() + "…",
            fingerprint: "—".into(),
            method,
        },
    }
}

pub fn format_date(ts: i64) -> String {
    if ts <= 0 {
        return "—".into();
    }
    let dt = match Local.timestamp_opt(ts, 0) {
        chrono::LocalResult::Single(d) => d,
        _ => return "—".into(),
    };
    let now = Local::now();
    if dt.date_naive() == now.date_naive() {
        dt.format("%H:%M").to_string()
    } else if dt.year() == now.year() {
        format!("{}月{}日", dt.month(), dt.day())
    } else {
        dt.format("%Y/%m/%d").to_string()
    }
}

/// 从原始邮件中取出第 index 个附件（名字, 内容）
pub fn extract_attachment(raw: &[u8], index: usize) -> Result<(String, Vec<u8>), String> {
    let msg = MessageParser::default().parse(raw).ok_or("无法解析邮件内容")?;
    let part = msg
        .attachments()
        .nth(index)
        .ok_or("附件不存在（邮件可能已变化，请刷新后重试）")?;
    Ok((
        part.attachment_name().unwrap_or("attachment").to_string(),
        part.contents().to_vec(),
    ))
}

/// 解析原始邮件 → EmailFull（含验证、风险、语言识别）
pub fn parse_email(
    raw: &[u8],
    uid: u32,
    account_id: &str,
    folder: &str,
    unread: bool,
    trusted: &[TrustedContact],
) -> Result<EmailFull, String> {
    let msg = MessageParser::default()
        .parse(raw)
        .ok_or("无法解析邮件内容")?;

    let (from_name, from_addr) = msg
        .from()
        .and_then(|a| a.first())
        .map(|a| {
            (
                a.name().unwrap_or_default().to_string(),
                a.address().unwrap_or_default().to_string(),
            )
        })
        .unwrap_or_default();
    let from_name = if from_name.is_empty() { from_addr.clone() } else { from_name };

    let to: Vec<String> = msg
        .to()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.address().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let cc: Vec<String> = msg
        .cc()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.address().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let subject = msg.subject().unwrap_or("(无主题)").to_string();
    let timestamp = msg.date().map(|d| d.to_timestamp()).unwrap_or(0);
    let body_text = msg
        .body_text(0)
        .map(|c| c.to_string())
        .unwrap_or_default();
    let body_html = msg.body_html(0).map(|c| c.to_string());

    let attachments: Vec<AttachmentMeta> = msg
        .attachments()
        .map(|p| AttachmentMeta {
            name: p.attachment_name().unwrap_or("attachment").to_string(),
            size: p.contents().len(),
            mime: p
                .content_type()
                .map(|c| {
                    format!(
                        "{}/{}",
                        c.ctype(),
                        c.subtype().unwrap_or("octet-stream")
                    )
                })
                .unwrap_or_else(|| "application/octet-stream".into()),
        })
        .collect();

    let verify = verify_message(&msg, &body_text, &from_name, &from_addr, trusted);
    let risk = detect_risk(&subject, &body_text);
    let preview: String = body_text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .chars()
        .take(120)
        .collect();

    let meta = EmailMeta {
        uid,
        account_id: account_id.to_string(),
        folder: folder.to_string(),
        from_name,
        from_addr,
        subject: subject.clone(),
        preview,
        date_display: format_date(timestamp),
        timestamp,
        unread,
        lang: detect_lang(&body_text),
        trust: verify.trust_tag().to_string(),
        risk,
        has_attach: !attachments.is_empty(),
    };

    Ok(EmailFull {
        meta,
        to,
        cc,
        body_text,
        body_html,
        attachments,
        verify,
    })
}
