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
      statusTitle = "可信联系人";
      sources = [
        { kind: "ok", label: "邮箱地址", val: m.meta.fromAddr },
        { kind: "ok", label: "密钥指纹", val: v.fingerprint },
        { kind: "ok", label: "签名方式", val: v.method },
      ];
      stats = [
        { num: v.since, label: "建立信任时间" },
        { num: String(v.verifiedCount), label: "已验证签名" },
        { num: "0", label: "失败验证" },
        { num: m.meta.lang, label: "常用语言" },
      ];
      note = `这把密钥自 ${v.since} 起为该联系人签署邮件，期间指纹保持一致。可放心信任。`;
      noteTone = "jade";
      break;
    case "signedUnknown":
      statusTitle = "签名有效 · 未列入可信";
      sources = [
        { kind: "ok", label: "签名校验", val: "通过" },
        { kind: "neu", label: "密钥指纹", val: v.fingerprint },
        { kind: "neu", label: "可信记录", val: "无" },
      ];
      stats = [
        { num: "1", label: "收到邮件" },
        { num: "—", label: "已知时长" },
      ];
      note = "签名有效说明邮件确实出自这把密钥，但密钥与真人身份的对应关系需要你通过其他渠道（电话、面对面）核实一次，之后加入可信联系人。";
      break;
    case "unsigned":
      statusTitle = "未验证 · 无签名";
      sources = [
        { kind: "neu", label: "签名", val: "无" },
        { kind: "neu", label: "密钥指纹", val: "—" },
        { kind: "neu", label: "历史记录", val: contact ? "曾在可信记录中" : "无" },
      ];
      stats = [
        { num: "0", label: "已验证签名" },
        { num: "—", label: "已知时长" },
      ];
      note = "未签名不代表恶意——很多正常往来也未签名。但在身份得到验证前，请勿据此执行付款、合同或账号安全相关操作。";
      break;
    case "tampered":
      statusTitle = "签名无效 · 内容被改动";
      sources = [
        { kind: "ok", label: "签名", val: "存在" },
        { kind: "bad", label: "内容完整性", val: "已被改动" },
        { kind: "bad", label: "哈希", val: "不匹配" },
      ];
      stats = [
        { num: "⚠", label: "完整性", red: true },
        { num: "1", label: "收到邮件" },
      ];
      note = "这封邮件携带签名，但收到时的正文哈希与签名时不符，说明内容在传输途中被改动。请以发件人确认的原始版本为准。";
      noteTone = "red";
      break;
    case "impersonation":
      statusTitle = "非可信联系人 · 疑似冒充";
      sources = [
        { kind: "bad", label: "域名", val: v.gotDomain },
        { kind: "bad", label: "密钥指纹", val: v.gotFingerprint ?? "不符 / 无" },
        { kind: "neu", label: "可信记录", val: v.realDomain },
      ];
      stats = [
        { num: "0", label: "可信记录", red: true },
        { num: "⛔", label: "冒充判定", red: true },
      ];
      note = `显示名「${v.claimed}」与你的可信联系人相同，但密钥指纹和域名都对不上。这是冒充可信联系人的典型钓鱼手法——头像、显示名、语言都可能是真的，唯有密钥不会说谎。`;
      noteTone = "red";
      break;
  }

  const noteColors = {
    jade: { bg: "#EAF4EE", border: "#CDE4D7", fg: "#1E6B49" },
    gray: { bg: "#F1EDE3", border: "#E4DECF", fg: "#6E6A5F" },
    red: { bg: "#FBECE9", border: "#F2D7D0", fg: "#9A2C1D" },
  }[noteTone];

  const statusBg = { jade: "#EAF4EE", gold: "#FBEFD9", gray: "#F1EDE3", red: "#FBECE9" }[st.tone];

  return (
    <>
      <div className="overlay dim" style={{ zIndex: 40 }} onClick={p.onClose} />
      <div className="slideover">
        <div className="modal-head">
          <span className="title">发件人可信档案</span>
          <button className="modal-close" onClick={p.onClose}>
            ×
          </button>
        </div>
        <div style={{ flex: 1, overflowY: "auto", padding: "28px 26px 40px" }}>
          <div style={{ display: "flex", flexDirection: "column", alignItems: "center", textAlign: "center" }}>
            <Seal trust={m.meta.trust} size={108} />
            <div style={{ fontSize: 19, fontWeight: 700, color: "#23272F", marginTop: 16 }}>{m.meta.fromName}</div>
            <div style={{ fontSize: 12, color: "#8A8576", marginTop: 3, fontFamily: "var(--mono)" }}>
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
            身份来源
          </div>
          <div style={{ display: "flex", flexDirection: "column", gap: 12, marginBottom: 26 }}>
            {sources.map((s, i) => (
              <div className={`check ${s.kind}`} key={i} style={{ alignItems: "center" }}>
                <div className="dot">{s.kind === "ok" ? "✓" : s.kind === "bad" ? "✕" : "–"}</div>
                <span style={{ fontSize: 12.5, color: "#34383F", flex: 1 }}>{s.label}</span>
                <span style={{ fontFamily: "var(--mono)", fontSize: 11, color: "#8A8576", maxWidth: 220, textAlign: "right", wordBreak: "break-all" }}>
                  {s.val}
                </span>
              </div>
            ))}
          </div>

          <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12, marginBottom: 26 }}>
            {stats.map((s, i) => (
              <div key={i} style={{ border: "1px solid #E8E3D8", borderRadius: 11, padding: 14, background: "#fff" }}>
                <div style={{ fontSize: 21, fontWeight: 700, color: s.red ? "#9A2C1D" : "#23272F", fontFamily: "var(--mono)" }}>
                  {s.num}
                </div>
                <div style={{ fontSize: 11, color: "#8A8576", marginTop: 3 }}>{s.label}</div>
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
