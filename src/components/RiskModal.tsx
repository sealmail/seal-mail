import { useState } from "react";
import type { EmailFull } from "../types";

interface Props {
  mail: EmailFull;
  onClose: () => void;
}

export function RiskModal(p: Props) {
  const [ack, setAck] = useState(false);
  const reasons = p.mail.meta.risk?.reasons ?? [];
  const kind = p.mail.meta.risk?.kind;
  const trust = p.mail.meta.trust;
  const dangerous = trust === "impersonation" || trust === "tampered" || kind === "account";

  const ackText =
    kind === "fund"
      ? "我已通过电话或线下渠道独立核实此付款请求"
      : "我已了解上述风险，并自行承担后续操作的责任";

  return (
    <div className="overlay" onClick={p.onClose}>
      <div className="modal" style={{ width: 520 }} onClick={(e) => e.stopPropagation()}>
        <div
          style={{
            padding: "24px 26px",
            background: "#FBECE9",
            borderBottom: "1px solid #F2D7D0",
            display: "flex",
            gap: 14,
            alignItems: "flex-start",
          }}
        >
          <div style={{ fontSize: 26 }}>🔺</div>
          <div>
            <div style={{ fontSize: 16, fontWeight: 700, color: "#9A2C1D" }}>
              {dangerous ? "高风险邮件 · 请勿按邮件要求操作" : "高风险操作 · 需人工核实"}
            </div>
            <div style={{ fontSize: 12.5, color: "#9A2C1D", opacity: 0.9, marginTop: 4, lineHeight: 1.5 }}>
              {trust === "verified"
                ? "发件人身份已验证，但此操作不应仅凭一封邮件执行。"
                : "此邮件未通过身份验证，其中的要求不可信。"}
            </div>
          </div>
        </div>
        <div style={{ padding: "22px 26px" }}>
          <div style={{ display: "flex", flexDirection: "column", gap: 13 }}>
            {reasons.map((r, i) => (
              <div key={i} style={{ display: "flex", gap: 11, alignItems: "flex-start" }}>
                <div
                  style={{
                    width: 18,
                    height: 18,
                    borderRadius: "50%",
                    background: "#FBECE9",
                    color: "#B23A2B",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    fontSize: 11,
                    fontWeight: 700,
                    flexShrink: 0,
                    marginTop: 1,
                  }}
                >
                  !
                </div>
                <div style={{ fontSize: 12.5, color: "#2A2E36", lineHeight: 1.55 }}>{r}</div>
              </div>
            ))}
            {reasons.length === 0 && (
              <div style={{ fontSize: 12.5, color: "#2A2E36" }}>验证未通过：请通过其他渠道与发件人核实。</div>
            )}
          </div>
          <div className={`ack-row${ack ? " on" : ""}`} onClick={() => setAck(!ack)}>
            <div className="ack-box">{ack ? "✓" : ""}</div>
            <span style={{ fontSize: 12.5, color: "#3A3E46" }}>{ackText}</span>
          </div>
          <div style={{ display: "flex", gap: 10, marginTop: 20 }}>
            <button className="btn-ghost" style={{ flex: 1, height: 42, borderRadius: 9 }} onClick={p.onClose}>
              取消
            </button>
            <button
              style={{
                flex: 1,
                height: 42,
                border: "none",
                borderRadius: 9,
                background: ack ? "#B23A2B" : "#D8B6B0",
                color: "#fff",
                fontSize: 13,
                fontWeight: 600,
                cursor: ack ? "pointer" : "not-allowed",
                opacity: ack ? 1 : 0.7,
              }}
              disabled={!ack}
              onClick={p.onClose}
            >
              确认并继续
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
