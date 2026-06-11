//! Microsoft OAuth2 设备码流程 + XOAUTH2。
//! 微软已停用 Exchange Online / Outlook.com 的 IMAP/POP/SMTP 基本认证
//! （官方文档明确 "requires the use of Modern Auth / OAuth2"），
//! 因此这里实现 RFC 8628 设备码授权：
//!   1) begin_device_flow → 用户在浏览器 microsoft.com/devicelogin 输入代码登录
//!   2) poll_device 轮询令牌端点直到拿到 access_token + refresh_token
//!   3) 连接 IMAP/POP/SMTP 时用 XOAUTH2 SASL（access_token 过期前自动 refresh）
//! 令牌与密码一样只存本机 secrets.json（0600）。

use serde::{Deserialize, Serialize};

/// 默认用 Thunderbird 的公共客户端 ID（注册为公共客户端、预置邮件权限，
/// 已实测 /common 租户支持设备码流程）。组织若禁止第三方应用，可在界面里
/// 改填自己注册的 Azure 应用 Client ID。
pub const DEFAULT_MS_CLIENT_ID: &str = "9e5f94bc-e8a4-4e73-b8be-63364c29d753";
const DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
const SCOPES: &str = "https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/POP.AccessAsUser.All https://outlook.office.com/SMTP.Send offline_access";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: String,
    /// unix 秒
    pub expires_at: i64,
    pub client_id: String,
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl OAuthTokens {
    /// 到期前 2 分钟即视为需要刷新，避免连接过程中过期
    pub fn needs_refresh(&self) -> bool {
        self.expires_at - 120 <= now_unix()
    }
}

/// XOAUTH2 SASL 初始响应（base64 前的原文）
pub fn xoauth2_string(user: &str, access_token: &str) -> String {
    format!("user={}\x01auth=Bearer {}\x01\x01", user, access_token)
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceFlowStart {
    pub user_code: String,
    pub verification_uri: String,
    pub message: String,
    pub device_code: String,
    /// 轮询间隔（秒）
    pub interval: u64,
    pub expires_in: u64,
    pub client_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum DevicePoll {
    /// 用户尚未在浏览器完成登录，继续轮询
    Pending,
    Ok { tokens: OAuthTokens },
}

async fn form_post(params: &[(&str, &str)], url: &str) -> Result<serde_json::Value, String> {
    let resp = reqwest::Client::new()
        .post(url)
        .form(params)
        .send()
        .await
        .map_err(|e| format!("请求微软登录服务失败: {e}"))?;
    resp.json()
        .await
        .map_err(|e| format!("解析微软登录响应失败: {e}"))
}

fn ms_error(v: &serde_json::Value) -> String {
    v["error_description"]
        .as_str()
        .or_else(|| v["error"].as_str())
        .unwrap_or("未知错误")
        .to_string()
}

pub async fn begin_device_flow(client_id: &str) -> Result<DeviceFlowStart, String> {
    let v = form_post(&[("client_id", client_id), ("scope", SCOPES)], DEVICE_CODE_URL).await?;
    if v.get("error").is_some() {
        return Err(format!("发起设备码授权失败: {}", ms_error(&v)));
    }
    Ok(DeviceFlowStart {
        user_code: v["user_code"].as_str().ok_or("响应缺少 user_code")?.into(),
        verification_uri: v["verification_uri"]
            .as_str()
            .ok_or("响应缺少 verification_uri")?
            .into(),
        message: v["message"].as_str().unwrap_or_default().into(),
        device_code: v["device_code"].as_str().ok_or("响应缺少 device_code")?.into(),
        interval: v["interval"].as_u64().unwrap_or(5),
        expires_in: v["expires_in"].as_u64().unwrap_or(900),
        client_id: client_id.to_string(),
    })
}

/// 解析令牌端点成功响应（设备码流程要求带 refresh_token，即 offline_access 生效）
fn parse_tokens(v: &serde_json::Value, client_id: &str, old_refresh: Option<&str>) -> Result<OAuthTokens, String> {
    let access_token = v["access_token"]
        .as_str()
        .ok_or("令牌响应缺少 access_token")?
        .to_string();
    let expires_in = v["expires_in"].as_i64().ok_or("令牌响应缺少 expires_in")?;
    // 刷新响应允许不返回新 refresh_token（沿用旧的）；首次授权必须有
    let refresh_token = match (v["refresh_token"].as_str(), old_refresh) {
        (Some(r), _) => r.to_string(),
        (None, Some(old)) => old.to_string(),
        (None, None) => return Err("令牌响应缺少 refresh_token（请确认 offline_access 权限）".into()),
    };
    Ok(OAuthTokens {
        access_token,
        refresh_token,
        expires_at: now_unix() + expires_in,
        client_id: client_id.to_string(),
    })
}

pub async fn poll_device(client_id: &str, device_code: &str) -> Result<DevicePoll, String> {
    let v = form_post(
        &[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", client_id),
            ("device_code", device_code),
        ],
        TOKEN_URL,
    )
    .await?;
    match v["error"].as_str() {
        None => Ok(DevicePoll::Ok { tokens: parse_tokens(&v, client_id, None)? }),
        Some("authorization_pending") | Some("slow_down") => Ok(DevicePoll::Pending),
        Some("authorization_declined") => Err("你在登录页面拒绝了授权".into()),
        Some("expired_token") => Err("登录代码已过期，请重新获取".into()),
        Some(_) => Err(format!("授权失败: {}", ms_error(&v))),
    }
}

pub async fn refresh_tokens(old: &OAuthTokens) -> Result<OAuthTokens, String> {
    let v = form_post(
        &[
            ("grant_type", "refresh_token"),
            ("client_id", &old.client_id),
            ("refresh_token", &old.refresh_token),
            ("scope", SCOPES),
        ],
        TOKEN_URL,
    )
    .await?;
    if v.get("error").is_some() {
        return Err(format!(
            "OAuth2 授权已失效，请在账户设置中重新授权: {}",
            ms_error(&v)
        ));
    }
    parse_tokens(&v, &old.client_id, Some(&old.refresh_token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xoauth2_sasl_format() {
        let s = xoauth2_string("a@b.com", "TOKEN");
        assert_eq!(s, "user=a@b.com\u{1}auth=Bearer TOKEN\u{1}\u{1}");
    }

    #[test]
    fn parse_token_response() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"token_type":"Bearer","scope":"IMAP.AccessAsUser.All","expires_in":3600,
                "access_token":"AT","refresh_token":"RT"}"#,
        )
        .unwrap();
        let t = parse_tokens(&v, "cid", None).unwrap();
        assert_eq!(t.access_token, "AT");
        assert_eq!(t.refresh_token, "RT");
        assert_eq!(t.client_id, "cid");
        assert!(t.expires_at > now_unix() + 3000);
        assert!(!t.needs_refresh());

        // 刷新响应不带 refresh_token 时沿用旧值
        let v2: serde_json::Value =
            serde_json::from_str(r#"{"expires_in":10,"access_token":"AT2"}"#).unwrap();
        let t2 = parse_tokens(&v2, "cid", Some("OLD_RT")).unwrap();
        assert_eq!(t2.refresh_token, "OLD_RT");
        // 仅剩 10 秒有效期 → 需要刷新
        assert!(t2.needs_refresh());

        // 首次授权缺 refresh_token 必须报错
        assert!(parse_tokens(&v2, "cid", None).is_err());
    }
}
