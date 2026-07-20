//! OAuth2 设备码流程 + XOAUTH2。
//! Microsoft/Google 都不应再用普通账户密码登录第三方邮件客户端：
//!   1) begin_device_flow → 用户在浏览器输入设备码完成登录
//!   2) poll_device 轮询令牌端点直到拿到 access_token + refresh_token
//!   3) 连接 IMAP/POP/SMTP 时用 XOAUTH2 SASL（access_token 过期前自动 refresh）
//! 令牌与密码一样只存本机 secrets.json（0600）。

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// 默认用 Thunderbird 的公共客户端 ID（注册为公共客户端、预置邮件权限，
/// 已实测 /common 租户支持设备码流程）。组织若禁止第三方应用，可在界面里
/// 改填自己注册的 Azure 应用 Client ID。
pub const DEFAULT_MS_CLIENT_ID: &str = "9e5f94bc-e8a4-4e73-b8be-63364c29d753";
fn default_google_client_id() -> &'static str {
    option_env!("GOOGLE_OAUTH_CLIENT_ID").unwrap_or("")
}
fn default_google_client_secret() -> &'static str {
    option_env!("GOOGLE_OAUTH_CLIENT_SECRET").unwrap_or("")
}
const MS_DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/devicecode";
const MS_TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
const MS_SCOPES: &str = "https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/POP.AccessAsUser.All https://outlook.office.com/SMTP.Send offline_access";

const GOOGLE_DEVICE_CODE_URL: &str = "https://oauth2.googleapis.com/device/code";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_SCOPES: &str = "https://mail.google.com/";
const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OAuthProvider {
    Microsoft,
    Google,
}

impl OAuthProvider {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "microsoft" => Ok(Self::Microsoft),
            "google" => Ok(Self::Google),
            _ => Err(format!("不支持的 OAuth2 服务商: {s}")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Microsoft => "microsoft",
            Self::Google => "google",
        }
    }

    fn service_name(self) -> &'static str {
        match self {
            Self::Microsoft => "微软登录服务",
            Self::Google => "Google 登录服务",
        }
    }

    fn device_code_url(self) -> &'static str {
        match self {
            Self::Microsoft => MS_DEVICE_CODE_URL,
            Self::Google => GOOGLE_DEVICE_CODE_URL,
        }
    }

    fn token_url(self) -> &'static str {
        match self {
            Self::Microsoft => MS_TOKEN_URL,
            Self::Google => GOOGLE_TOKEN_URL,
        }
    }

    fn scopes(self) -> &'static str {
        match self {
            Self::Microsoft => MS_SCOPES,
            Self::Google => GOOGLE_SCOPES,
        }
    }
}

fn default_provider() -> String {
    "microsoft".into()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: String,
    /// unix 秒
    pub expires_at: i64,
    pub client_id: String,
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default = "default_provider")]
    pub provider: String,
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

    pub fn is_expired(&self) -> bool {
        self.expires_at <= now_unix()
    }
}

/// 提前刷新只是优化：刷新端点短时故障时，尚未过期的 access token 仍可继续尝试邮件操作。
/// 真正过期的 token 不能复用；服务器明确拒绝后的强制刷新也不走此降级。
pub fn resolve_proactive_refresh(
    current: &OAuthTokens,
    result: Result<OAuthTokens, String>,
) -> Result<OAuthTokens, String> {
    match result {
        Ok(tokens) => Ok(tokens),
        Err(_) if !current.is_expired() => Ok(current.clone()),
        Err(e) => Err(e),
    }
}

/// 判断错误是否为「服务器拒绝了 OAuth2 认证」。
/// access_token 可能在本地 expires_at 之前就被服务器作废（改密码/安全事件/用户撤销授权），
/// 这种错误单看本地过期时间永远不会触发刷新，必须强制刷新令牌后重试。
/// 匹配依据：IMAP/POP3 客户端的 OAuth2 登录失败标记，以及 SMTP 535 认证被拒状态码。
pub fn is_auth_rejected(err: &str) -> bool {
    err.contains("OAuth2 登录失败") || err.contains("(535)")
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
#[serde(rename_all = "camelCase")]
pub struct BrowserFlowStart {
    pub flow_id: String,
    pub auth_url: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum DevicePoll {
    /// 用户尚未在浏览器完成登录，继续轮询
    Pending,
    Ok {
        tokens: OAuthTokens,
    },
}

struct PendingBrowserFlow {
    listener: TcpListener,
    provider: OAuthProvider,
    client_id: String,
    client_secret: Option<String>,
    redirect_uri: String,
    code_verifier: String,
    state: String,
}

static BROWSER_FLOWS: OnceLock<Mutex<HashMap<String, PendingBrowserFlow>>> = OnceLock::new();

fn browser_flows() -> &'static Mutex<HashMap<String, PendingBrowserFlow>> {
    BROWSER_FLOWS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn random_urlsafe(bytes: usize) -> Result<String, String> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let mut b = vec![0u8; bytes];
    getrandom::getrandom(&mut b).map_err(|e| format!("生成 OAuth 随机数失败: {e}"))?;
    Ok(URL_SAFE_NO_PAD.encode(b))
}

fn pkce_challenge(verifier: &str) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query_param(path: &str, key: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        (percent_decode(k) == key).then(|| percent_decode(v))
    })
}

async fn form_post(
    provider: OAuthProvider,
    params: &[(&str, &str)],
    url: &str,
) -> Result<serde_json::Value, String> {
    let resp = reqwest::Client::new()
        .post(url)
        .form(params)
        .send()
        .await
        .map_err(|e| format!("请求{}失败: {e}", provider.service_name()))?;
    resp.json()
        .await
        .map_err(|e| format!("解析{}响应失败: {e}", provider.service_name()))
}

fn ms_error(v: &serde_json::Value) -> String {
    v["error_description"]
        .as_str()
        .or_else(|| v["error"].as_str())
        .unwrap_or("未知错误")
        .to_string()
}

pub async fn begin_device_flow(
    provider: OAuthProvider,
    client_id: &str,
) -> Result<DeviceFlowStart, String> {
    if provider == OAuthProvider::Google && client_id.trim().is_empty() {
        return Err("Gmail OAuth2 需要填写 Google Cloud OAuth Client ID".into());
    }
    let v = form_post(
        provider,
        &[("client_id", client_id), ("scope", provider.scopes())],
        provider.device_code_url(),
    )
    .await?;
    if v.get("error").is_some() {
        return Err(format!("发起设备码授权失败: {}", ms_error(&v)));
    }
    Ok(DeviceFlowStart {
        user_code: v["user_code"].as_str().ok_or("响应缺少 user_code")?.into(),
        verification_uri: v["verification_uri"]
            .as_str()
            .or_else(|| v["verification_url"].as_str())
            .or_else(|| v["verification_uri_complete"].as_str())
            .ok_or("响应缺少 verification_uri")?
            .into(),
        message: v["message"].as_str().unwrap_or_default().into(),
        device_code: v["device_code"]
            .as_str()
            .ok_or("响应缺少 device_code")?
            .into(),
        interval: v["interval"].as_u64().unwrap_or(5),
        expires_in: v["expires_in"].as_u64().unwrap_or(900),
        client_id: client_id.to_string(),
    })
}

pub fn begin_browser_flow(
    provider: OAuthProvider,
    client_id: &str,
    client_secret: Option<&str>,
    login_hint: Option<&str>,
) -> Result<BrowserFlowStart, String> {
    if provider != OAuthProvider::Google {
        return Err("浏览器登录目前仅用于 Gmail / Google".into());
    }
    let client_id = if client_id.trim().is_empty() {
        default_google_client_id()
    } else {
        client_id.trim()
    };
    if client_id.is_empty() {
        return Err("Gmail OAuth2 需要填写 Google Cloud Desktop OAuth Client ID".into());
    }
    let client_secret = client_secret
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let s = default_google_client_secret();
            (!s.is_empty()).then_some(s)
        });

    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|e| format!("启动 Google 登录本机回调失败: {e}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("设置 Google 登录超时失败: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("读取 Google 登录回调地址失败: {e}"))?
        .port();
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let state = random_urlsafe(24)?;
    let code_verifier = random_urlsafe(64)?;
    let code_challenge = pkce_challenge(&code_verifier);
    let flow_id = random_urlsafe(18)?;

    let mut url = reqwest::Url::parse(GOOGLE_AUTH_URL)
        .map_err(|e| format!("Google 登录地址无效: {e}"))?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", client_id);
        q.append_pair("redirect_uri", &redirect_uri);
        q.append_pair("response_type", "code");
        q.append_pair("scope", GOOGLE_SCOPES);
        q.append_pair("state", &state);
        q.append_pair("code_challenge", &code_challenge);
        q.append_pair("code_challenge_method", "S256");
        q.append_pair("access_type", "offline");
        q.append_pair("prompt", "consent");
        if let Some(hint) = login_hint.map(str::trim).filter(|h| !h.is_empty()) {
            q.append_pair("login_hint", hint);
        }
    }

    browser_flows().lock().unwrap().insert(
        flow_id.clone(),
        PendingBrowserFlow {
            listener,
            provider,
            client_id: client_id.to_string(),
            client_secret: client_secret.map(str::to_string),
            redirect_uri,
            code_verifier,
            state,
        },
    );

    Ok(BrowserFlowStart {
        flow_id,
        auth_url: url.to_string(),
    })
}

fn read_callback(stream: &mut TcpStream) -> Result<String, String> {
    let mut buf = [0u8; 4096];
    let n = stream
        .read(&mut buf)
        .map_err(|e| format!("读取 Google 登录回调失败: {e}"))?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let first = req.lines().next().ok_or("Google 登录回调为空")?;
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");
    if method != "GET" || path.is_empty() {
        return Err("Google 登录回调格式无效".into());
    }
    Ok(path.to_string())
}

fn write_callback_page(stream: &mut TcpStream, ok: bool, detail: &str) {
    let title = if ok { "SealMail 登录成功" } else { "SealMail 登录失败" };
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>{title}</title><body style=\"font-family:-apple-system,BlinkMacSystemFont,sans-serif;padding:32px\"><h2>{title}</h2><p>{detail}</p><p>可以关闭这个页面并回到 SealMail。</p></body>"
    );
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.as_bytes().len(),
        body
    );
    let _ = stream.write_all(resp.as_bytes());
}

pub async fn finish_browser_flow(flow_id: &str) -> Result<OAuthTokens, String> {
    let flow = browser_flows()
        .lock()
        .unwrap()
        .remove(flow_id)
        .ok_or("Google 登录会话已失效，请重新开始")?;

    let (path, mut stream, flow) = tauri::async_runtime::spawn_blocking(move || {
        let started = Instant::now();
        let mut stream = loop {
            match flow.listener.accept() {
                Ok((stream, _)) => break stream,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if started.elapsed() > Duration::from_secs(300) {
                        return Err("等待 Google 登录回调超时，请重新授权".into());
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(format!("等待 Google 登录回调失败: {e}")),
            }
        };
        let path = match read_callback(&mut stream) {
            Ok(path) => path,
            Err(e) => {
                write_callback_page(&mut stream, false, &e);
                return Err(e);
            }
        };
        Ok::<_, String>((path, stream, flow))
    })
    .await
    .map_err(|e| e.to_string())??;

    if let Some(err) = parse_query_param(&path, "error") {
        write_callback_page(&mut stream, false, &err);
        return Err(format!("Google 授权失败: {err}"));
    }
    let got_state = parse_query_param(&path, "state").ok_or("Google 登录回调缺少 state")?;
    if got_state != flow.state {
        write_callback_page(&mut stream, false, "state 校验失败");
        return Err("Google 登录 state 校验失败，请重新授权".into());
    }
    let code = parse_query_param(&path, "code").ok_or("Google 登录回调缺少授权码")?;

    let mut params = vec![
        ("client_id", flow.client_id.as_str()),
        ("code", code.as_str()),
        ("code_verifier", flow.code_verifier.as_str()),
        ("grant_type", "authorization_code"),
        ("redirect_uri", flow.redirect_uri.as_str()),
    ];
    if let Some(secret) = &flow.client_secret {
        params.push(("client_secret", secret.as_str()));
    }
    let v = form_post(flow.provider, &params, flow.provider.token_url())
    .await?;
    if v.get("error").is_some() {
        let msg = format!("Google 授权失败: {}", ms_error(&v));
        write_callback_page(&mut stream, false, &msg);
        return Err(msg);
    }
    let tokens = parse_tokens(
        &v,
        &flow.client_id,
        flow.client_secret.as_deref(),
        None,
        flow.provider,
    )?;
    write_callback_page(&mut stream, true, "Google 已完成授权。");
    Ok(tokens)
}

/// 解析令牌端点成功响应（设备码流程要求带 refresh_token，即 offline_access 生效）
fn parse_tokens(
    v: &serde_json::Value,
    client_id: &str,
    client_secret: Option<&str>,
    old_refresh: Option<&str>,
    provider: OAuthProvider,
) -> Result<OAuthTokens, String> {
    let access_token = v["access_token"]
        .as_str()
        .ok_or("令牌响应缺少 access_token")?
        .to_string();
    let expires_in = v["expires_in"].as_i64().ok_or("令牌响应缺少 expires_in")?;
    // 刷新响应允许不返回新 refresh_token（沿用旧的）；首次授权必须有
    let refresh_token = match (v["refresh_token"].as_str(), old_refresh) {
        (Some(r), _) => r.to_string(),
        (None, Some(old)) => old.to_string(),
        (None, None) => {
            return Err("令牌响应缺少 refresh_token（请确认 offline_access 权限）".into())
        }
    };
    Ok(OAuthTokens {
        access_token,
        refresh_token,
        expires_at: now_unix() + expires_in,
        client_id: client_id.to_string(),
        client_secret: client_secret.map(str::to_string),
        provider: provider.as_str().to_string(),
    })
}

pub async fn poll_device(client_id: &str, device_code: &str) -> Result<DevicePoll, String> {
    poll_device_for(OAuthProvider::Microsoft, client_id, None, device_code).await
}

pub async fn poll_device_for(
    provider: OAuthProvider,
    client_id: &str,
    client_secret: Option<&str>,
    device_code: &str,
) -> Result<DevicePoll, String> {
    if provider == OAuthProvider::Google && client_secret.unwrap_or("").trim().is_empty() {
        return Err("Gmail OAuth2 需要填写 Google Cloud OAuth Client Secret".into());
    }
    let mut params = vec![
        ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ("client_id", client_id),
        ("device_code", device_code),
    ];
    if let Some(secret) = client_secret {
        if !secret.trim().is_empty() {
            params.push(("client_secret", secret));
        }
    }
    let v = form_post(
        provider,
        &params,
        provider.token_url(),
    )
    .await?;
    match v["error"].as_str() {
        None => Ok(DevicePoll::Ok {
            tokens: parse_tokens(&v, client_id, client_secret, None, provider)?,
        }),
        Some("authorization_pending") | Some("slow_down") => Ok(DevicePoll::Pending),
        Some("authorization_declined") => Err("你在登录页面拒绝了授权".into()),
        Some("expired_token") => Err("登录代码已过期，请重新获取".into()),
        Some(_) => Err(format!("授权失败: {}", ms_error(&v))),
    }
}

pub async fn refresh_tokens(old: &OAuthTokens) -> Result<OAuthTokens, String> {
    let provider = OAuthProvider::parse(&old.provider)?;
    let mut params = vec![
        ("grant_type", "refresh_token"),
        ("client_id", old.client_id.as_str()),
        ("refresh_token", old.refresh_token.as_str()),
        ("scope", provider.scopes()),
    ];
    if let Some(secret) = &old.client_secret {
        if !secret.trim().is_empty() {
            params.push(("client_secret", secret.as_str()));
        }
    }
    let v = form_post(
        provider,
        &params,
        provider.token_url(),
    )
    .await?;
    if v.get("error").is_some() {
        return Err(format!(
            "OAuth2 授权已失效，请在账户设置中重新授权: {}",
            ms_error(&v)
        ));
    }
    parse_tokens(
        &v,
        &old.client_id,
        old.client_secret.as_deref(),
        Some(&old.refresh_token),
        provider,
    )
}

/// refresh_tokens 的阻塞版（后台监听线程用，那里没有 async 运行时）
pub fn refresh_tokens_blocking(old: &OAuthTokens) -> Result<OAuthTokens, String> {
    let provider = OAuthProvider::parse(&old.provider)?;
    let mut params = vec![
        ("grant_type", "refresh_token"),
        ("client_id", old.client_id.as_str()),
        ("refresh_token", old.refresh_token.as_str()),
        ("scope", provider.scopes()),
    ];
    if let Some(secret) = &old.client_secret {
        if !secret.trim().is_empty() {
            params.push(("client_secret", secret.as_str()));
        }
    }
    let v: serde_json::Value = reqwest::blocking::Client::new()
        .post(provider.token_url())
        .form(&params)
        .send()
        .map_err(|e| format!("请求{}失败: {e}", provider.service_name()))?
        .json()
        .map_err(|e| format!("解析{}响应失败: {e}", provider.service_name()))?;
    if v.get("error").is_some() {
        return Err(format!(
            "OAuth2 授权已失效，请在账户设置中重新授权: {}",
            ms_error(&v)
        ));
    }
    parse_tokens(
        &v,
        &old.client_id,
        old.client_secret.as_deref(),
        Some(&old.refresh_token),
        provider,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_rejected_detection() {
        // 线上真实错误：access_token 被服务器提前作废（改密码/撤销授权），本地 expires_at 未到期
        assert!(is_auth_rejected(
            "IMAP OAuth2 登录失败（授权可能已失效，请重新授权）: No Response: AUTHENTICATE failed."
        ));
        assert!(is_auth_rejected(
            "POP3 OAuth2 登录失败（授权可能已失效，请重新授权）: ERR invalid credentials"
        ));
        // SMTP XOAUTH2 被拒（lettre 535 永久错误）
        assert!(is_auth_rejected(
            "发送失败: permanent error (535): 5.7.8 Username and Password not accepted"
        ));
        // 网络错误、普通密码登录失败不得触发强制刷新
        assert!(!is_auth_rejected(
            "无法连接 imap.gmail.com:993 — Connection refused (os error 61)"
        ));
        assert!(!is_auth_rejected(
            "IMAP 登录失败（请检查用户名/密码或应用专用密码）: No Response: LOGIN failed."
        ));
        assert!(!is_auth_rejected("SMTP 连接失败: connection error"));
    }

    #[test]
    fn proactive_refresh_failure_keeps_a_still_valid_token() {
        let current = OAuthTokens {
            access_token: "still-valid".into(),
            refresh_token: "refresh".into(),
            expires_at: now_unix() + 30,
            client_id: "client".into(),
            client_secret: None,
            provider: "microsoft".into(),
        };
        assert!(current.needs_refresh());
        let kept = resolve_proactive_refresh(&current, Err("token endpoint offline".into())).unwrap();
        assert_eq!(kept.access_token, "still-valid");
    }

    #[test]
    fn proactive_refresh_failure_rejects_an_expired_token() {
        let current = OAuthTokens {
            access_token: "expired".into(),
            refresh_token: "refresh".into(),
            expires_at: now_unix() - 1,
            client_id: "client".into(),
            client_secret: None,
            provider: "microsoft".into(),
        };
        let err = resolve_proactive_refresh(&current, Err("token endpoint offline".into()))
            .expect_err("expired token must not be reused");
        assert!(err.contains("offline"));
    }

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
        let t = parse_tokens(&v, "cid", None, None, OAuthProvider::Microsoft).unwrap();
        assert_eq!(t.access_token, "AT");
        assert_eq!(t.refresh_token, "RT");
        assert_eq!(t.client_id, "cid");
        assert_eq!(t.provider, "microsoft");
        assert!(t.expires_at > now_unix() + 3000);
        assert!(!t.needs_refresh());

        // 刷新响应不带 refresh_token 时沿用旧值
        let v2: serde_json::Value =
            serde_json::from_str(r#"{"expires_in":10,"access_token":"AT2"}"#).unwrap();
        let t2 = parse_tokens(
            &v2,
            "cid",
            Some("secret"),
            Some("OLD_RT"),
            OAuthProvider::Google,
        )
        .unwrap();
        assert_eq!(t2.refresh_token, "OLD_RT");
        assert_eq!(t2.client_secret.as_deref(), Some("secret"));
        assert_eq!(t2.provider, "google");
        // 仅剩 10 秒有效期 → 需要刷新
        assert!(t2.needs_refresh());

        // 首次授权缺 refresh_token 必须报错
        assert!(parse_tokens(&v2, "cid", None, None, OAuthProvider::Microsoft).is_err());
    }
}
