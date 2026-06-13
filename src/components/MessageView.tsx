import { Fragment, useEffect, useState } from "react";
import { save as saveFileDialog } from "@tauri-apps/plugin-dialog";
import { AppIcon } from "./AppIcon";
import { HtmlBody } from "./HtmlBody";
import { Seal } from "./Seal";
import { TextBody } from "./TextBody";
import { saveAttachment } from "../api";
import { buildChecks, riskBanner, statusText, TONE_COLOR } from "../trust";
import type { EmailFull, EmailMeta, FolderInfo } from "../types";

interface Props {
  mail: EmailFull | null;
  thread: EmailMeta[];
  folders: FolderInfo[];
  onOpenThreadMail: (mail: EmailMeta) => void;
  onReply: () => void;
  onReplyAll: () => void;
  onForward: () => void;
  onMove: (target: string) => void;
  canMove: boolean;
  canArchive: boolean;
  onArchive: () => void;
  onDelete: () => void;
  onShowRisk: () => void;
  onTrustSender: () => void;
  onOpenProfile: () => void;
  onMarkUnread: () => void;
  onToggleFlag: () => void;
}

function fmtSize(n: number) {
  if (n > 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  if (n > 1024) return `${Math.round(n / 1024)} KB`;
  return `${n} B`;
}

function defaultShowHtml(bodyHtml: string | null | undefined) {
  return !!bodyHtml?.trim();
}

const RISKY_ATTACHMENT_EXTS = new Set([
  "apk",
  "app",
  "bat",
  "cmd",
  "com",
  "command",
  "desktop",
  "dmg",
  "docm",
  "exe",
  "hta",
  "iso",
  "jar",
  "js",
  "jse",
  "lnk",
  "msi",
  "pkg",
  "pptm",
  "ps1",
  "reg",
  "scpt",
  "scr",
  "sh",
  "terminal",
  "vbs",
  "workflow",
  "wsf",
  "xlsm",
]);

function attachmentWarning(name: string) {
  const normalized = name.replace(/[\u202a-\u202e\u2066-\u2069]/g, "");
  const parts = normalized.toLowerCase().split(".").filter(Boolean);
  const ext = parts.length > 0 ? parts[parts.length - 1] : "";
  const hasHiddenDirection = normalized !== name;
  const risky = RISKY_ATTACHMENT_EXTS.has(ext);
  const doubleExt = parts.length >= 3 && RISKY_ATTACHMENT_EXTS.has(ext);
  if (!risky && !doubleExt && !hasHiddenDirection) return null;
  const reasons = [];
  if (risky) reasons.push(`扩展名 .${ext} 可能是可执行文件`);
  if (doubleExt) reasons.push("文件名包含多重扩展名");
  if (hasHiddenDirection) reasons.push("文件名包含可能伪装扩展名的控制字符");
  return `这个附件有风险：${reasons.join("，")}。\n\n只在确认来源可信时保存。`;
}

export function MessageView(p: Props) {
  // 一键信任确认卡：换邮件时收起
  const [trustConfirm, setTrustConfirm] = useState(false);
  const [copied, setCopied] = useState(false);
  const [verifyOpen, setVerifyOpen] = useState(false);
  const [verifyPinned, setVerifyPinned] = useState(false);
  const [threadExpanded, setThreadExpanded] = useState(false);
  /** 正文视图：null=自动（未签名邮件优先 HTML；签名邮件显示被签名的纯文本） */
  const [htmlMode, setHtmlMode] = useState<boolean | null>(null);
  /** 附件下载状态：index → 状态文案 */
  const [attachState, setAttachState] = useState<Record<number, string>>({});
  const uid = p.mail?.meta.uid;
  useEffect(() => {
    setTrustConfirm(false);
    setCopied(false);
    setVerifyOpen(false);
    setVerifyPinned(false);
    setThreadExpanded(false);
    setHtmlMode(null);
    setAttachState({});
  }, [uid]);

  async function downloadAttachment(i: number, name: string) {
    if (!p.mail) return;
    const warning = attachmentWarning(name);
    if (warning && !window.confirm(warning)) return;
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
          <AppIcon className="empty-icon" alt="" />
          选择一封邮件查看内容与验证结果
        </div>
      </div>
    );
  }
  const m = p.mail;
  const status = statusText(m.verify);
  const checks = buildChecks(m);
  const unknownFpr = m.verify.status === "signedUnknown" ? m.verify.fingerprint : null;
  const banner = riskBanner(m);
  const moveTargets = p.folders.filter((f) => f.name !== m.meta.folder && f.name !== "__risk__");
  const canTrust = m.verify.status === "signedUnknown";

  return (
    <div className="msg-pane">
      <div className="msg-scroll">
        <div className="msg-head">
          <div className="msg-subject">{m.meta.subject}</div>
          <div className="msg-head2">
            <div className="msg-fromline">
              <div
                className="verify-trigger-wrap"
                onMouseEnter={() => setVerifyOpen(true)}
                onMouseLeave={() => !verifyPinned && setVerifyOpen(false)}
              >
                <button
                  className="verify-trigger"
                  title="查看验证详情"
                  onClick={() => {
                    setVerifyPinned((v) => !v);
                    setVerifyOpen(true);
                  }}
                >
                  <Seal trust={m.meta.trust} size={30} />
                </button>
                {verifyOpen && (
                  <div className="verify-popover">
                    <div className="verify-pop-head">
                      <Seal trust={m.meta.trust} size={42} />
                      <div style={{ minWidth: 0 }}>
                        <div className="verify-pop-title" style={{ color: TONE_COLOR[status.tone] }}>
                          {status.title}
                        </div>
                        <div className="verify-pop-sub">{status.sub}</div>
                      </div>
                    </div>
                    <div className="verify-pop-checks">
                      {checks.map((c, i) => (
                        <div className={`check ${c.kind}`} key={i}>
                          <div className="dot">{c.kind === "ok" ? "✓" : c.kind === "bad" ? "✕" : c.kind === "warn" ? "!" : "–"}</div>
                          <div style={{ minWidth: 0, flex: 1 }}>
                            <div className="label">{c.label}</div>
                            <div className={`val${c.mono ? " mono" : ""}`}>{c.val}</div>
                            {c.sub && <div className={`sub${c.mono ? " mono" : ""}`}>{c.sub}</div>}
                          </div>
                        </div>
                      ))}
                    </div>
                    <div className="verify-pop-actions">
                      {canTrust && (
                        <button className="btn-ghost" onClick={p.onTrustSender}>
                          加入可信联系人
                        </button>
                      )}
                      <button className="btn-ghost" onClick={p.onOpenProfile}>
                        发件人档案
                      </button>
                    </div>
                  </div>
                )}
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
                {p.canMove && (
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
                )}
                {p.canArchive && (
                  <button className="btn-ghost" onClick={p.onArchive} title="归档">
                    归档
                  </button>
                )}
                <button className="btn-ghost" onClick={p.onToggleFlag} title={m.meta.flagged ? "取消星标" : "加星标"}>
                  {m.meta.flagged ? "★ 已星标" : "☆ 星标"}
                </button>
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
          {m.attachments.length > 0 && (
            <div className="attach-row">
              {m.attachments.map((a, i) => (
                <div className="attach" key={i}>
                  <div className="ext">{(a.name.split(".").pop() || "?").toUpperCase().slice(0, 4)}</div>
                  <div className="attach-main">
                    <div className="name">{a.name}</div>
                    <div className="info">
                      {fmtSize(a.size)} · {a.mime}
                      {attachState[i] && <span className="attach-state">{attachState[i]}</span>}
                    </div>
                  </div>
                  <button className="btn-ghost attach-save" onClick={() => downloadAttachment(i, a.name)}>
                    保存
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>

        {p.thread.length > 1 && (
          <div className="thread-strip" aria-label="会话线程">
            <div className="thread-title">
              <span>会话</span>
              <span>{p.thread.length} 封</span>
            </div>
            <div className="thread-list">
              {(() => {
                const collapsed = p.thread.length > 3 && !threadExpanded;
                const visibleThread = collapsed ? [p.thread[0], ...p.thread.slice(-2)] : p.thread;
                const hiddenCount = p.thread.length - visibleThread.length;
                return visibleThread.map((item, index) => {
                  const showExpand = collapsed && index === 1;
                  const current = item.uid === m.meta.uid && item.folder === m.meta.folder;
                  return (
                    <Fragment key={`${item.accountId}/${item.folder}/${item.uid}`}>
                      {showExpand && (
                        <button className="thread-more" key="thread-more" onClick={() => setThreadExpanded(true)}>
                          展开中间 {hiddenCount} 封
                        </button>
                      )}
                      <button
                        key={`${item.accountId}/${item.folder}/${item.uid}`}
                        className={`thread-item${current ? " current" : ""}${item.unread ? " unread" : ""}`}
                        onClick={() => !current && p.onOpenThreadMail(item)}
                        disabled={current}
                      >
                        <span className="thread-dot" />
                        <span className="thread-main">
                          <span className="thread-from">{item.fromName}</span>
                          <span className="thread-preview">{item.preview || item.subject}</span>
                        </span>
                        <span className="thread-time">{item.dateDisplay}</span>
                      </button>
                    </Fragment>
                  );
                });
              })()}
            </div>
          </div>
        )}

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
              <button className="btn-solid" style={{ background: "var(--jade)" }} onClick={p.onTrustSender}>
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
          const showHtml = hasHtml && (htmlMode ?? defaultShowHtml(m.bodyHtml));
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
                <TextBody text={m.bodyText} />
              )}
            </>
          );
        })()}
      </div>
    </div>
  );
}
