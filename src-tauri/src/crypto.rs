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
/// 签名方案：ed25519（本地密钥）| eth-personal（Ledger secp256k1 EIP-191）
pub const H_METHOD: &str = "X-SealMail-Method";
/// eth-personal 方案下签名者的以太坊地址（验证时与 ecrecover 结果比对）
pub const H_ADDRESS: &str = "X-SealMail-Address";

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

pub fn canon_string(from: &str, date: &str, body_hash: &str) -> String {
    format!("sealmail-v1|{}|{}|{}", from.to_lowercase(), date, body_hash)
}

pub struct SignedHeaders {
    pub headers: Vec<(String, String)>,
}

pub fn sign_email(identity: &Identity, from_addr: &str, body: &str) -> SignedHeaders {
    let date = chrono::Utc::now().to_rfc3339();
    let bh = body_hash_hex(body);
    let canon = canon_string(from_addr, &date, &bh);
    let sig = identity.signing_key.sign(canon.as_bytes());
    SignedHeaders {
        headers: vec![
            (H_VERSION.into(), "1".into()),
            (H_METHOD.into(), "ed25519".into()),
            (H_PUBKEY.into(), identity.public_key_b64()),
            (H_FROM.into(), from_addr.to_lowercase()),
            (H_DATE.into(), date),
            (H_BODY_HASH.into(), bh),
            (H_SIGNATURE.into(), B64.encode(sig.to_bytes())),
        ],
    }
}

/// Ledger 签名（EIP-191 personal_sign）。`sign` 回调把 canon 字节送往设备并返回 65 字节 r‖s‖v——
/// 注入回调便于测试（k256 软件密钥模拟设备）。
pub fn sign_email_eth(
    address: &str,
    from_addr: &str,
    body: &str,
    sign: impl FnOnce(&[u8]) -> Result<[u8; 65], String>,
) -> Result<SignedHeaders, String> {
    let date = chrono::Utc::now().to_rfc3339();
    let bh = body_hash_hex(body);
    let canon = canon_string(from_addr, &date, &bh);
    let rsv = sign(canon.as_bytes())?;
    // 自检：签名必须能恢复出绑定地址，避免把坏签名发出去
    let recovered = eth_personal_recover(canon.as_bytes(), &rsv)?;
    if !recovered.eq_ignore_ascii_case(address) {
        return Err(format!(
            "设备返回的签名与绑定地址不符（恢复出 {recovered}，期望 {address}）。请确认 Ledger 上选择的是绑定时的账户。"
        ));
    }
    Ok(SignedHeaders {
        headers: vec![
            (H_VERSION.into(), "1".into()),
            (H_METHOD.into(), "eth-personal".into()),
            (H_ADDRESS.into(), address.to_lowercase()),
            (H_FROM.into(), from_addr.to_lowercase()),
            (H_DATE.into(), date),
            (H_BODY_HASH.into(), bh),
            (H_SIGNATURE.into(), hex::encode(rsv)),
        ],
    })
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

pub struct SealHeaders {
    pub pubkey: String,
    pub signature: String,
    pub from: String,
    pub date: String,
    pub body_hash: String,
}

/// 验证签名头。返回 Ok((fingerprint, body_hash_matches, signed_hash, got_hash, signed_from))
pub fn verify_headers(
    h: &SealHeaders,
    actual_body: &str,
) -> Result<(String, bool, String, String), String> {
    let pk_bytes = B64.decode(h.pubkey.trim()).map_err(|_| "公钥格式错误")?;
    let pk_arr: [u8; 32] = pk_bytes.try_into().map_err(|_| "公钥长度错误")?;
    let vk = VerifyingKey::from_bytes(&pk_arr).map_err(|_| "公钥无效")?;
    let sig_bytes = B64.decode(h.signature.trim()).map_err(|_| "签名格式错误")?;
    let sig = Signature::from_slice(&sig_bytes).map_err(|_| "签名长度错误")?;
    let canon = canon_string(&h.from, &h.date, &h.body_hash);
    vk.verify(canon.as_bytes(), &sig)
        .map_err(|_| "签名校验失败")?;
    let got = body_hash_hex(actual_body);
    let matches = got == h.body_hash.trim().to_lowercase();
    Ok((fingerprint_of(&vk), matches, h.body_hash.clone(), got))
}
