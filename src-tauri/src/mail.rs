use crate::crypto;
use crate::models::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{Datelike, Local, TimeZone};
use encoding_rs::{Encoding, GB18030, UTF_8};
use mail_parser::{MessageParser, MimeHeaders};

fn domain_of(addr: &str) -> String {
    addr.rsplit('@').next().unwrap_or("").to_lowercase()
}

fn header_text(msg: &mail_parser::Message, name: &str) -> Option<String> {
    msg.header(name)
        .and_then(|h| h.as_text())
        .map(|s| s.to_string())
}

fn header_raw_text(msg: &mail_parser::Message, name: &'static str) -> Option<String> {
    msg.header_raw(name).map(|s| s.to_string())
}

fn replacement_count(s: &str) -> usize {
    s.chars().filter(|&c| c == '\u{FFFD}').count()
}

fn contains_cjk(s: &str) -> bool {
    s.chars().any(|c| {
        let u = c as u32;
        (0x4E00..=0x9FFF).contains(&u)
    })
}

fn looks_mojibake(s: &str) -> bool {
    let replacements = replacement_count(s);
    replacements >= 2 || (replacements > 0 && s.chars().count() <= 24)
}

fn has_encoded_word(s: &str) -> bool {
    s.contains("=?") && s.contains("?=")
}

fn prefer_decoded(current: String, candidate: String) -> String {
    let candidate = candidate.trim_matches('\0').trim().to_string();
    if candidate.is_empty() {
        return current;
    }
    let current_bad = replacement_count(&current);
    let candidate_bad = replacement_count(&candidate);
    if candidate_bad < current_bad || (looks_mojibake(&current) && contains_cjk(&candidate)) {
        candidate
    } else {
        current
    }
}

fn prefer_body(current: String, candidate: String) -> String {
    if candidate.trim().is_empty() {
        return current;
    }
    let current_bad = replacement_count(&current);
    let candidate_bad = replacement_count(&candidate);
    if current.trim().is_empty()
        || candidate_bad < current_bad
        || (looks_mojibake(&current) && contains_cjk(&candidate))
    {
        candidate
    } else {
        current
    }
}

fn split_message(raw: &[u8]) -> Option<(&[u8], &[u8])> {
    if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
        Some((&raw[..pos], &raw[pos + 4..]))
    } else {
        raw.windows(2)
            .position(|w| w == b"\n\n")
            .map(|pos| (&raw[..pos], &raw[pos + 2..]))
    }
}

fn extract_header_bytes(headers: &[u8], name: &str) -> Option<Vec<u8>> {
    let wanted = name.to_ascii_lowercase();
    let mut found = false;
    let mut out = Vec::new();
    for raw_line in headers.split(|&b| b == b'\n') {
        let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
        if line.first().is_some_and(|b| *b == b' ' || *b == b'\t') {
            if found {
                out.push(b' ');
                out.extend_from_slice(line.trim_ascii());
            }
            continue;
        }
        if found {
            break;
        }
        let Some(colon) = line.iter().position(|&b| b == b':') else {
            continue;
        };
        let header_name = String::from_utf8_lossy(&line[..colon]).to_ascii_lowercase();
        if header_name == wanted {
            found = true;
            out.extend_from_slice(line[colon + 1..].trim_ascii());
        }
    }
    found.then_some(out)
}

fn decode_bytes_best(bytes: &[u8], charset: Option<&str>) -> String {
    let mut tried = Vec::new();
    if let Some(label) = charset {
        if let Some(enc) = Encoding::for_label(label.trim_matches('"').trim().as_bytes()) {
            tried.push(enc);
        }
    }
    tried.push(UTF_8);
    tried.push(GB18030);

    let mut best = String::new();
    let mut best_bad = usize::MAX;
    for enc in tried {
        let (decoded, _, had_errors) = enc.decode(bytes);
        let decoded = decoded.into_owned();
        let bad = replacement_count(&decoded) + usize::from(had_errors);
        if best.is_empty()
            || bad < best_bad
            || (bad == best_bad && contains_cjk(&decoded) && !contains_cjk(&best))
        {
            best = decoded;
            best_bad = bad;
        }
        if bad == 0 && (enc == UTF_8 || contains_cjk(&best)) {
            break;
        }
    }
    best
}

fn decode_header_q(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'_' {
            out.push(b' ');
            i += 1;
        } else if bytes[i] == b'=' && i + 2 < bytes.len() {
            if let Ok(s) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(v) = u8::from_str_radix(s, 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

fn decode_rfc2047_words(value: &[u8]) -> Option<String> {
    let input = String::from_utf8_lossy(value);
    if !has_encoded_word(&input) {
        return None;
    }

    let mut out = String::new();
    let mut rest = input.as_ref();
    let mut decoded_any = false;
    while let Some(start) = rest.find("=?") {
        let before = &rest[..start];
        out.push_str(before);
        let word = &rest[start + 2..];
        let Some(cs_end) = word.find('?') else {
            out.push_str(&rest[start..]);
            return decoded_any.then_some(out.trim().to_string());
        };
        let charset = &word[..cs_end];
        let after_charset = &word[cs_end + 1..];
        let Some(enc_end) = after_charset.find('?') else {
            out.push_str(&rest[start..]);
            return decoded_any.then_some(out.trim().to_string());
        };
        let encoding = &after_charset[..enc_end];
        let after_encoding = &after_charset[enc_end + 1..];
        let Some(text_end) = after_encoding.find("?=") else {
            out.push_str(&rest[start..]);
            return decoded_any.then_some(out.trim().to_string());
        };
        let encoded = &after_encoding[..text_end];
        let decoded_bytes = if encoding.eq_ignore_ascii_case("B") {
            STANDARD.decode(encoded.as_bytes()).ok()?
        } else if encoding.eq_ignore_ascii_case("Q") {
            decode_header_q(encoded.as_bytes())
        } else {
            out.push_str(&rest[start..]);
            return decoded_any.then_some(out.trim().to_string());
        };
        out.push_str(&decode_bytes_best(&decoded_bytes, Some(charset)));
        decoded_any = true;

        rest = &after_encoding[text_end + 2..];
        if rest.trim_start().strip_prefix("=?").is_some()
            && rest[..rest.len() - rest.trim_start().len()]
                .chars()
                .all(char::is_whitespace)
        {
            rest = rest.trim_start();
        }
    }
    out.push_str(rest);
    decoded_any.then_some(out.trim().to_string())
}

fn decoded_raw_header(raw: &[u8], name: &str) -> Option<String> {
    let (headers, _) = split_message(raw)?;
    let value = extract_header_bytes(headers, name)?;
    decode_rfc2047_words(&value).or_else(|| Some(decode_bytes_best(&value, None)))
}

fn decoded_from_name(raw: &[u8], current: String) -> String {
    if !looks_mojibake(&current) && !has_encoded_word(&current) {
        return current;
    }
    let Some(from) = decoded_raw_header(raw, "From") else {
        return current;
    };
    let name = from
        .split('<')
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches('"')
        .trim()
        .to_string();
    if name.is_empty() || name.contains('@') {
        current
    } else {
        prefer_decoded(current, name)
    }
}

fn header_value_ascii(headers: &[u8], name: &str) -> Option<String> {
    extract_header_bytes(headers, name).map(|v| String::from_utf8_lossy(&v).into_owned())
}

fn header_param(value: &str, param: &str) -> Option<String> {
    let wanted = param.to_ascii_lowercase();
    value.split(';').skip(1).find_map(|part| {
        let (key, val) = part.split_once('=')?;
        (key.trim().eq_ignore_ascii_case(&wanted))
            .then(|| val.trim().trim_matches('"').trim_matches('\'').to_string())
    })
}

fn decode_quoted_printable(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' {
            if bytes.get(i + 1) == Some(&b'\r') && bytes.get(i + 2) == Some(&b'\n') {
                i += 3;
                continue;
            }
            if bytes.get(i + 1) == Some(&b'\n') {
                i += 2;
                continue;
            }
            if i + 2 < bytes.len() {
                let hex = &bytes[i + 1..i + 3];
                if let Ok(s) = std::str::from_utf8(hex) {
                    if let Ok(v) = u8::from_str_radix(s, 16) {
                        out.push(v);
                        i += 3;
                        continue;
                    }
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn decode_transfer(body: &[u8], encoding: Option<&str>) -> Vec<u8> {
    match encoding.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "base64" => {
            let compact: Vec<u8> = body
                .iter()
                .copied()
                .filter(|b| !b.is_ascii_whitespace())
                .collect();
            STANDARD.decode(compact).unwrap_or_else(|_| body.to_vec())
        }
        "quoted-printable" => decode_quoted_printable(body),
        _ => body.to_vec(),
    }
}

fn body_from_part(headers: &[u8], body: &[u8]) -> String {
    let content_type = header_value_ascii(headers, "Content-Type").unwrap_or_default();
    let charset = header_param(&content_type, "charset");
    let transfer = header_value_ascii(headers, "Content-Transfer-Encoding");
    let decoded = decode_transfer(body, transfer.as_deref());
    decode_bytes_best(&decoded, charset.as_deref())
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn find_body_part(headers: &[u8], body: &[u8], want_html: bool) -> Option<String> {
    let content_type = header_value_ascii(headers, "Content-Type").unwrap_or_default();
    let content_type_lower = content_type.to_ascii_lowercase();
    if let Some(boundary) = header_param(&content_type, "boundary") {
        let marker = format!("--{}", boundary);
        let marker = marker.as_bytes();
        let mut cursor = 0;
        while let Some(pos) = find_subslice(&body[cursor..], marker) {
            let start = cursor + pos + marker.len();
            let mut after = &body[start..];
            if after.starts_with(b"--") {
                break;
            }
            if after.starts_with(b"\r\n") {
                after = &after[2..];
            } else if after.starts_with(b"\n") {
                after = &after[1..];
            }
            let next = find_subslice(after, marker).unwrap_or(after.len());
            let part = &after[..next];
            cursor = start + next;

            let Some((part_headers, part_body)) = split_message(part) else {
                continue;
            };
            if let Some(found) = find_body_part(part_headers, part_body, want_html) {
                return Some(found);
            }
        }
        None
    } else {
        let is_html = content_type_lower.contains("text/html");
        let is_text = content_type_lower.is_empty() || content_type_lower.contains("text/plain");
        if (want_html && is_html) || (!want_html && is_text) {
            return Some(body_from_part(headers, body));
        }
        None
    }
}

fn fallback_body(raw: &[u8], want_html: bool) -> Option<String> {
    let (headers, body) = split_message(raw)?;
    if header_value_ascii(headers, "Content-Type")
        .unwrap_or_default()
        .to_ascii_lowercase()
        .starts_with("multipart/")
    {
        find_body_part(headers, body, want_html)
    } else {
        Some(body_from_part(headers, body))
    }
}

fn normalize_msg_id(s: &str) -> Option<String> {
    let trimmed = s.trim().trim_matches('<').trim_matches('>').trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_lowercase())
    }
}

fn extract_msg_ids(s: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = s;
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('>') else { break };
        if let Some(id) = normalize_msg_id(&after[..end]) {
            ids.push(id);
        }
        rest = &after[end + 1..];
    }
    if ids.is_empty() {
        ids.extend(s.split_whitespace().filter_map(normalize_msg_id));
    }
    ids
}

fn base_subject(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        let lower = s.to_lowercase();
        let prefixes = ["re:", "fw:", "fwd:", "答复:", "回复:", "转发:"];
        let Some(prefix) = prefixes.iter().find(|p| lower.starts_with(**p)) else {
            break;
        };
        s = s[prefix.len()..].trim();
    }
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub fn thread_id_for(msg: &mail_parser::Message, subject: &str) -> (Option<String>, String) {
    let message_id = header_raw_text(msg, "Message-ID").and_then(|s| normalize_msg_id(&s));
    let reference_root = header_raw_text(msg, "References")
        .and_then(|s| extract_msg_ids(&s).into_iter().next())
        .or_else(|| {
            header_raw_text(msg, "In-Reply-To").and_then(|s| extract_msg_ids(&s).into_iter().next())
        });
    let thread_id = reference_root
        .or_else(|| message_id.clone())
        .unwrap_or_else(|| format!("subject:{}", base_subject(subject)));
    (message_id, thread_id)
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
    "usdc",
    "usdt",
    "transfer",
    "wire",
    "settlement",
    "payment",
    "invoice",
    "remittance",
    "转账",
    "汇款",
    "付款",
    "收款",
    "打款",
    "结算",
];
const ACCOUNT_KW: &[&str] = &[
    "seed phrase",
    "recovery phrase",
    "private key",
    "verify your account",
    "reset your password",
    "助记词",
    "私钥",
    "验证码",
    "重置密码",
    "恢复短语",
];
const CONTRACT_KW: &[&str] = &[
    "contract",
    "agreement",
    "clause",
    "indemnity",
    "counter-sign",
    "msa",
    "合同",
    "条款",
    "协议",
    "签署",
    "盖章",
];
const URGENT_KW: &[&str] = &[
    "urgent",
    "immediately",
    "asap",
    "end of day",
    "within 2 hours",
    "right away",
    "short notice",
    "紧急",
    "尽快",
    "立即",
    "限时",
    "今天之内",
    "马上",
];

pub fn detect_risk(subject: &str, body: &str) -> Option<RiskInfo> {
    let text = format!("{}\n{}", subject, body).to_lowercase();
    let hit = |kws: &[&str]| -> Vec<String> {
        kws.iter()
            .filter(|k| text.contains(&***k))
            .map(|k| k.to_string())
            .collect()
    };
    let urgent = hit(URGENT_KW);
    let account = hit(ACCOUNT_KW);
    if !account.is_empty() {
        let mut reasons = vec!["邮件要求提供凭据 / 密钥类敏感信息".to_string()];
        reasons.extend(account.iter().map(|k| format!("命中关键词：{}", k)));
        return Some(RiskInfo {
            kind: "account".into(),
            reasons,
        });
    }
    let fund = hit(FUND_KW);
    if !fund.is_empty() && !urgent.is_empty() {
        let mut reasons = vec!["涉及资金操作且带有紧急措辞".to_string()];
        reasons.extend(fund.iter().take(3).map(|k| format!("资金相关：{}", k)));
        reasons.extend(urgent.iter().take(2).map(|k| format!("施压话术：{}", k)));
        return Some(RiskInfo {
            kind: "fund".into(),
            reasons,
        });
    }
    let contract = hit(CONTRACT_KW);
    if !contract.is_empty() && !urgent.is_empty() {
        let mut reasons = vec!["涉及合同条款且带有时限要求".to_string()];
        reasons.extend(contract.iter().take(3).map(|k| format!("合同相关：{}", k)));
        return Some(RiskInfo {
            kind: "contract".into(),
            reasons,
        });
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
    let by_addr = trusted
        .iter()
        .find(|t| t.email.eq_ignore_ascii_case(from_addr));
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
                let rsv: [u8; 65] = sig_bytes
                    .try_into()
                    .map_err(|_| "签名长度错误".to_string())?;
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
            (
                "SealMail · Ed25519".to_string(),
                crypto::verify_headers(&h, body_text),
            )
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
            VerifyDetail::SignedUnknown {
                fingerprint,
                method,
            }
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
        if crate::i18n::is_english() {
            dt.format("%b %-d").to_string()
        } else {
            format!("{}月{}日", dt.month(), dt.day())
        }
    } else {
        dt.format("%Y/%m/%d").to_string()
    }
}

/// 从原始邮件中取出第 index 个附件
pub struct ExtractedAttachment {
    pub filename: String,
    pub mime: String,
    pub contents: Vec<u8>,
}

/// 从原始邮件中取出第 index 个附件（名字, MIME, 内容）
pub fn extract_attachment(raw: &[u8], index: usize) -> Result<ExtractedAttachment, String> {
    let msg = MessageParser::default()
        .parse(raw)
        .ok_or("无法解析邮件内容")?;
    let part = msg
        .attachments()
        .nth(index)
        .ok_or("附件不存在（邮件可能已变化，请刷新后重试）")?;
    let mime = part
        .content_type()
        .map(|c| format!("{}/{}", c.ctype(), c.subtype().unwrap_or("octet-stream")))
        .unwrap_or_else(|| "application/octet-stream".into());
    Ok(ExtractedAttachment {
        filename: part.attachment_name().unwrap_or("attachment").to_string(),
        mime,
        contents: part.contents().to_vec(),
    })
}

/// 解析原始邮件 → EmailFull（含验证、风险、语言识别）
pub fn parse_email(
    raw: &[u8],
    uid: u32,
    account_id: &str,
    folder: &str,
    unread: bool,
    flagged: bool,
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
    let from_name = if from_name.is_empty() {
        from_addr.clone()
    } else {
        from_name
    };
    let from_name = decoded_from_name(raw, from_name);

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
    let subject = if looks_mojibake(&subject) || has_encoded_word(&subject) {
        decoded_raw_header(raw, "Subject").map_or(subject.clone(), |s| prefer_decoded(subject, s))
    } else {
        subject
    };
    let (message_id, thread_id) = thread_id_for(&msg, &subject);
    let timestamp = msg.date().map(|d| d.to_timestamp()).unwrap_or(0);
    let mut body_text = msg.body_text(0).map(|c| c.to_string()).unwrap_or_default();
    if looks_mojibake(&body_text) || body_text.trim().is_empty() {
        if let Some(fallback) = fallback_body(raw, false) {
            body_text = prefer_body(body_text, fallback);
        }
    }
    let mut body_html = msg.body_html(0).map(|c| c.to_string());
    if body_html.as_deref().is_some_and(looks_mojibake) {
        if let Some(fallback) = fallback_body(raw, true) {
            body_html = body_html.map(|current| prefer_body(current, fallback));
        }
    }

    let attachments: Vec<AttachmentMeta> = msg
        .attachments()
        .map(|p| AttachmentMeta {
            name: p.attachment_name().unwrap_or("attachment").to_string(),
            size: p.contents().len(),
            mime: p
                .content_type()
                .map(|c| format!("{}/{}", c.ctype(), c.subtype().unwrap_or("octet-stream")))
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
        message_id,
        thread_id,
        from_name,
        from_addr,
        subject: subject.clone(),
        preview,
        date_display: format_date(timestamp),
        timestamp,
        unread,
        flagged,
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
