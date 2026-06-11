import { useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { addAccount, oauthBeginDevice, oauthPollDevice, testConnection } from "../api";
import { PROVIDER_PRESETS } from "../types";
import type { Account, AccountSecret, DeviceFlowStart, OAuthTokens } from "../types";

interface Props {
  onClose: () => void;
  onAdded: (account: Account) => void;
}

export function AccountModal(p: Props) {
  const [presetKey, setPresetKey] = useState(PROVIDER_PRESETS[0].key);
  const preset = PROVIDER_PRESETS.find((x) => x.key === presetKey)!;

  const [email, setEmail] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [incomingHost, setIncomingHost] = useState(preset.incomingHost);
  const [incomingPort, setIncomingPort] = useState(preset.incomingPort);
  const [smtpHost, setSmtpHost] = useState(preset.smtpHost);
  const [smtpPort, setSmtpPort] = useState(preset.smtpPort);
  const [smtpSecurity, setSmtpSecurity] = useState<"ssl" | "starttls">(preset.smtpSecurity);
  const [busy, setBusy] = useState<"" | "test" | "save">("");
  const [error, setError] = useState<string | null>(null);
  const [ok, setOk] = useState<string | null>(null);

  // OAuth2 设备码授权状态（Exchange Online / Outlook.com 已强制 OAuth2）
  const [authMode, setAuthMode] = useState<"password" | "oauth2">(preset.oauth ? "oauth2" : "password");
  const [clientId, setClientId] = useState("");
  const [device, setDevice] = useState<DeviceFlowStart | null>(null);
  const [tokens, setTokens] = useState<OAuthTokens | null>(null);
  // 轮询代际：+1 即作废正在跑的轮询循环（取消/重开/关闭弹窗）
  const pollGen = useRef(0);
  useEffect(() => () => void pollGen.current++, []);

  function applyPreset(key: string) {
    const pr = PROVIDER_PRESETS.find((x) => x.key === key)!;
    setPresetKey(key);
    setIncomingHost(pr.incomingHost);
    setIncomingPort(pr.incomingPort);
    setSmtpHost(pr.smtpHost);
    setSmtpPort(pr.smtpPort);
    setSmtpSecurity(pr.smtpSecurity);
    setAuthMode(pr.oauth ? "oauth2" : "password");
    cancelDeviceFlow();
    setTokens(null);
    setOk(null);
    setError(null);
  }

  function cancelDeviceFlow() {
    pollGen.current++;
    setDevice(null);
  }

  async function startDeviceFlow() {
    setError(null);
    setOk(null);
    setTokens(null);
    const gen = ++pollGen.current;
    try {
      const d = await oauthBeginDevice(clientId.trim() || undefined);
      if (pollGen.current !== gen) return;
      setDevice(d);
      await openUrl(d.verificationUri);
      const intervalMs = Math.max(1, d.interval) * 1000;
      while (pollGen.current === gen) {
        await new Promise((r) => setTimeout(r, intervalMs));
        if (pollGen.current !== gen) return;
        const res = await oauthPollDevice(d.clientId, d.deviceCode);
        if (pollGen.current !== gen) return;
        if (res.status === "ok") {
          setTokens(res.tokens);
          setDevice(null);
          setOk("Microsoft 授权成功，现在可以测试连接并保存账户");
          return;
        }
      }
    } catch (e) {
      if (pollGen.current === gen) {
        setError(String(e));
        setDevice(null);
      }
    }
  }

  function buildAccount(): Account {
    return {
      id: "",
      label: preset.label.split("（")[0].split(" /")[0],
      email: email.trim(),
      displayName: displayName.trim() || email.split("@")[0],
      protocol: preset.protocol,
      incomingHost: incomingHost.trim(),
      incomingPort,
      smtpHost: smtpHost.trim(),
      smtpPort,
      smtpSecurity,
      username: (username || email).trim(),
      auth: authMode,
    };
  }

  function buildSecret(): AccountSecret {
    return authMode === "oauth2" ? { password: "", oauth: tokens } : { password };
  }

  function validate(): string | null {
    if (!email.includes("@")) return "请填写正确的邮箱地址";
    if (authMode === "oauth2" && !tokens) return "请先点击「用 Microsoft 账户授权」完成登录";
    if (authMode === "password" && !password) return "请填写密码 / 授权码";
    if (!incomingHost.trim() || !smtpHost.trim()) return "请填写服务器地址";
    return null;
  }

  async function doTest() {
    const v = validate();
    if (v) return setError(v);
    setBusy("test");
    setError(null);
    setOk(null);
    try {
      await testConnection(buildAccount(), buildSecret());
      setOk("连接成功：收件与发件服务器均验证通过");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy("");
    }
  }

  async function doSave() {
    const v = validate();
    if (v) return setError(v);
    setBusy("save");
    setError(null);
    try {
      const acc = await addAccount(buildAccount(), buildSecret());
      p.onAdded(acc);
    } catch (e) {
      setError(String(e));
      setBusy("");
    }
  }

  return (
    <div className="overlay">
      <div className="modal" style={{ width: 560 }}>
        <div className="modal-head">
          <span className="title">添加邮箱账户</span>
          <button className="modal-close" onClick={p.onClose}>
            ×
          </button>
        </div>
        <div className="modal-body" style={{ display: "flex", flexDirection: "column", gap: 14 }}>
          <div className="field">
            <label>邮箱服务商</label>
            <select className="select" value={presetKey} onChange={(e) => applyPreset(e.target.value)}>
              {PROVIDER_PRESETS.map((x) => (
                <option key={x.key} value={x.key}>
                  {x.label}
                </option>
              ))}
            </select>
            {preset.note && (
              <div style={{ fontSize: 11, color: "#9A5B16", lineHeight: 1.5 }}>ⓘ {preset.note}</div>
            )}
          </div>

          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
            <div className="field">
              <label>邮箱地址</label>
              <input className="input mono" placeholder="you@company.com" value={email} onChange={(e) => setEmail(e.target.value)} />
            </div>
            <div className="field">
              <label>显示名（发件人姓名）</label>
              <input className="input" placeholder="你的名字" value={displayName} onChange={(e) => setDisplayName(e.target.value)} />
            </div>
          </div>

          {preset.oauth && (
            <div className="field">
              <label>认证方式</label>
              <select
                className="select"
                value={authMode}
                onChange={(e) => {
                  setAuthMode(e.target.value as "password" | "oauth2");
                  cancelDeviceFlow();
                  setError(null);
                  setOk(null);
                }}
              >
                <option value="oauth2">OAuth2 授权登录（微软现已强制，推荐）</option>
                <option value="password">密码 / 应用密码（多数租户已被微软禁用）</option>
              </select>
            </div>
          )}

          {authMode === "oauth2" ? (
            <div
              className="field"
              style={{ border: "1px solid #E4DFD3", borderRadius: 10, padding: 14, gap: 10, display: "flex", flexDirection: "column" }}
            >
              {tokens ? (
                <>
                  <div className="form-ok">✓ 已获得 Microsoft 授权（令牌只保存在本机）</div>
                  <button className="btn-ghost" style={{ height: 34, alignSelf: "flex-start" }} onClick={startDeviceFlow}>
                    重新授权
                  </button>
                </>
              ) : device ? (
                <>
                  <div style={{ fontSize: 12, color: "#6F6A5E" }}>
                    已在浏览器打开 Microsoft 登录页面，请输入以下代码并用 <b>{email || "你的邮箱"}</b> 登录：
                  </div>
                  <div
                    className="mono"
                    style={{ fontSize: 26, letterSpacing: 4, fontWeight: 700, textAlign: "center", padding: "6px 0", userSelect: "all" }}
                  >
                    {device.userCode}
                  </div>
                  <div style={{ fontSize: 12, color: "#6F6A5E", textAlign: "center" }}>正在等待授权完成…</div>
                  <div style={{ display: "flex", gap: 8, justifyContent: "center" }}>
                    <button className="btn-ghost" style={{ height: 34 }} onClick={() => openUrl(device.verificationUri)}>
                      重新打开登录页面
                    </button>
                    <button className="btn-ghost" style={{ height: 34 }} onClick={cancelDeviceFlow}>
                      取消
                    </button>
                  </div>
                </>
              ) : (
                <>
                  <button className="btn-primary" style={{ height: 40 }} onClick={startDeviceFlow}>
                    用 Microsoft 账户授权
                  </button>
                  <div style={{ fontSize: 11, color: "#A39E91", lineHeight: 1.5 }}>
                    将打开浏览器，在 microsoft.com/devicelogin 输入代码完成登录。组织若禁止第三方应用，可在下方填入自己注册的
                    Azure 应用 Client ID。
                  </div>
                  <input
                    className="input mono"
                    placeholder="Client ID（可留空，默认使用通用邮件客户端 ID）"
                    value={clientId}
                    onChange={(e) => setClientId(e.target.value)}
                  />
                </>
              )}
            </div>
          ) : (
            <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
              <div className="field">
                <label>登录用户名（默认同邮箱）</label>
                <input className="input mono" placeholder={email || "可留空"} value={username} onChange={(e) => setUsername(e.target.value)} />
              </div>
              <div className="field">
                <label>密码 / 授权码 / 应用密码</label>
                <input className="input mono" type="password" value={password} onChange={(e) => setPassword(e.target.value)} />
              </div>
            </div>
          )}

          <div style={{ display: "grid", gridTemplateColumns: "2fr 1fr", gap: 12 }}>
            <div className="field">
              <label>收件服务器（{preset.protocol === "imap" ? "IMAP · SSL" : "POP3 · SSL"}）</label>
              <input className="input mono" value={incomingHost} onChange={(e) => setIncomingHost(e.target.value)} />
            </div>
            <div className="field">
              <label>端口</label>
              <input
                className="input mono"
                type="number"
                value={incomingPort}
                onChange={(e) => setIncomingPort(Number(e.target.value) || 0)}
              />
            </div>
          </div>

          <div style={{ display: "grid", gridTemplateColumns: "2fr 1fr 1fr", gap: 12 }}>
            <div className="field">
              <label>发件服务器（SMTP）</label>
              <input className="input mono" value={smtpHost} onChange={(e) => setSmtpHost(e.target.value)} />
            </div>
            <div className="field">
              <label>端口</label>
              <input
                className="input mono"
                type="number"
                value={smtpPort}
                onChange={(e) => setSmtpPort(Number(e.target.value) || 0)}
              />
            </div>
            <div className="field">
              <label>加密</label>
              <select className="select" value={smtpSecurity} onChange={(e) => setSmtpSecurity(e.target.value as "ssl" | "starttls")}>
                <option value="ssl">SSL</option>
                <option value="starttls">STARTTLS</option>
              </select>
            </div>
          </div>

          {error && <div className="form-error">{error}</div>}
          {ok && <div className="form-ok">{ok}</div>}

          <div style={{ fontSize: 11, color: "#A39E91", lineHeight: 1.6 }}>
            密码与 OAuth 令牌只保存在本机（应用配置目录，权限 600），不会上传。Exchange Online / Outlook.com
            已被微软强制使用 OAuth2，请选择「用 Microsoft 账户授权」。
          </div>
        </div>
        <div className="modal-foot">
          <button className="btn-ghost" style={{ height: 40 }} disabled={!!busy} onClick={doTest}>
            {busy === "test" ? "正在测试…" : "测试连接"}
          </button>
          <button className="btn-primary" style={{ height: 40, padding: "0 22px" }} disabled={!!busy} onClick={doSave}>
            {busy === "save" ? "正在验证并保存…" : "保存账户"}
          </button>
        </div>
      </div>
    </div>
  );
}
