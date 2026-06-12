//! 升级检测：tauri-plugin-updater 走 latest.json + 签名校验（前端调用）；
//! 本模块提供回退路径——当签名升级不可用（如本地构建无公钥）时，
//! 直接查 GitHub Releases API，引导用户手动下载匹配平台的安装包。
//! UX 参考 auto-desktop。

use serde::{Deserialize, Serialize};

const LATEST_URL: &str = "https://api.github.com/repos/sealmail/seal-mail/releases/latest";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub available: bool,
    pub release_url: String,
    pub download_url: Option<String>,
}

#[derive(Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    pub html_url: String,
    #[serde(default)]
    pub assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
pub struct GithubAsset {
    pub name: String,
    pub browser_download_url: String,
}

fn version_parts(version: &str) -> Vec<u64> {
    version
        .trim_start_matches('v')
        .split('.')
        .map(|part| {
            part.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u64>()
                .unwrap_or(0)
        })
        .collect()
}

pub fn is_newer_version(latest: &str, current: &str) -> bool {
    let mut l = version_parts(latest);
    let mut c = version_parts(current);
    let n = l.len().max(c.len());
    l.resize(n, 0);
    c.resize(n, 0);
    l > c
}

/// 当前平台/架构对应的发布资产（资产命名见 release.yml 的 Stage 步骤）。
pub fn matching_release_asset(assets: &[GithubAsset]) -> Option<&GithubAsset> {
    #[cfg(target_os = "macos")]
    {
        let arch = if cfg!(target_arch = "aarch64") {
            "aarch64"
        } else {
            "x64"
        };
        return assets.iter().find(|asset| {
            let name = asset.name.to_ascii_lowercase();
            name.contains("macos") && name.contains(arch) && name.ends_with(".dmg")
        });
    }
    #[cfg(target_os = "windows")]
    {
        return assets.iter().find(|asset| {
            let name = asset.name.to_ascii_lowercase();
            name.contains("windows") && name.contains("x64") && name.ends_with("setup.exe")
        });
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        assets.first()
    }
}

pub async fn check_for_update() -> Result<UpdateInfo, String> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let release: GithubRelease = reqwest::Client::new()
        .get(LATEST_URL)
        .header(reqwest::header::USER_AGENT, "SealMail")
        .send()
        .await
        .map_err(|e| format!("检查更新失败: {e}"))?
        .error_for_status()
        .map_err(|e| format!("检查更新失败: {e}"))?
        .json()
        .await
        .map_err(|e| format!("解析更新信息失败: {e}"))?;
    let latest = release.tag_name.trim_start_matches('v').to_string();
    let available = is_newer_version(&latest, &current);
    let download_url = if available {
        matching_release_asset(&release.assets)
            .map(|a| a.browser_download_url.clone())
            .or_else(|| Some(release.html_url.clone()))
    } else {
        None
    };
    Ok(UpdateInfo {
        current_version: current,
        latest_version: latest,
        available,
        release_url: release.html_url,
        download_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(is_newer_version("0.2.0", "0.1.9"));
        assert!(is_newer_version("v1.0.0", "0.9.9"));
        assert!(is_newer_version("0.1.10", "0.1.9"));
        assert!(!is_newer_version("0.1.0", "0.1.0"));
        assert!(!is_newer_version("0.1.0", "0.2.0"));
        // 带后缀的分量只取数字前缀
        assert!(is_newer_version("0.2.0-beta1", "0.1.0"));
        // 长度不一致时补零
        assert!(is_newer_version("0.1.0.1", "0.1.0"));
        assert!(!is_newer_version("0.1", "0.1.0"));
    }
}
