//! 账户凭据存储：优先系统钥匙串（macOS Keychain / Windows Credential Manager / secret-service），
//! 不可用时回退到 secrets.json（0600）。
//!
//! 钥匙串写入成功后，磁盘上只保留 `{"backend":"keychain"}` 占位标记，**不再**落盘明文密码。
//! 这样 CLI 与 GUI 同用户同配置目录仍可从钥匙串读到凭据；无钥匙串环境才用文件。

use crate::models::AccountSecret;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

const SERVICE: &str = "com.sealmail.app";

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum SecretsFile {
    Map(HashMap<String, AccountSecret>),
    Marker { backend: String },
}

/// 每个配置目录独立钥匙串条目，避免测试/多 profile 互踩。
fn entry_for(dir: &Path) -> Result<keyring::Entry, String> {
    use sha2::{Digest, Sha256};
    let canon = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let digest = Sha256::digest(canon.to_string_lossy().as_bytes());
    let name = format!("secrets-{}", hex::encode(&digest[..8]));
    keyring::Entry::new(SERVICE, &name).map_err(|e| format!("钥匙串不可用: {e}"))
}

fn keychain_read(dir: &Path) -> Result<HashMap<String, AccountSecret>, String> {
    let e = entry_for(dir)?;
    let raw = e
        .get_password()
        .map_err(|err| format!("读取钥匙串失败: {err}"))?;
    serde_json::from_str(&raw).map_err(|err| format!("解析钥匙串凭据失败: {err}"))
}

fn keychain_write(dir: &Path, map: &HashMap<String, AccountSecret>) -> Result<(), String> {
    let e = entry_for(dir)?;
    let raw = serde_json::to_string(map).map_err(|err| err.to_string())?;
    e.set_password(&raw)
        .map_err(|err| format!("写入钥匙串失败: {err}"))
}

fn file_path(dir: &Path) -> PathBuf {
    dir.join("secrets.json")
}

fn write_marker(dir: &Path) -> Result<(), String> {
    let p = file_path(dir);
    let marker = serde_json::json!({ "backend": "keychain" });
    let s = serde_json::to_string_pretty(&marker).map_err(|e| e.to_string())?;
    let tmp = p.with_extension("json.tmp");
    fs::write(&tmp, &s).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &p).map_err(|e| e.to_string())?;
    crate::crypto::restrict_perms(&p);
    Ok(())
}

fn write_file_map(dir: &Path, map: &HashMap<String, AccountSecret>) -> Result<(), String> {
    let p = file_path(dir);
    let s = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
    let tmp = p.with_extension("json.tmp");
    fs::write(&tmp, &s).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &p).map_err(|e| e.to_string())?;
    crate::crypto::restrict_perms(&p);
    Ok(())
}

fn file_is_keychain_marker(dir: &Path) -> bool {
    let Ok(raw) = fs::read_to_string(file_path(dir)) else {
        return false;
    };
    matches!(
        serde_json::from_str::<SecretsFile>(&raw),
        Ok(SecretsFile::Marker { .. })
    )
}

fn read_file_map(dir: &Path) -> Result<HashMap<String, AccountSecret>, String> {
    let p = file_path(dir);
    match fs::read_to_string(&p) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(HashMap::new()),
        Err(e) => Err(format!("读取 secrets.json 失败: {e}")),
        Ok(raw) if raw.trim().is_empty() => Ok(HashMap::new()),
        Ok(raw) => match serde_json::from_str::<SecretsFile>(&raw) {
            Ok(SecretsFile::Map(m)) => Ok(m),
            Ok(SecretsFile::Marker { .. }) => Ok(HashMap::new()),
            Err(e) => {
                let backup = p.with_extension(format!(
                    "json.corrupt.{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0)
                ));
                let _ = fs::rename(&p, &backup);
                Err(format!("解析 secrets.json 失败（已备份）: {e}"))
            }
        },
    }
}

/// 加载凭据：钥匙串优先；失败且磁盘声明在钥匙串时要报错，不能静默变空表。
pub fn load(dir: &Path) -> Result<HashMap<String, AccountSecret>, String> {
    match keychain_read(dir) {
        Ok(m) if !m.is_empty() => Ok(m),
        Ok(_empty) => {
            // 钥匙串空：尝试从文件迁移旧明文
            let file_map = read_file_map(dir)?;
            if !file_map.is_empty() {
                if keychain_write(dir, &file_map).is_ok() {
                    let _ = write_marker(dir);
                }
            }
            Ok(file_map)
        }
        Err(kc_err) => {
            // 磁盘写着 keychain 占位，却读不出钥匙串 → 绝不能当成无账户
            if file_is_keychain_marker(dir) {
                return Err(format!(
                    "凭据保存在系统钥匙串中，但当前无法读取：{kc_err}"
                ));
            }
            read_file_map(dir)
        }
    }
}

/// 保存凭据：优先钥匙串；成功后磁盘只留标记（无明文）。钥匙串失败才写文件。
pub fn save(dir: &Path, map: &HashMap<String, AccountSecret>) -> Result<(), String> {
    match keychain_write(dir, map) {
        Ok(()) => {
            // 故意不把明文再写回 secrets.json
            write_marker(dir)
        }
        Err(_kc_err) => write_file_map(dir, map),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static N: AtomicUsize = AtomicUsize::new(0);

    fn temp() -> PathBuf {
        let id = N.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!(
            "sealmail-sec-{}-{}-{}",
            std::process::id(),
            id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|t| t.as_secs())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn file_roundtrip_when_used_as_fallback() {
        let dir = temp();
        let mut m = HashMap::new();
        m.insert(
            "a1".into(),
            AccountSecret {
                password: "p".into(),
                smtp_password: None,
                oauth: None,
            },
        );
        write_file_map(&dir, &m).unwrap();
        let loaded = read_file_map(&dir).unwrap();
        assert_eq!(loaded.get("a1").unwrap().password, "p");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn marker_file_is_not_treated_as_account_map() {
        let dir = temp();
        write_marker(&dir).unwrap();
        let m = read_file_map(&dir).unwrap();
        assert!(m.is_empty());
        assert!(file_is_keychain_marker(&dir));
        let _ = fs::remove_dir_all(dir);
    }
}
