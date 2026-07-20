use base64::{engine::general_purpose::STANDARD as B64, Engine};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

pub const H_VERSION: &str = "X-SealMail-Version";
pub const H_PUBKEY: &str = "X-SealMail-PublicKey";
pub const H_SIGNATURE: &str = "X-SealMail-Signature";
pub const H_FROM: &str = "X-SealMail-From";
pub const H_DATE: &str = "X-SealMail-Date";
pub const H_BODY_HASH: &str = "X-SealMail-Body-Hash";
/// v2：主题 / HTML / 附件清单 / 收件人 的哈希头
pub const H_SUBJECT_HASH: &str = "X-SealMail-Subject-Hash";
pub const H_HTML_HASH: &str = "X-SealMail-Html-Hash";
pub const H_ATTACH_HASH: &str = "X-SealMail-Attach-Hash";
pub const H_TO_HASH: &str = "X-SealMail-To-Hash";
pub const H_MID_HASH: &str = "X-SealMail-MessageId-Hash";
/// 签名方案：ed25519（本地密钥）| eth-personal（Ledger secp256k1 EIP-191）
pub const H_METHOD: &str = "X-SealMail-Method";
/// eth-personal 方案下签名者的以太坊地址（验证时与 ecrecover 结果比对）
pub const H_ADDRESS: &str = "X-SealMail-Address";

/// 当前发送使用的 canon 版本（v3 = v2 + Message-ID）
pub const CANON_VERSION_CURRENT: &str = "3";
pub const CANON_VERSION_V1: &str = "1";
pub const CANON_VERSION_V2: &str = "2";
pub const EMPTY_HASH_TOKEN: &str = "-";
/// 超过此天数的签名不再给完整 Verified 绿标（防旧邮件重放当「当前指令」）
pub const MAX_VERIFIED_AGE_DAYS: i64 = 180;

pub struct Identity {
    pub signing_key: SigningKey,
    pub created: String,
}

impl Identity {
    pub fn fingerprint(&self) -> String {
        fingerprint_of(&self.signing_key.verifying_key())
    }
    pub fn public_key_b64(&self) -> String {
        B64.encode(self.signing_key.verifying_key().as_bytes())
    }
}

/// 形如 "9F2A 41C8 7B0E 5D19" 的指纹：公钥 SHA-256 前 16 字节，4 字节一组
pub fn fingerprint_of(vk: &VerifyingKey) -> String {
    let digest = Sha256::digest(vk.as_bytes());
    digest[..16]
        .chunks(2)
        .map(|c| format!("{:02X}{:02X}", c[0], c[1]))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn short_fpr(fpr: &str) -> String {
    let groups: Vec<&str> = fpr.split(' ').collect();
    if groups.len() >= 2 {
        format!("{}…{}", groups[0], groups[groups.len() - 1])
    } else {
        fpr.to_string()
    }
}

pub fn load_or_create_identity(dir: &Path) -> Result<Identity, String> {
    let key_path = dir.join("identity.key");
    if key_path.exists() {
        let data = fs::read_to_string(&key_path).map_err(|e| e.to_string())?;
        let mut lines = data.lines();
        let key_b64 = lines.next().ok_or("identity.key 为空")?;
        let created = lines.next().unwrap_or("").to_string();
        let bytes = B64.decode(key_b64.trim()).map_err(|e| e.to_string())?;
        let arr: [u8; 32] = bytes.try_into().map_err(|_| "identity.key 长度错误")?;
        return Ok(Identity {
            signing_key: SigningKey::from_bytes(&arr),
            created,
        });
    }
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(|e| e.to_string())?;
    let signing_key = SigningKey::from_bytes(&seed);
    let created = chrono::Utc::now().to_rfc3339();
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    fs::write(&key_path, format!("{}\n{}\n", B64.encode(seed), created))
        .map_err(|e| e.to_string())?;
    restrict_perms(&key_path);
    Ok(Identity {
        signing_key,
        created,
    })
}

pub fn restrict_perms(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
}

/// 统一的正文规范化：CRLF→LF，去除每行行尾空白与整体末尾空行
pub fn normalize_body(body: &str) -> String {
    let lf = body.replace("\r\n", "\n");
    let lines: Vec<&str> = lf.lines().map(|l| l.trim_end()).collect();
    lines.join("\n").trim_end().to_string()
}

pub fn body_hash_hex(body: &str) -> String {
    hex::encode(Sha256::digest(normalize_body(body).as_bytes()))
}

pub fn bytes_hash_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// 待签名/待验证的邮件内容（v3：主题、HTML、附件、收件人、Message-ID）。
pub struct SignContent<'a> {
    pub subject: &'a str,
    pub body_text: &'a str,
    pub body_html: Option<&'a str>,
    /// To + Cc，规范化后排序哈希（防整封重放给其他人）
    pub recipients: &'a [String],
    /// (文件名, 内容)
    pub attachments: &'a [(String, Vec<u8>)],
    /// Message-ID（含尖括号）；缺省不参与 v2，v3 发送时必填
    pub message_id: Option<&'a str>,
}

#[derive(Clone, Debug)]
pub struct ContentHashes {
    pub subject: String,
    pub body: String,
    pub html: String,
    pub attach: String,
    pub to: String,
    pub mid: String,
}

pub fn content_hashes(c: &SignContent<'_>) -> ContentHashes {
    ContentHashes {
        subject: body_hash_hex(c.subject),
        body: body_hash_hex(c.body_text),
        html: match c.body_html {
            Some(h) if !h.trim().is_empty() => body_hash_hex(h),
            _ => EMPTY_HASH_TOKEN.into(),
        },
        attach: attachments_hash_hex(c.attachments),
        to: recipients_hash_hex(c.recipients),
        mid: match c.message_id {
            Some(m) if !m.trim().is_empty() => body_hash_hex(&normalize_message_id(m)),
            _ => EMPTY_HASH_TOKEN.into(),
        },
    }
}

/// Message-ID 规范化：统一成 `<...>` 形式再哈希，避免 builder/parser 尖括号差异。
pub fn normalize_message_id(raw: &str) -> String {
    let t = raw.trim();
    if t.is_empty() {
        return String::new();
    }
    if t.starts_with('<') && t.ends_with('>') {
        t.to_string()
    } else {
        format!("<{t}>")
    }
}

/// 生成签名用 Message-ID
pub fn generate_message_id() -> String {
    let mut rnd = [0u8; 8];
    let _ = getrandom::getrandom(&mut rnd);
    format!(
        "<sm.{}.{}@sealmail.local>",
        chrono::Utc::now().timestamp_millis(),
        hex::encode(rnd)
    )
}

pub fn recipients_hash_hex(addrs: &[String]) -> String {
    let mut list: Vec<String> = addrs
        .iter()
        .map(|a| a.trim().to_lowercase())
        .filter(|a| !a.is_empty())
        .collect();
    list.sort();
    list.dedup();
    if list.is_empty() {
        return EMPTY_HASH_TOKEN.into();
    }
    body_hash_hex(&list.join(","))
}

pub fn attachments_hash_hex(attachments: &[(String, Vec<u8>)]) -> String {
    if attachments.is_empty() {
        return EMPTY_HASH_TOKEN.into();
    }
    let mut lines: Vec<String> = attachments
        .iter()
        .map(|(name, data)| {
            let safe = name.replace('|', "_").replace('\n', " ");
            format!("{safe}:{}:{}", data.len(), bytes_hash_hex(data))
        })
        .collect();
    lines.sort();
    bytes_hash_hex(lines.join("\n").as_bytes())
}

/// v1 canon（历史邮件）：仅 from|date|body_hash
pub fn canon_string(from: &str, date: &str, body_hash: &str) -> String {
    format!("sealmail-v1|{}|{}|{}", from.to_lowercase(), date, body_hash)
}

/// v2 canon：主题/纯文本/HTML/附件/收件人
pub fn canon_string_v2(
    from: &str,
    date: &str,
    subject_hash: &str,
    body_hash: &str,
    html_hash: &str,
    attach_hash: &str,
    to_hash: &str,
) -> String {
    format!(
        "sealmail-v2|{}|{}|{}|{}|{}|{}|{}",
        from.to_lowercase(),
        date,
        subject_hash.trim().to_lowercase(),
        body_hash.trim().to_lowercase(),
        html_hash.trim().to_lowercase(),
        attach_hash.trim().to_lowercase(),
        to_hash.trim().to_lowercase()
    )
}

/// v3 = v2 + Message-ID 哈希（绑定单封邮件身份，抑制同内容跨会话重放）
pub fn canon_string_v3(
    from: &str,
    date: &str,
    subject_hash: &str,
    body_hash: &str,
    html_hash: &str,
    attach_hash: &str,
    to_hash: &str,
    mid_hash: &str,
) -> String {
    format!(
        "sealmail-v3|{}|{}|{}|{}|{}|{}|{}|{}",
        from.to_lowercase(),
        date,
        subject_hash.trim().to_lowercase(),
        body_hash.trim().to_lowercase(),
        html_hash.trim().to_lowercase(),
        attach_hash.trim().to_lowercase(),
        to_hash.trim().to_lowercase(),
        mid_hash.trim().to_lowercase()
    )
}

pub struct SignedHeaders {
    pub headers: Vec<(String, String)>,
}

fn headers_for_current(
    method: &str,
    id_header: (&str, String),
    from_addr: &str,
    date: String,
    hashes: &ContentHashes,
    signature: String,
    message_id: Option<&str>,
) -> SignedHeaders {
    let mut headers = vec![
        (H_VERSION.into(), CANON_VERSION_CURRENT.into()),
        (H_METHOD.into(), method.into()),
        (id_header.0.into(), id_header.1),
        (H_FROM.into(), from_addr.to_lowercase()),
        (H_DATE.into(), date),
        (H_SUBJECT_HASH.into(), hashes.subject.clone()),
        (H_BODY_HASH.into(), hashes.body.clone()),
        (H_HTML_HASH.into(), hashes.html.clone()),
        (H_ATTACH_HASH.into(), hashes.attach.clone()),
        (H_TO_HASH.into(), hashes.to.clone()),
        (H_MID_HASH.into(), hashes.mid.clone()),
        (H_SIGNATURE.into(), signature),
    ];
    if let Some(mid) = message_id {
        if !mid.trim().is_empty() {
            headers.insert(0, ("Message-ID".into(), mid.trim().to_string()));
        }
    }
    SignedHeaders { headers }
}

pub fn sign_email(identity: &Identity, from_addr: &str, content: &SignContent<'_>) -> SignedHeaders {
    let date = chrono::Utc::now().to_rfc3339();
    let hashes = content_hashes(content);
    let canon = canon_string_v3(
        from_addr,
        &date,
        &hashes.subject,
        &hashes.body,
        &hashes.html,
        &hashes.attach,
        &hashes.to,
        &hashes.mid,
    );
    let sig = identity.signing_key.sign(canon.as_bytes());
    headers_for_current(
        "ed25519",
        (H_PUBKEY, identity.public_key_b64()),
        from_addr,
        date,
        &hashes,
        B64.encode(sig.to_bytes()),
        content.message_id,
    )
}

/// Ledger 签名（EIP-191 personal_sign）。`sign` 回调把 canon 字节送往设备并返回 65 字节 r‖s‖v——
/// 注入回调便于测试（k256 软件密钥模拟设备）。
pub fn sign_email_eth(
    address: &str,
    from_addr: &str,
    content: &SignContent<'_>,
    sign: impl FnOnce(&[u8]) -> Result<[u8; 65], String>,
) -> Result<SignedHeaders, String> {
    let date = chrono::Utc::now().to_rfc3339();
    let hashes = content_hashes(content);
    let canon = canon_string_v3(
        from_addr,
        &date,
        &hashes.subject,
        &hashes.body,
        &hashes.html,
        &hashes.attach,
        &hashes.to,
        &hashes.mid,
    );
    let rsv = sign(canon.as_bytes())?;
    // 自检：签名必须能恢复出绑定地址，避免把坏签名发出去
    let recovered = eth_personal_recover(canon.as_bytes(), &rsv)?;
    if !recovered.eq_ignore_ascii_case(address) {
        return Err(format!(
            "设备返回的签名与绑定地址不符（恢复出 {recovered}，期望 {address}）。请确认 Ledger 上选择的是绑定时的账户。"
        ));
    }
    Ok(headers_for_current(
        "eth-personal",
        (H_ADDRESS, address.to_lowercase()),
        from_addr,
        date,
        &hashes,
        hex::encode(rsv),
        content.message_id,
    ))
}

/// EIP-191 personal_sign 的 ecrecover：返回小写 0x 地址。
pub fn eth_personal_recover(message: &[u8], sig_rsv: &[u8; 65]) -> Result<String, String> {
    use k256::ecdsa::{RecoveryId, Signature as K256Sig, VerifyingKey as K256Vk};
    use sha3::{Digest as _, Keccak256};

    let mut hasher = Keccak256::new();
    hasher.update(format!("\x19Ethereum Signed Message:\n{}", message.len()).as_bytes());
    hasher.update(message);
    let digest = hasher.finalize();

    let sig = K256Sig::from_slice(&sig_rsv[..64]).map_err(|_| "secp256k1 签名格式错误")?;
    let v = sig_rsv[64];
    let recid = RecoveryId::try_from((if v >= 27 { v - 27 } else { v }) & 1)
        .map_err(|_| "签名恢复位无效")?;
    let vk =
        K256Vk::recover_from_prehash(&digest, &sig, recid).map_err(|_| "无法从签名恢复公钥")?;

    let point = vk.to_encoded_point(false);
    let mut h = Keccak256::new();
    h.update(&point.as_bytes()[1..]);
    let out = h.finalize();
    Ok(format!("0x{}", hex::encode(&out[12..])))
}

/// 用 k256 软件密钥做 EIP-191 personal_sign（测试与 Ledger 行为对齐用）。
pub fn eth_personal_sign_with_key(secret: &[u8; 32], message: &[u8]) -> Result<[u8; 65], String> {
    use k256::ecdsa::SigningKey as K256Sk;
    use sha3::{Digest as _, Keccak256};

    let sk = K256Sk::from_slice(secret).map_err(|e| e.to_string())?;
    let mut hasher = Keccak256::new();
    hasher.update(format!("\x19Ethereum Signed Message:\n{}", message.len()).as_bytes());
    hasher.update(message);
    let digest = hasher.finalize();
    let (sig, recid) = sk
        .sign_prehash_recoverable(&digest)
        .map_err(|e| e.to_string())?;
    let mut out = [0u8; 65];
    out[..64].copy_from_slice(&sig.to_bytes());
    out[64] = 27 + recid.to_byte();
    Ok(out)
}

#[derive(Clone, Debug)]
pub struct SealHeaders {
    pub pubkey: String,
    pub signature: String,
    pub from: String,
    pub date: String,
    pub body_hash: String,
    /// "1" | "2" | "3"；缺省按 v1
    pub version: String,
    pub subject_hash: String,
    pub html_hash: String,
    pub attach_hash: String,
    pub to_hash: String,
    pub mid_hash: String,
}

impl SealHeaders {
    pub fn version_num(&self) -> u8 {
        match self.version.trim() {
            "3" => 3,
            "2" => 2,
            _ => 1,
        }
    }
}

/// 验证签名头。
/// 返回 Ok((fingerprint, content_matches, signed_body_hash, got_body_hash))
pub fn verify_headers(
    h: &SealHeaders,
    actual: &SignContent<'_>,
) -> Result<(String, bool, String, String), String> {
    let pk_bytes = B64.decode(h.pubkey.trim()).map_err(|_| "公钥格式错误")?;
    let pk_arr: [u8; 32] = pk_bytes.try_into().map_err(|_| "公钥长度错误")?;
    let vk = VerifyingKey::from_bytes(&pk_arr).map_err(|_| "公钥无效")?;
    let sig_bytes = B64.decode(h.signature.trim()).map_err(|_| "签名格式错误")?;
    let sig = Signature::from_slice(&sig_bytes).map_err(|_| "签名长度错误")?;
    let v = h.version_num();
    let canon = match v {
        3 => canon_string_v3(
            &h.from,
            &h.date,
            &h.subject_hash,
            &h.body_hash,
            &h.html_hash,
            &h.attach_hash,
            &h.to_hash,
            &h.mid_hash,
        ),
        2 => canon_string_v2(
            &h.from,
            &h.date,
            &h.subject_hash,
            &h.body_hash,
            &h.html_hash,
            &h.attach_hash,
            &h.to_hash,
        ),
        _ => canon_string(&h.from, &h.date, &h.body_hash),
    };
    vk.verify(canon.as_bytes(), &sig)
        .map_err(|_| "签名校验失败")?;
    let got = content_hashes(actual);
    let matches = match v {
        3 => {
            got.subject == h.subject_hash.trim().to_lowercase()
                && got.body == h.body_hash.trim().to_lowercase()
                && got.html == h.html_hash.trim().to_lowercase()
                && got.attach == h.attach_hash.trim().to_lowercase()
                && got.to == h.to_hash.trim().to_lowercase()
                && got.mid == h.mid_hash.trim().to_lowercase()
        }
        2 => {
            got.subject == h.subject_hash.trim().to_lowercase()
                && got.body == h.body_hash.trim().to_lowercase()
                && got.html == h.html_hash.trim().to_lowercase()
                && got.attach == h.attach_hash.trim().to_lowercase()
                && got.to == h.to_hash.trim().to_lowercase()
        }
        _ => got.body == h.body_hash.trim().to_lowercase(),
    };
    Ok((fingerprint_of(&vk), matches, h.body_hash.clone(), got.body))
}

/// v1 签名且邮件含 HTML/附件时：正文绿标可信范围不足，不应给完整 Verified。
pub fn v1_unsigned_surface_present(body_html: Option<&str>, attachment_count: usize) -> bool {
    attachment_count > 0
        || body_html
            .map(|h| !h.trim().is_empty())
            .unwrap_or(false)
}

/// 签名时间是否过旧，不宜作为「当前可信指令」绿标（历史归档仍可看 SignedUnknown）。
pub fn signature_too_old(signed_date: &str) -> bool {
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(signed_date.trim()) else {
        return false;
    };
    let age = chrono::Utc::now().signed_duration_since(dt.with_timezone(&chrono::Utc));
    age.num_days() > MAX_VERIFIED_AGE_DAYS
}
