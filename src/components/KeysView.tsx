import { Seal } from "./Seal";
import type { IdentityInfo, TrustedContact } from "../types";

interface Props {
  identity: IdentityInfo | null;
  trusted: TrustedContact[];
  demoMode: boolean;
  onBack: () => void;
  onRemoveTrusted: (email: string) => void;
}

export function KeysView(p: Props) {
  return (
    <div className="keys-page">
      <div className="keys-inner">
        <button className="keys-back" onClick={p.onBack}>
          ← 返回收件箱
        </button>
        <h1 className="keys-title">身份与密钥</h1>
        <p className="keys-sub">
          你的签名密钥决定收件人看到的封印。可信联系人记录用于识别冒充——即使对方头像、显示名、域名都对得上。
        </p>

        <div className="keys-hero">
          <div className="seal-lg">印</div>
          <div style={{ minWidth: 0 }}>
            <div style={{ fontSize: 18, fontWeight: 700, color: "#23272F" }}>我的签名身份</div>
            <div style={{ fontFamily: "var(--mono)", fontSize: 12, color: "#1E6B49", marginTop: 3 }}>
              Ed25519 · 本地生成 · {p.identity ? p.identity.created.slice(0, 10) : "…"}
            </div>
            <div style={{ fontFamily: "var(--mono)", fontSize: 11, color: "#8A8576", marginTop: 5, wordBreak: "break-all" }}>
              指纹 {p.identity?.fingerprint ?? "…"}
            </div>
          </div>
        </div>

        <div className="section-label">签名密钥</div>
        <div style={{ display: "flex", flexDirection: "column", gap: 10, marginBottom: 30 }}>
          <div className="card-row" style={{ border: "1px solid #E8E3D8", borderRadius: 12, background: "#fff" }}>
            <div
              style={{
                width: 44,
                height: 30,
                borderRadius: 6,
                background: "#23272F",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
              }}
            >
              <div style={{ width: 20, height: 14, borderRadius: 2, background: "#0E1217", boxShadow: "inset 0 0 0 1px #3A3E46" }} />
            </div>
            <div style={{ flex: 1 }}>
              <div style={{ fontSize: 13.5, fontWeight: 600, color: "#2A2E36" }}>SealMail 本地密钥</div>
              <div style={{ fontFamily: "var(--mono)", fontSize: 11, color: "#8A8576", marginTop: 2 }}>
                Ed25519 · 私钥仅保存在本机
              </div>
            </div>
            <span className="pill jade">使用中</span>
          </div>
          <button className="dashed-add" title="硬件密钥支持在规划中">
            + 绑定硬件密钥（Ledger / YubiKey）— 规划中
          </button>
        </div>

        <div className="section-label">可信联系人（已记录密钥指纹）</div>
        {p.trusted.length === 0 ? (
          <div className="card-list" style={{ padding: "22px 18px", fontSize: 12.5, color: "#8A8576", lineHeight: 1.6 }}>
            还没有可信联系人。当你收到签名有效的邮件时，可在右侧验证面板将对方加入可信——之后任何冒充该联系人的邮件都会被标红。
          </div>
        ) : (
          <div className="card-list">
            {p.trusted.map((t) => (
              <div className="card-row" key={t.email}>
                <div style={{ paddingTop: 1 }}>
                  <Seal trust="verified" size={26} />
                </div>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: 13, fontWeight: 600, color: "#2A2E36" }}>
                    {t.name}
                    {t.org ? <span style={{ fontWeight: 400, color: "#A39E91" }}> · {t.org}</span> : null}
                  </div>
                  <div style={{ fontFamily: "var(--mono)", fontSize: 11, color: "#8A8576", marginTop: 2 }}>
                    {t.email} · {t.fingerprint}
                  </div>
                </div>
                <div style={{ textAlign: "right" }}>
                  <div style={{ fontSize: 11.5, color: "#3A3E46", fontWeight: 500 }}>自 {t.since}</div>
                  <div style={{ fontSize: 10.5, color: "#A39E91" }}>{t.verifiedCount} 封已验证</div>
                </div>
                {!p.demoMode && (
                  <button className="icon-btn" title="移除可信" onClick={() => p.onRemoveTrusted(t.email)}>
                    ×
                  </button>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
