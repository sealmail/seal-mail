import { Seal } from "./Seal";
import { buildChecks, statusText, TONE_COLOR } from "../trust";
import type { EmailFull } from "../types";

interface Props {
  mail: EmailFull | null;
  onOpenProfile: () => void;
  onTrustSender: () => void;
}

export function VerifyRail(p: Props) {
  if (!p.mail) {
    return (
      <div className="rail" style={{ background: "#FAF9F5" }}>
        <div className="empty-pane">验证面板</div>
      </div>
    );
  }
  const st = statusText(p.mail.verify);
  const checks = buildChecks(p.mail);
  const canTrust = p.mail.verify.status === "signedUnknown";

  return (
    <div className="rail" style={{ background: st.railBg }}>
      <div className="rail-scroll">
        <div className="rail-hero">
          <Seal trust={p.mail.meta.trust} size={116} />
          <div className="rail-status" style={{ color: TONE_COLOR[st.tone] }}>
            {st.title}
          </div>
          <div className="rail-sub">{st.sub}</div>
        </div>

        <div className="rail-div" />

        <div className="rail-checks">
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

        {canTrust && (
          <button className="rail-btn" style={{ borderColor: "#C99B4E" }} onClick={p.onTrustSender}>
            ✓ 核实后加入可信联系人
          </button>
        )}
        <button className="rail-btn" onClick={p.onOpenProfile}>
          查看发件人可信档案 <span style={{ fontSize: 14 }}>→</span>
        </button>

        <div className="rail-note">SealMail 在本地验证，不依赖头像、邮件头或语言判断真伪</div>
      </div>
    </div>
  );
}
