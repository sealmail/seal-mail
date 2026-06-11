import { useState } from "react";
import { addAccount, testConnection } from "../api";
import { PROVIDER_PRESETS } from "../types";
import type { Account } from "../types";

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

  function applyPreset(key: string) {
    const pr = PROVIDER_PRESETS.find((x) => x.key === key)!;
    setPresetKey(key);
    setIncomingHost(pr.incomingHost);
    setIncomingPort(pr.incomingPort);
    setSmtpHost(pr.smtpHost);
    setSmtpPort(pr.smtpPort);
    setSmtpSecurity(pr.smtpSecurity);
    setOk(null);
    setError(null);
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
    };
  }

  function validate(): string | null {
    if (!email.includes("@")) return "请填写正确的邮箱地址";
    if (!password) return "请填写密码 / 授权码";
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
      await testConnection(buildAccount(), { password });
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
      const acc = await addAccount(buildAccount(), { password });
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
            密码只保存在本机（应用配置目录，权限 600），不会上传。Exchange Online 用户：管理员需启用 IMAP 与
            SMTP AUTH，个人账户建议使用应用密码。
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
