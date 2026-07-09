import { t, useI18n } from "../i18n";
import { useEffect, useRef, useState } from "react";
import { ask as askDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";
import { downloadDir, join } from "@tauri-apps/api/path";
import { AppIcon } from "./AppIcon";
import { HtmlBody } from "./HtmlBody";
import { Seal } from "./Seal";
import { TextBody } from "./TextBody";
import { readAttachment, saveAttachment } from "../api";
import { buildChecks, riskBanner, statusText, TONE_COLOR } from "../trust";
import type { AttachmentMeta, EmailFull, EmailMeta, FolderInfo } from "../types";

interface Props {
  mail: EmailFull | null;
  thread: EmailMeta[];
  /** 会话正文缓存（mailKey → 全文）；没有正文的邮件渲染占位卡片，点击再加载 */
  threadFulls: Record<string, EmailFull>;
  folders: FolderInfo[];
  onOpenThreadMail: (mail: EmailMeta) => void;
  onLoadThreadMail: (mail: EmailMeta) => void;
  onReply: () => void;
  onReplyAll: () => void;
  onForward: () => void;
  onMove: (target: string) => void;
  canMove: boolean;
  canArchive: boolean;
  onArchive: () => void;
  onDelete: () => void;
  onShowRisk: () => void;
  /** 用户已在风险弹窗里勾选确认：红色横幅收起为一行低调提示 */
  riskAcked: boolean;
  onTrustSender: () => void;
  onOpenProfile: () => void;
  onMarkUnread: () => void;
  onToggleFlag: () => void;
  onBlockSender: () => void;
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
  if (risky) reasons.push(t("扩展名 .{ext} 可能是可执行文件", { ext }));
  if (doubleExt) reasons.push(t("文件名包含多重扩展名"));
  if (hasHiddenDirection) reasons.push(t("文件名包含可能伪装扩展名的控制字符"));
  return t("这个附件有风险：{reasons}。", { reasons: reasons.join(t("，")) }) + "\n\n" + t("只在确认来源可信时保存。");
}

const IMAGE_EXTS = new Set(["jpg", "jpeg", "png", "gif", "webp", "bmp", "svg", "heic", "heif", "avif", "tif", "tiff"]);

function isImageAttachment(a: AttachmentMeta): boolean {
  const mime = a.mime.toLowerCase();
  if (mime.startsWith("image/")) return true;
  const ext = a.name.split(".").pop()?.toLowerCase() ?? "";
  return IMAGE_EXTS.has(ext);
}

interface ImagePreviewState {
  name: string;
  src: string | null;
  error: string | null;
}

export function MessageView(p: Props) {
  useI18n();
  // 一键信任确认卡：换邮件时收起
  const [trustConfirm, setTrustConfirm] = useState(false);
  const [copied, setCopied] = useState(false);
  const [verifyOpen, setVerifyOpen] = useState(false);
  const [verifyPinned, setVerifyPinned] = useState(false);
  /** 正文视图：null=自动（未签名邮件优先 HTML；签名邮件显示被签名的纯文本） */
  const [htmlMode, setHtmlMode] = useState<boolean | null>(null);
  const [threadHtmlModes, setThreadHtmlModes] = useState<Record<string, boolean | null>>({});
  /** 附件下载状态：mail/index → 状态文案 */
  const [attachState, setAttachState] = useState<Record<string, string>>({});
  /** 图片附件预览（lightbox） */
  const [imagePreview, setImagePreview] = useState<ImagePreviewState | null>(null);
  const selectedCardRef = useRef<HTMLDivElement | null>(null);
  const uid = p.mail?.meta.uid;
  useEffect(() => {
    setTrustConfirm(false);
    setCopied(false);
    setVerifyOpen(false);
    setVerifyPinned(false);
    setHtmlMode(null);
    setThreadHtmlModes({});
    setAttachState({});
    setImagePreview(null);
  }, [uid]);

  useEffect(() => {
    if (p.thread.length > 1) {
      selectedCardRef.current?.scrollIntoView({ block: "start" });
    }
  }, [uid, p.thread.length]);

  useEffect(() => {
    if (!imagePreview) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") setImagePreview(null);
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [imagePreview]);

  async function downloadAttachment(mail: EmailFull, i: number, name: string) {
    const warning = attachmentWarning(name);
    // WKWebView 里 window.confirm 是 no-op（静默返回 false），必须走 dialog 插件
    if (warning && !(await askDialog(warning, { title: t("危险附件"), kind: "warning", okLabel: t("继续保存"), cancelLabel: t("取消") }))) return;
    // 默认落到系统「下载」目录，而不是「文稿」
    const defaultPath = await join(await downloadDir(), name);
    const path = await saveFileDialog({ defaultPath, title: t("保存附件") });
    if (!path) return;
    const stateKey = `${mail.meta.accountId}/${mail.meta.folder}/${mail.meta.uid}/${i}`;
    setAttachState((s) => ({ ...s, [stateKey]: t("保存中…") }));
    try {
      await saveAttachment(mail.meta.accountId, mail.meta.folder, mail.meta.uid, i, path);
      setAttachState((s) => ({ ...s, [stateKey]: t("已保存 ✓") }));
    } catch (e) {
      setAttachState((s) => ({ ...s, [stateKey]: t("失败：") + e }));
    }
  }

  async function previewImageAttachment(mail: EmailFull, i: number, a: AttachmentMeta) {
    setImagePreview({ name: a.name, src: null, error: null });
    try {
      const data = await readAttachment(mail.meta.accountId, mail.meta.folder, mail.meta.uid, i);
      const mime = data.mime.startsWith("image/") ? data.mime : a.mime.startsWith("image/") ? a.mime : "image/jpeg";
      setImagePreview({ name: data.filename || a.name, src: `data:${mime};base64,${data.dataBase64}`, error: null });
    } catch (e) {
      setImagePreview({ name: a.name, src: null, error: t("预览失败：") + e });
    }
  }

  function renderAttachment(mail: EmailFull, a: AttachmentMeta, i: number) {
    const stateKey = `${mail.meta.accountId}/${mail.meta.folder}/${mail.meta.uid}/${i}`;
    const image = isImageAttachment(a);
    return (
      <div
        className={`attach${image ? " attach-image" : ""}`}
        key={i}
        role={image ? "button" : undefined}
        tabIndex={image ? 0 : undefined}
        title={image ? t("点击预览图片") : undefined}
        onClick={image ? () => previewImageAttachment(mail, i, a) : undefined}
        onKeyDown={
          image
            ? (e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  previewImageAttachment(mail, i, a);
                }
              }
            : undefined
        }
      >
        <div className="ext">{(a.name.split(".").pop() || "?").toUpperCase().slice(0, 4)}</div>
        <div className="attach-main">
          <div className="name">{a.name}</div>
          <div className="info">
            {fmtSize(a.size)} · {a.mime}
            {image && <span className="attach-preview-hint">{t("点击预览")}</span>}
            {attachState[stateKey] && <span className="attach-state">{attachState[stateKey]}</span>}
          </div>
        </div>
        <button
          className="btn-ghost attach-save"
          onClick={(e) => {
            e.stopPropagation();
            downloadAttachment(mail, i, a.name);
          }}
        >
          {t("保存")}
        </button>
      </div>
    );
  }

  if (!p.mail) {
    return (
      <div className="msg-pane">
        <div className="empty-pane">
          <AppIcon className="empty-icon" alt="" />
          {t("选择一封邮件查看内容与验证结果")}
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
  const conversation = p.thread.length > 1 ? p.thread : [];

  function renderBody(mail: EmailFull, mode: boolean | null, setMode: (next: boolean) => void) {
    const hasHtml = !!mail.bodyHtml;
    const signed = mail.verify.status !== "unsigned";
    const showHtml = hasHtml && (mode ?? defaultShowHtml(mail.bodyHtml));
    return (
      <>
        {hasHtml && (
          <div className="body-toolbar">
            {signed && showHtml && <span className="body-note">⚠ {t("签名校验针对纯文本正文，HTML 版式仅供参考")}</span>}
            <button className="btn-ghost" style={{ height: 24, padding: "0 10px", fontSize: 11 }} onClick={() => setMode(!showHtml)}>
              {showHtml ? t("查看纯文本") : t("查看 HTML 版式")}
            </button>
          </div>
        )}
        {showHtml ? <HtmlBody html={mail.bodyHtml as string} /> : <TextBody text={mail.bodyText} />}
      </>
    );
  }

  function renderThreadCard(meta: EmailMeta) {
    const key = `${meta.accountId}/${meta.folder}/${meta.uid}`;
    const current = key === `${m.meta.accountId}/${m.meta.folder}/${m.meta.uid}`;
    const mail = current ? m : p.threadFulls[key];
    // 正文未加载（长会话中间的邮件）：占位卡片，点击按需加载
    if (!mail) {
      return (
        <div
          className="thread-card thread-card-stub"
          key={key}
          onClick={() => p.onLoadThreadMail(meta)}
          title={t("点击加载这封邮件的正文")}
        >
          <div className="thread-card-head">
            <div className="msg-fromline">
              <Seal trust={meta.trust} size={30} />
              <div style={{ minWidth: 0 }}>
                <div className="msg-fromname">{meta.fromName}</div>
                <div className="msg-addr">{meta.fromAddr}</div>
              </div>
            </div>
            <span className="msg-date">{meta.dateDisplay}</span>
          </div>
          <div className="thread-stub-preview">{meta.preview || "…"}</div>
          <button className="btn-ghost thread-stub-load" onClick={(e) => { e.stopPropagation(); p.onLoadThreadMail(meta); }}>
            {t("展开正文")}
          </button>
        </div>
      );
    }
    const mode = threadHtmlModes[key] ?? null;
    return (
      <div className={`thread-card${current ? " current" : ""}`} key={key} ref={current ? selectedCardRef : null}>
        <div className="thread-card-head">
          <div className="msg-fromline">
            <Seal trust={mail.meta.trust} size={30} />
            <div style={{ minWidth: 0 }}>
              <div className="msg-fromname">{mail.meta.fromName}</div>
              <div className="msg-addr">{mail.meta.fromAddr}</div>
            </div>
          </div>
          <span className="msg-date">{mail.meta.dateDisplay}</span>
        </div>
        {(mail.to.length > 0 || mail.cc.length > 0) && (
          <div className="msg-rcpts">
            {mail.to.length > 0 && (
              <div className="rcpt-line">
                <span className="rcpt-label">{t("收件人")}</span>
                <span className="rcpt-list">{mail.to.join("、")}</span>
              </div>
            )}
            {mail.cc.length > 0 && (
              <div className="rcpt-line">
                <span className="rcpt-label">{t("抄送")}</span>
                <span className="rcpt-list">{mail.cc.join("、")}</span>
              </div>
            )}
          </div>
        )}
        {mail.attachments.length > 0 && (
          <div className="attach-row">{mail.attachments.map((a, i) => renderAttachment(mail, a, i))}</div>
        )}
        <div className="thread-card-body">
          {renderBody(mail, mode, (next) => setThreadHtmlModes((s) => ({ ...s, [key]: next })))}
        </div>
      </div>
    );
  }

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
                  title={t("查看验证详情")}
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
                          {t("加入可信联系人")}
                        </button>
                      )}
                      <button className="btn-ghost" onClick={p.onOpenProfile}>
                        {t("发件人档案")}
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
                    ✓ {t("信任此发件人")}
                  </button>
                )}
              </div>
            </div>
            <div className="msg-side">
              <span className="msg-date">{m.meta.dateDisplay}</span>
              <div className="msg-actions">
                <button className="btn-ghost" onClick={p.onReply}>
                  {t("回复")}
                </button>
                <button className="btn-ghost" onClick={p.onReplyAll}>
                  {t("回复全部")}
                </button>
                <button className="btn-ghost" onClick={p.onForward}>
                  {t("转发")}
                </button>
                {p.canMove && (
                  <select
                    className="btn-ghost"
                    style={{ paddingRight: 6, maxWidth: 104 }}
                    value=""
                    onChange={(e) => e.target.value && p.onMove(e.target.value)}
                  >
                    <option value="">{t("移动到…")}</option>
                    {moveTargets.map((f) => (
                      <option key={f.name} value={f.name}>
                        {t(f.display)}
                      </option>
                    ))}
                  </select>
                )}
                {p.canArchive && (
                  <button className="btn-ghost" onClick={p.onArchive} title={t("归档")}>
                    {t("归档")}
                  </button>
                )}
                <button className="btn-ghost" onClick={p.onToggleFlag} title={m.meta.flagged ? t("取消星标") : t("加星标")}>
                  {m.meta.flagged ? t("★ 已星标") : t("☆ 星标")}
                </button>
                <button className="btn-ghost" onClick={p.onMarkUnread} title={t("标为未读")}>
                  {t("标为未读")}
                </button>
                <button className="btn-ghost" onClick={p.onBlockSender} title={t("后续来自该邮箱的邮件移入垃圾邮件")}>
                  {t("屏蔽发件人")}
                </button>
                <button className="btn-ghost" onClick={p.onDelete} title={t("删除")}>
                  {t("删除")}
                </button>
              </div>
            </div>
          </div>
          {conversation.length === 0 && (m.to.length > 0 || m.cc.length > 0) && (
            <div className="msg-rcpts">
              {m.to.length > 0 && (
                <div className="rcpt-line">
                  <span className="rcpt-label">{t("收件人")}</span>
                  <span className="rcpt-list">{m.to.join("、")}</span>
                </div>
              )}
              {m.cc.length > 0 && (
                <div className="rcpt-line">
                  <span className="rcpt-label">{t("抄送")}</span>
                  <span className="rcpt-list">{m.cc.join("、")}</span>
                </div>
              )}
            </div>
          )}
          {conversation.length === 0 && m.attachments.length > 0 && (
            <div className="attach-row">{m.attachments.map((a, i) => renderAttachment(m, a, i))}</div>
          )}
        </div>

        {trustConfirm && unknownFpr && (
          <div className="trust-confirm">
            <div className="title">
              {t("将 {name} 加入可信联系人？", { name: m.meta.fromName })}
            </div>
            <div className="row">
              <span className="mono">{m.meta.fromAddr}</span>
            </div>
            <div className="row">
              <span className="mono">{t("指纹")} {unknownFpr}</span>
              <button
                className="btn-ghost"
                style={{ height: 24, padding: "0 8px", fontSize: 11 }}
                onClick={() => {
                  navigator.clipboard.writeText(unknownFpr);
                  setCopied(true);
                }}
              >
                {copied ? t("已复制") : t("复制指纹")}
              </button>
            </div>
            <div className="msg">
              {t("签名只能证明「这封信出自这把密钥、内容未被改动」，并不能证明密钥背后就是 TA 本人。建议先通过")}<b>{t("邮件以外的渠道")}</b>{t("（微信、电话等）与对方核对一遍指纹再确认。确认后，今后凡是这把密钥签名的邮件都会直接显示绿色「已验证本人」；若有人换用别的密钥冒充这个地址，SealMail 会立即标红警告。")}
            </div>
            <div className="actions">
              <button className="btn-solid" style={{ background: "var(--jade)" }} onClick={p.onTrustSender}>
                ✓ {t("确认信任")}
              </button>
              <button className="btn-ghost" onClick={() => setTrustConfirm(false)}>
                {t("取消")}
              </button>
            </div>
          </div>
        )}

        {banner && p.riskAcked && (
          <div className="risk-acked-row">
            <span>✓ {t("已确认风险提示")} · {banner.title}</span>
            <button className="btn-ghost" style={{ height: 24, padding: "0 10px", fontSize: 11 }} onClick={p.onShowRisk}>
              {t("重新查看")}
            </button>
          </div>
        )}
        {banner && !p.riskAcked && (
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

        {conversation.length > 0 ? (
          <div className="thread-conversation" aria-label={t("会话，共 {n} 封", { n: conversation.length })}>
            {conversation.map(renderThreadCard)}
          </div>
        ) : (
          renderBody(m, htmlMode, setHtmlMode)
        )}
      </div>

      {imagePreview && (
        <div
          className="image-preview-overlay"
          role="dialog"
          aria-modal="true"
          aria-label={t("图片预览")}
          onClick={() => setImagePreview(null)}
        >
          <div className="image-preview-panel" onClick={(e) => e.stopPropagation()}>
            <div className="image-preview-head">
              <div className="image-preview-title" title={imagePreview.name}>
                {imagePreview.name}
              </div>
              <button className="modal-close" onClick={() => setImagePreview(null)} aria-label={t("关闭预览")}>
                ×
              </button>
            </div>
            <div className="image-preview-body">
              {imagePreview.error ? (
                <div className="image-preview-status image-preview-error">{imagePreview.error}</div>
              ) : imagePreview.src ? (
                <img src={imagePreview.src} alt={imagePreview.name} className="image-preview-img" />
              ) : (
                <div className="image-preview-status">{t("加载预览…")}</div>
              )}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
