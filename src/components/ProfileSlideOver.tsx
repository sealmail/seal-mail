import { useI18n } from "../i18n";
import { Seal } from "./Seal";
import { statusText, TONE_COLOR } from "../trust";
import type { EmailFull, TrustedContact } from "../types";

interface Props {
  mail: EmailFull;
  trusted: TrustedContact[];
  onClose: () => void;
}

interface SourceRow {
  kind: "ok" | "bad" | "neu";
  label: string;
  val: string;
}

export function ProfileSlideOver(p: Props) {
  const t = useI18n();
  const m = p.mail;
  const v = m.verify;
  const st = statusText(v);
  const contact = p.trusted.find((t) => t.email.toLowerCase() === m.meta.fromAddr.toLowerCase());

  let statusTitle = st.title;
  let sources: SourceRow[] = [];
  let stats: { num: string; label: string; red?: boolean }[] = [];
  let note = "";
  let noteTone: "jade" | "gray" | "red" = "gray";

  switch (v.status) {
    case "verified":
      statusTitle = t("可信联系人");
      sources = [
        { kind: "ok", label: t("邮箱地址"), val: m.meta.fromAddr },
        { kind: "ok", label: t("密钥指纹"), val: v.fingerprint },
        { kind: "ok", label: t("签名方式"), val: v.method },
      ];
      stats = [
        { num: v.since, label: t("建立信任时间") },
        { num: String(v.verifiedCount), label: t("已验证签名") },
        { num: "0", label: t("失败验证") },
        { num: m.meta.lang, label: t("常用语言") },
      ];
      note = t("这把密钥自 {since} 起为该联系人签署邮件，期间指纹保持一致。可放心信任。", { since: v.since });
      noteTone = "jade";
      break;
    case "signedUnknown":
      statusTitle = t("签名有效 · 未列入可信");
      sources = [
        { kind: "ok", label: t("签名校验"), val: t("通过") },
        { kind: "neu", label: t("密钥指纹"), val: v.fingerprint },
        { kind: "neu", label: t("可信记录"), val: t("无") },
      ];
      stats = [
        { num: "1", label: t("收到邮件") },
        { num: "—", label: t("已知时长") },
      ];
      note = t("签名有效说明邮件确实出自这把密钥，但密钥与真人身份的对应关系需要你通过其他渠道（电话、面对面）核实一次，之后加入可信联系人。");
      break;
    case "unsigned":
      statusTitle = t("未验证 · 无签名");
      sources = [
        { kind: "neu", label: t("签名"), val: t("无") },
        { kind: "neu", label: t("密钥指纹"), val: "—" },
        { kind: "neu", label: t("历史记录"), val: contact ? t("曾在可信记录中") : t("无") },
      ];
      stats = [
        { num: "0", label: t("已验证签名") },
        { num: "—", label: t("已知时长") },
      ];
      note = t("未签名不代表恶意——很多正常往来也未签名。但在身份得到验证前，请勿据此执行付款、合同或账号安全相关操作。");
      break;
    case "tampered":
      statusTitle = t("签名无效 · 内容被改动");
      sources = [
        { kind: "ok", label: t("签名"), val: t("存在") },
        { kind: "bad", label: t("内容完整性"), val: t("已被改动") },
        { kind: "bad", label: t("哈希"), val: t("不匹配") },
      ];
      stats = [
        { num: "⚠", label: t("完整性"), red: true },
        { num: "1", label: t("收到邮件") },
      ];
      note = t("这封邮件携带签名，但收到时的正文哈希与签名时不符，说明内容在传输途中被改动。请以发件人确认的原始版本为准。");
      noteTone = "red";
      break;
    case "impersonation":
      statusTitle = t("非可信联系人 · 疑似冒充");
      sources = [
        { kind: "bad", label: t("域名"), val: v.gotDomain },
        { kind: "bad", label: t("密钥指纹"), val: v.gotFingerprint ?? t("不符 / 无") },
        { kind: "neu", label: t("可信记录"), val: v.realDomain },
      ];
      stats = [
        { num: "0", label: t("可信记录"), red: true },
        { num: "⛔", label: t("冒充判定"), red: true },
      ];
      note = t("显示名「{name}」与你的可信联系人相同，但密钥指纹和域名都对不上。这是冒充可信联系人的典型钓鱼手法——头像、显示名、语言都可能是真的，唯有密钥不会说谎。", { name: v.claimed });
      noteTone = "red";
      break;
  }

  const noteColors = {
    jade: { bg: "var(--jade-bg)", border: "var(--jade-border-soft)", fg: "var(--jade)" },
    gray: { bg: "var(--gray-bg)", border: "var(--border-3)", fg: "var(--gray)" },
    red: { bg: "var(--red-soft-bg)", border: "var(--red-border)", fg: "var(--tone-red)" },
  }[noteTone];

  const statusBg = { jade: "var(--jade-bg)", gold: "var(--amber-bg)", gray: "var(--gray-bg)", red: "var(--red-soft-bg)" }[st.tone];

  return (
    <>
      <div className="overlay dim" style={{ zIndex: 40 }} onClick={p.onClose} />
      <div className="slideover">
        <div className="modal-head">
          <span className="title">{t("发件人可信档案")}</span>
          <button className="modal-close" onClick={p.onClose}>
            ×
          </button>
        </div>
        <div style={{ flex: 1, overflowY: "auto", padding: "28px 26px 40px" }}>
          <div style={{ display: "flex", flexDirection: "column", alignItems: "center", textAlign: "center" }}>
            <Seal trust={m.meta.trust} size={108} />
            <div style={{ fontSize: 19, fontWeight: 700, color: "var(--ink)", marginTop: 16 }}>{m.meta.fromName}</div>
            <div style={{ fontSize: 12, color: "var(--mut)", marginTop: 3, fontFamily: "var(--mono)" }}>
              {m.meta.fromAddr}
            </div>
            <div
              style={{
                marginTop: 12,
                fontSize: 12.5,
                fontWeight: 700,
                color: TONE_COLOR[st.tone],
                background: statusBg,
                borderRadius: 8,
                padding: "6px 14px",
              }}
            >
              {statusTitle}
            </div>
          </div>

          <div className="rail-div" />

          <div className="section-label" style={{ fontSize: 11 }}>
            {t("身份来源")}
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 12, marginBottom: 26 }}>
            {sources.map((s, i) => (
              <div className={`check ${s.kind}`} key={i} style={{ alignItems: "center" }}>
                <div className="dot">{s.kind === "ok" ? "✓" : s.kind === "bad" ? "✕" : "–"}</div>
                <span style={{ fontSize: 12.5, color: "var(--ink-3)", flex: 1 }}>{s.label}</span>
                <span style={{ fontFamily: "var(--mono)", fontSize: 11, color: "var(--mut)", maxWidth: 220, textAlign: "right", wordBreak: "break-all" }}>
                  {s.val}
                </span>
              </div>
            ))}
          </div>

          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12, marginBottom: 26 }}>
            {stats.map((s, i) => (
              <div key={i} style={{ border: "1px solid var(--border-2)", borderRadius: 11, padding: 14, background: "var(--surface)" }}>
                <div style={{ fontSize: 21, fontWeight: 700, color: s.red ? "var(--tone-red)" : "var(--ink)", fontFamily: "var(--mono)" }}>
                  {s.num}
                </div>
                <div style={{ fontSize: 11, color: "var(--mut)", marginTop: 3 }}>{s.label}</div>
              </div>
            ))}
          </div>

          <div
            style={{
              borderRadius: 12,
              background: noteColors.bg,
              border: `1px solid ${noteColors.border}`,
              padding: "15px 16px",
            }}
          >
            <div style={{ fontSize: 12.5, lineHeight: 1.6, color: noteColors.fg }}>{note}</div>
          </div>
        </div>
      </div>
    </>
  );
}
