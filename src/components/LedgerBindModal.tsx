import { useState } from "react";
import { bindLedger, ledgerGetAddresses } from "../api";
import type { IdentityInfo, LedgerAccountRow } from "../types";

interface Props {
  onClose: () => void;
  onBound: (info: IdentityInfo) => void;
}

/** 绑定 Ledger：读取设备前 5 个账户地址，选择一个作为签名身份 */
export function LedgerBindModal(p: Props) {
  const [rows, setRows] = useState<LedgerAccountRow[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<LedgerAccountRow | null>(null);

  async function loadAddresses() {
    setBusy(true);
    setError(null);
    try {
      const r = await ledgerGetAddresses(5);
      setRows(r);
      setSelected(r[0] ?? null);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function doBind() {
    if (!selected) return;
    setBusy(true);
    setError(null);
    try {
      const info = await bindLedger(selected.path, selected.address);
      p.onBound(info);
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  }

  return (
    <div className="overlay">
      <div className="modal" style={{ width: 520 }}>
        <div className="modal-head">
          <span className="title">绑定 Ledger 硬件密钥</span>
          <button className="modal-close" onClick={p.onClose}>
            ×
          </button>
        </div>
        <div className="modal-body" style={{ display: "flex", flexDirection: "column", gap: 14 }}>
          {!rows && (
            <>
              <div style={{ fontSize: 13, color: "#3A3E46", lineHeight: 1.7 }}>
                1. 用 USB 连接 Ledger 并解锁
                <br />
                2. 在设备上打开 <b>Ethereum</b> app
                <br />
                3. 点击下方按钮读取账户地址
              </div>
              <div style={{ fontSize: 11.5, color: "#A39E91", lineHeight: 1.6 }}>
                绑定后，发邮件签名会改用 Ledger（EIP-191 personal_sign，secp256k1）。
                每次发送签名邮件需要在设备上按键确认；私钥永不离开硬件。
              </div>
            </>
          )}

          {rows && (
            <>
              <div style={{ fontSize: 12.5, color: "#3A3E46" }}>选择用于签名的账户：</div>
              <div className="card-list">
                {rows.map((r) => (
                  <div
                    className="card-row"
                    key={r.index}
                    style={{ cursor: "pointer", background: selected?.index === r.index ? "#F3EFE6" : "#fff" }}
                    onClick={() => setSelected(r)}
                  >
                    <div
                      className="ack-box"
                      style={
                        selected?.index === r.index
                          ? { borderColor: "#1E6B49", background: "#1E6B49" }
                          : undefined
                      }
                    >
                      {selected?.index === r.index ? "✓" : ""}
                    </div>
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{ fontFamily: "var(--mono)", fontSize: 12, color: "#2A2E36" }}>
                        {r.address.slice(0, 22)}…{r.address.slice(-6)}
                      </div>
                      <div style={{ fontFamily: "var(--mono)", fontSize: 10.5, color: "#A39E91", marginTop: 2 }}>
                        {r.path}
                      </div>
                    </div>
                  </div>
                ))}
              </div>
            </>
          )}

          {error && <div className="form-error">{error}</div>}
        </div>
        <div className="modal-foot">
          <button className="btn-ghost" style={{ height: 40 }} onClick={p.onClose}>
            取消
          </button>
          {!rows ? (
            <button className="btn-primary" style={{ height: 40 }} disabled={busy} onClick={loadAddresses}>
              {busy ? "正在连接设备…" : "连接 Ledger 并读取地址"}
            </button>
          ) : (
            <button className="btn-primary" style={{ height: 40 }} disabled={busy || !selected} onClick={doBind}>
              {busy ? "正在绑定…" : "绑定所选账户"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
