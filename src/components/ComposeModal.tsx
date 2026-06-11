import { useEffect, useRef, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { AddrInput } from "./AddrInput";
import { Seal } from "./Seal";
import { deleteDraft, saveDraft, sendMail } from "../api";
import { shortFpr } from "../trust";
import type { Account, Draft, IdentityInfo, SendResult } from "../types";

export interface ComposePrefill {
  to?: string;
  cc?: string;
  subject?: string;
  body?: string;
}

interface Props {
  accounts: Account[];
  currentAccountId: string;
  identity: IdentityInfo | null;
  prefill?: ComposePrefill;
  /** 从草稿箱打开时传入 */
  draft?: Draft;
  onClose: () => void;
}

function shortAddr(addr: string) {
  return addr.length > 12 ? `${addr.slice(0, 6)}…${addr.slice(-4)}` : addr;
}

export function ComposeModal(p: Props) {
  const [accountId, setAccountId] = useState(p.draft?.accountId || p.currentAccountId);
  const [to, setTo] = useState(p.draft?.to ?? p.prefill?.to ?? "");
  const [cc, setCc] = useState(p.draft?.cc ?? p.prefill?.cc ?? "");
  const [subject, setSubject] = useState(p.draft?.subject ?? p.prefill?.subject ?? "");
  const [body, setBody] = useState(p.draft?.body ?? p.prefill?.body ?? "");
  const [sign, setSign] = useState(p.draft?.sign ?? true);
  /** 附件：本地文件绝对路径（发送时后端读取） */
  const [attachPaths, setAttachPaths] = useState<string[]>([]);
  const draftIdRef = useRef(p.draft?.id ?? "");

  async function pickAttachments() {
    const picked = await openFileDialog({ multiple: true, title: "选择附件" });
    if (!picked) return;
    const list = Array.isArray(picked) ? picked : [picked];
    setAttachPaths((prev) => [...prev, ...list.filter((x) => !prev.includes(x))]);
  }
  const [step, setStep] = useState(0); // 0 写 1 签名发送中 2 完成
  /** 撤销发送窗口：非 null 时正在倒计时，归零才真正发送 */
  const [countdown, setCountdown] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<SendResult | null>(null);

  const account = p.accounts.find((a) => a.id === accountId) ?? p.accounts[0];
  const isLedger = p.identity?.mode === "ledger";
  const idShort = isLedger
    ? shortAddr(p.identity?.ledgerAddress ?? "")
    : p.identity
      ? shortFpr(p.identity.fingerprint)
      : "…";

  const parseAddrs = (s: string) =>
    s
      .split(/[,;，；\s]+/)
      .map((x) => x.trim())
      .filter(Boolean);

  function startSend() {
    setError(null);
    if (parseAddrs(to).length === 0) {
      setError("请填写收件人地址");
      return;
    }
    if (!subject.trim()) {
      setError("请填写主题");
      return;
    }
    setCountdown(10);
  }

  async function doSend() {
    setStep(1);
    try {
      const r = await sendMail(account.id, parseAddrs(to), parseAddrs(cc), subject, body, sign, attachPaths);
      setResult(r);
      setStep(2);
      // 发送成功，草稿使命完成
      if (draftIdRef.current) {
        deleteDraft(draftIdRef.current).catch((e) => console.error("删除草稿失败", e));
        draftIdRef.current = "";
      }
    } catch (e) {
      setError(String(e));
      setStep(0);
    }
  }

  const hasContent = !!(to.trim() || cc.trim() || subject.trim() || body.trim());

  // 草稿自动保存（防抖 800ms；仅撰写阶段）
  useEffect(() => {
    if (step !== 0 || countdown !== null || !hasContent) return;
    const t = setTimeout(() => {
      saveDraft({ id: draftIdRef.current, accountId: account.id, to, cc, subject, body, sign, updatedAt: 0 })
        .then((d) => {
          draftIdRef.current = d.id;
        })
        .catch((e) => console.error("草稿保存失败", e));
    }, 800);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [to, cc, subject, body, sign, step, countdown]);

  function handleClose() {
    // 关闭前最后存一次，防抖窗口里的输入不丢
    if (step === 0 && hasContent) {
      saveDraft({ id: draftIdRef.current, accountId: account.id, to, cc, subject, body, sign, updatedAt: 0 }).catch(
        (e) => console.error("草稿保存失败", e)
      );
    }
    p.onClose();
  }

  // 倒计时归零才真正发送；期间可撤销
  useEffect(() => {
    if (countdown === null) return;
    if (countdown <= 0) {
      setCountdown(null);
      void doSend();
      return;
    }
    const t = setTimeout(() => setCountdown((c) => (c === null ? null : c - 1)), 1000);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [countdown]);

  const titles = ["写邮件 · 撰写", sign ? "写邮件 · 签名并发送" : "写邮件 · 发送中", "写邮件 · 完成"];

  return (
    <div className="overlay">
      <div className="modal" style={{ width: 640 }}>
        <div className="modal-head">
          <span className="title">{countdown !== null ? `写邮件 · ${countdown} 秒后发送` : titles[step]}</span>
          <button
            className="modal-close"
            onClick={() => {
              // 倒计时中点 × 先撤销发送，不直接关窗，防止误操作
              if (countdown !== null) setCountdown(null);
              else handleClose();
            }}
          >
            ×
          </button>
        </div>

        <div className="steps">
          {[0, 1, 2].map((i) => (
            <div key={i} className={`step${i <= step ? " on" : ""}`} />
          ))}
        </div>

        <div className="modal-body">
          {step === 0 && (
            <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
              {error && <div className="form-error">{error}</div>}
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 10,
                  border: "1px solid #E8E3D8",
                  borderRadius: 9,
                  padding: "0 14px",
                  height: 44,
                  background: "#fff",
                }}
              >
                <span style={{ fontSize: 11, fontWeight: 600, color: "#A39E91", width: 42, flexShrink: 0 }}>发自</span>
                {p.accounts.length > 1 ? (
                  <select
                    value={accountId}
                    onChange={(e) => setAccountId(e.target.value)}
                    style={{ border: "none", outline: "none", background: "transparent", fontFamily: "var(--mono)", fontSize: 12.5, color: "#2A2E36", flex: 1 }}
                  >
                    {p.accounts.map((a) => (
                      <option key={a.id} value={a.id}>
                        {a.email}
                      </option>
                    ))}
                  </select>
                ) : (
                  <span style={{ fontFamily: "var(--mono)", fontSize: 12.5, color: "#2A2E36" }}>{account?.email}</span>
                )}
                {sign && (
                  <span style={{ marginLeft: "auto", fontSize: 11, color: "#1E6B49", fontWeight: 600, whiteSpace: "nowrap" }}>
                    ● {isLedger ? "Ledger 签名" : "本地密钥签名"}
                  </span>
                )}
              </div>
              <AddrInput placeholder="收件人地址（多个用逗号分隔）" value={to} onChange={setTo} />
              <AddrInput placeholder="抄送（可选）" value={cc} onChange={setCc} />
              <input className="input" style={{ fontWeight: 500 }} placeholder="主题" value={subject} onChange={(e) => setSubject(e.target.value)} />
              <textarea className="textarea" style={{ minHeight: 180 }} placeholder="正文…" value={body} onChange={(e) => setBody(e.target.value)} />
              <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                <button className="btn-ghost" style={{ height: 30 }} onClick={pickAttachments}>
                  📎 添加附件
                </button>
                {attachPaths.map((path) => (
                  <span key={path} className="attach-chip" title={path}>
                    {path.split("/").pop()}
                    <button onClick={() => setAttachPaths((prev) => prev.filter((x) => x !== path))}>×</button>
                  </span>
                ))}
              </div>
              <label
                style={{
                  display: "flex",
                  alignItems: "center",
                  gap: 10,
                  fontSize: 12,
                  color: "#6E6A5F",
                  background: sign ? "#F1F6F2" : "#F4F1EA",
                  border: `1px solid ${sign ? "#DCE9DF" : "#E4DECF"}`,
                  borderRadius: 9,
                  padding: "12px 14px",
                  cursor: "pointer",
                }}
              >
                <input type="checkbox" checked={sign} onChange={(e) => setSign(e.target.checked)} style={{ accentColor: "#1E6B49" }} />
                <span style={{ fontSize: 15 }}>🔒</span>
                <span>
                  {isLedger ? (
                    <>
                      用 <b style={{ color: "#1E6B49" }}>Ledger 硬件密钥（{idShort}）</b>签名，发送时需在设备上确认。
                    </>
                  ) : (
                    <>
                      用本机 <b style={{ color: "#1E6B49" }}>SealMail 密钥（Ed25519 · {idShort}）</b>签名。
                    </>
                  )}
                  装有 SealMail 的收件人会看到完整封印；普通邮箱收件人只会在结尾看到一行低调的签名说明，不影响阅读。
                </span>
              </label>
            </div>
          )}

          {step === 1 && (
            <div style={{ display: "flex", flexDirection: "column", alignItems: "center", textAlign: "center", padding: "18px 0" }}>
              <div className="device">
                <div className="screen">
                  <span>
                    {sign ? "Sign message?" : "Sending…"}
                    <br />
                    {idShort}
                  </span>
                </div>
                <div className="btn" />
              </div>
              <div style={{ fontSize: 15, fontWeight: 700, color: "#23272F", marginTop: 22 }}>
                {sign && isLedger ? "在你的 Ledger 上确认签名" : sign ? "正在签名并发送" : "正在发送"}
              </div>
              <div style={{ fontSize: 12.5, color: "#8A8576", marginTop: 6, maxWidth: 360, lineHeight: 1.6 }}>
                {sign && isLedger
                  ? "核对设备屏幕上的内容摘要，按下两侧按钮确认。私钥永不离开硬件。"
                  : sign
                    ? "正文哈希已计算，正用你的本地密钥盖印，随后通过 SMTP 投递。私钥不会离开本机。"
                    : "正在通过 SMTP 投递。"}
              </div>
              <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 18, color: "#9A958A", fontSize: 12 }}>
                <span className="pulse-dot" /> {sign && isLedger ? "等待硬件确认…" : "正在投递…"}
              </div>
            </div>
          )}

          {step === 2 && result && (
            <div style={{ display: "flex", flexDirection: "column", alignItems: "center", textAlign: "center", padding: "14px 0" }}>
              <Seal trust="verified" size={104} />
              <div style={{ fontSize: 17, fontWeight: 700, color: "#1E6B49", marginTop: 18 }}>
                {result.signed ? "已签名并发送" : "已发送（未签名）"}
              </div>
              <div style={{ fontSize: 12.5, color: "#8A8576", marginTop: 7, maxWidth: 380, lineHeight: 1.6 }}>
                {result.signed
                  ? "收件人若使用 SealMail，将看到这枚完整封印——证明邮件确实出自你的密钥，且内容未被改动。普通邮箱则正常收信。"
                  : "邮件已通过 SMTP 正常发出。"}
              </div>
              <div
                style={{
                  marginTop: 18,
                  fontFamily: "var(--mono)",
                  fontSize: 11,
                  color: "#8A8576",
                  background: "#F1EDE3",
                  borderRadius: 8,
                  padding: "10px 14px",
                }}
              >
                {result.signed ? `sig ${result.shortFingerprint} · ${result.method} · ${result.sentAt}` : `sent · ${result.sentAt}`}
              </div>
            </div>
          )}
        </div>

        <div className="modal-foot">
          <span className="toolbar-note">
            {countdown !== null && (
              <span style={{ color: "#9A5B16", fontSize: 12, fontWeight: 600 }}>
                ⏳ {countdown} 秒后发送，反悔还来得及
              </span>
            )}
          </span>
          {step === 0 && countdown === null && (
            <button className="btn-primary" style={{ height: 40, padding: "0 22px" }} onClick={startSend}>
              {sign ? (isLedger ? "用 Ledger 签名并发送" : "签名并发送") : "发送"}
            </button>
          )}
          {step === 0 && countdown !== null && (
            <div style={{ display: "flex", gap: 9 }}>
              <button
                className="btn-ghost"
                style={{ height: 40, padding: "0 18px", borderColor: "#C99B4E", color: "#9A5B16", fontWeight: 700 }}
                onClick={() => setCountdown(null)}
              >
                ↩ 撤销发送
              </button>
              <button className="btn-primary" style={{ height: 40, padding: "0 18px" }} onClick={() => setCountdown(0)}>
                立即发送
              </button>
            </div>
          )}
          {step === 1 && (
            <button className="btn-ghost" style={{ height: 40 }} disabled>
              发送中…
            </button>
          )}
          {step === 2 && (
            <button className="btn-primary" style={{ height: 40, padding: "0 22px" }} onClick={p.onClose}>
              完成
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
