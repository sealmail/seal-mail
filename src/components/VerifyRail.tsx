import { Seal } from "./Seal";
import { buildChecks, statusText, TONE_COLOR } from "../trust";
import type { EmailFull } from "../types";

interface Props {
  mail: EmailFull | null;
  /** 展开完整面板；折叠时只显示图标条 */
  open: boolean;
  onToggle: () => void;
  onOpenProfile: () => void;
  onTrustSender: () => void;
}

export function VerifyRail(p: Props) {
  const st = p.mail ? statusText(p.mail.verify) : null;

  // ── 折叠态：窄条 + 封印图标 + 状态，点击展开 ──
  if (!p.open) {
    return (
      <div
        className="rail rail-collapsed"
        style={{ background: st?.railBg ?? "var(--bg-side)" }}
        onClick={p.onToggle}
        title="展开验证面板"
      >
        {p.mail && st ? (
          <>
            <Seal trust={p.mail.meta.trust} size={34} />
            <div className="rail-mini-label" style={{ color: TONE_COLOR[st.tone] }}>
              {st.title}
            </div>
          </>
        ) : (
          <div className="rail-mini-label">验证面板</div>
        )}
        <div className="rail-toggle">‹</div>
      </div>
    );
  }

  if (!p.mail || !st) {
    return (
      <div className="rail" style={{ background: "var(--bg-side)" }}>
        <button className="rail-collapse-btn" onClick={p.onToggle} title="收起验证面板">
          »
        </button>
        <div className="empty-pane">验证面板</div>
      </div>
    );
  }
  const checks = buildChecks(p.mail);
  const canTrust = p.mail.verify.status === "signedUnknown";

  return (
    <div className="rail" style={{ background: st.railBg }}>
      <button className="rail-collapse-btn" onClick={p.onToggle} title="收起验证面板">
        »
      </button>
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
          <button className="rail-btn" style={{ borderColor: "var(--gold)" }} onClick={p.onTrustSender}>
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
