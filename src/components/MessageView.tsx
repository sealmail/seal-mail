import { useEffect, useState } from "react";
import { save as saveFileDialog } from "@tauri-apps/plugin-dialog";
import { HtmlBody } from "./HtmlBody";
import { Seal } from "./Seal";
import { saveAttachment } from "../api";
import { riskBanner } from "../trust";
import type { EmailFull, FolderInfo } from "../types";

interface Props {
  mail: EmailFull | null;
  folders: FolderInfo[];
  onReply: () => void;
  onReplyAll: () => void;
  onForward: () => void;
  onMove: (target: string) => void;
  onDelete: () => void;
  onShowRisk: () => void;
  onTrustSender: () => void;
  onMarkUnread: () => void;
}

function fmtSize(n: number) {
  if (n > 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  if (n > 1024) return `${Math.round(n / 1024)} KB`;
  return `${n} B`;
}

export function MessageView(p: Props) {
  // 一键信任确认卡：换邮件时收起
  const [trustConfirm, setTrustConfirm] = useState(false);
  const [copied, setCopied] = useState(false);
  /** 正文视图：null=自动（未签名邮件优先 HTML；签名邮件显示被签名的纯文本） */
  const [htmlMode, setHtmlMode] = useState<boolean | null>(null);
  /** 附件下载状态：index → 状态文案 */
  const [attachState, setAttachState] = useState<Record<number, string>>({});
  const uid = p.mail?.meta.uid;
  useEffect(() => {
    setTrustConfirm(false);
    setCopied(false);
    setHtmlMode(null);
    setAttachState({});
  }, [uid]);

  async function downloadAttachment(i: number, name: string) {
    if (!p.mail) return;
    const path = await saveFileDialog({ defaultPath: name, title: "保存附件" });
    if (!path) return;
    setAttachState((s) => ({ ...s, [i]: "保存中…" }));
    try {
      await saveAttachment(p.mail.meta.accountId, p.mail.meta.folder, p.mail.meta.uid, i, path);
      setAttachState((s) => ({ ...s, [i]: "已保存 ✓" }));
    } catch (e) {
      setAttachState((s) => ({ ...s, [i]: `失败：${e}` }));
    }
  }

  if (!p.mail) {
    return (
      <div className="msg-pane">
        <div className="empty-pane">
          <div style={{ fontSize: 26, color: "#C7C1B2" }}>印</div>
          选择一封邮件查看内容与验证结果
        </div>
      </div>
    );
  }
  const m = p.mail;
  const unknownFpr = m.verify.status === "signedUnknown" ? m.verify.fingerprint : null;
  const banner = riskBanner(m);
  const moveTargets = p.folders.filter((f) => f.name !== m.meta.folder && f.name !== "__risk__");

  return (
    <div className="msg-pane">
      <div className="msg-scroll">
        <div className="msg-head">
          <div className="msg-subject">{m.meta.subject}</div>
          <div className="msg-head2">
            <div className="msg-fromline">
              <div style={{ paddingTop: 1 }}>
                <Seal trust={m.meta.trust} size={30} />
              </div>
              <div style={{ minWidth: 0 }}>
                <div className="msg-fromname">{m.meta.fromName}</div>
                <div className="msg-addr">{m.meta.fromAddr}</div>
                {m.verify.status === "signedUnknown" && !trustConfirm && (
                  <button className="trust-chip" onClick={() => setTrustConfirm(true)}>
                    ✓ 信任此发件人
                  </button>
                )}
              </div>
            </div>
            <div className="msg-side">
              <span className="msg-date">{m.meta.dateDisplay}</span>
              <div style={{ display: "flex", gap: 7, flexWrap: "wrap", justifyContent: "flex-end" }}>
                <button className="btn-ghost" onClick={p.onReply}>
                  回复
                </button>
                <button className="btn-ghost" onClick={p.onReplyAll}>
                  回复全部
                </button>
                <button className="btn-ghost" onClick={p.onForward}>
                  转发
                </button>
                <select
                  className="btn-ghost"
                  style={{ paddingRight: 6, maxWidth: 104 }}
                  value=""
                  onChange={(e) => e.target.value && p.onMove(e.target.value)}
                >
                  <option value="">移动到…</option>
                  {moveTargets.map((f) => (
                    <option key={f.name} value={f.name}>
                      {f.display}
                    </option>
                  ))}
                </select>
                <button className="btn-ghost" onClick={p.onMarkUnread} title="标为未读">
                  标为未读
                </button>
                <button className="btn-ghost" onClick={p.onDelete} title="删除">
                  删除
                </button>
              </div>
            </div>
          </div>
          {(m.to.length > 0 || m.cc.length > 0) && (
            <div className="msg-rcpts">
              {m.to.length > 0 && (
                <div className="rcpt-line">
                  <span className="rcpt-label">收件人</span>
                  <span className="rcpt-list">{m.to.join("、")}</span>
                </div>
              )}
              {m.cc.length > 0 && (
                <div className="rcpt-line">
                  <span className="rcpt-label">抄送</span>
                  <span className="rcpt-list">{m.cc.join("、")}</span>
                </div>
              )}
            </div>
          )}
        </div>

        {trustConfirm && unknownFpr && (
          <div className="trust-confirm">
            <div className="title">
              将 {m.meta.fromName} 加入可信联系人？
            </div>
            <div className="row">
              <span className="mono">{m.meta.fromAddr}</span>
            </div>
            <div className="row">
              <span className="mono">指纹 {unknownFpr}</span>
              <button
                className="btn-ghost"
                style={{ height: 24, padding: "0 8px", fontSize: 11 }}
                onClick={() => {
                  navigator.clipboard.writeText(unknownFpr);
                  setCopied(true);
                }}
              >
                {copied ? "已复制" : "复制指纹"}
              </button>
            </div>
            <div className="msg">
              签名只能证明「这封信出自这把密钥、内容未被改动」，并不能证明密钥背后就是 TA
              本人。建议先通过微信、电话等<b>邮件以外的渠道</b>与对方核对一遍指纹再确认。
              确认后，今后凡是这把密钥签名的邮件都会直接显示绿色「已验证本人」；若有人换用别的密钥冒充这个地址，SealMail
              会立即标红警告。
            </div>
            <div className="actions">
              <button className="btn-solid" style={{ background: "#1E6B49" }} onClick={p.onTrustSender}>
                ✓ 确认信任
              </button>
              <button className="btn-ghost" onClick={() => setTrustConfirm(false)}>
                取消
              </button>
            </div>
          </div>
        )}

        {banner && (
          <div className={`risk-banner ${banner.cls}`}>
            <div className="icon">{banner.icon}</div>
            <div style={{ flex: 1 }}>
              <div className="title">{banner.title}</div>
              <div className="msg">{banner.msg}</div>
              <div className="actions">
                <button className="btn-solid" style={{ background: banner.solid }} onClick={p.onShowRisk}>
                  {banner.btn}
                </button>
              </div>
            </div>
          </div>
        )}

        {(() => {
          const hasHtml = !!m.bodyHtml;
          const signed = m.verify.status !== "unsigned";
          const showHtml = hasHtml && (htmlMode ?? !signed);
          return (
            <>
              {hasHtml && (
                <div className="body-toolbar">
                  {signed && showHtml && (
                    <span className="body-note">⚠ 签名校验针对纯文本正文，HTML 版式仅供参考</span>
                  )}
                  <button className="btn-ghost" style={{ height: 24, padding: "0 10px", fontSize: 11 }} onClick={() => setHtmlMode(!showHtml)}>
                    {showHtml ? "查看纯文本" : "查看 HTML 版式"}
                  </button>
                </div>
              )}
              {showHtml ? (
                <HtmlBody html={m.bodyHtml as string} />
              ) : (
                <div className="msg-body">{m.bodyText || "(无正文)"}</div>
              )}
            </>
          );
        })()}

        {m.attachments.length > 0 && (
          <div className="attach-row">
            {m.attachments.map((a, i) => (
              <div className="attach" key={i}>
                <div className="ext">{(a.name.split(".").pop() || "?").toUpperCase().slice(0, 4)}</div>
                <div>
                  <div className="name">{a.name}</div>
                  <div className="info">
                    {fmtSize(a.size)} · {a.mime}
                    {attachState[i] && <span style={{ marginLeft: 6 }}>{attachState[i]}</span>}
                  </div>
                </div>
                <button
                  className="btn-ghost"
                  style={{ height: 26, padding: "0 10px", fontSize: 11 }}
                  onClick={() => downloadAttachment(i, a.name)}
                >
                  保存
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
