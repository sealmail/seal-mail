import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Seal } from "./Seal";
import { getCloseBehavior, getNotifyNewMail, setCloseBehavior, setNotifyNewMail, useLocalKey } from "../api";
import { LedgerBindModal } from "./LedgerBindModal";
import { shortFpr } from "../trust";
import {
  checkForUpdate,
  installUpdate,
  updateBarState,
  type UpdateInfo,
  type UpdateProgress,
} from "../updater";
import type { IdentityInfo, TrustedContact } from "../types";

interface Props {
  identity: IdentityInfo | null;
  trusted: TrustedContact[];
  onBack: () => void;
  onRemoveTrusted: (email: string) => void;
  onIdentityChanged: (info: IdentityInfo) => void;
}

function shortAddr(addr: string) {
  return addr.length > 12 ? `${addr.slice(0, 6)}…${addr.slice(-4)}` : addr;
}

export function KeysView(p: Props) {
  const [ledgerModal, setLedgerModal] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const isLedger = p.identity?.mode === "ledger";

  // ── 关闭按钮行为（macOS 默认隐藏窗口）──
  const [closeBehavior, setCloseBehaviorState] = useState<"hide" | "quit" | null>(null);
  useEffect(() => {
    getCloseBehavior()
      .then(setCloseBehaviorState)
      .catch((e) => setError(String(e)));
  }, []);

  async function handleCloseBehavior(next: "hide" | "quit") {
    try {
      setCloseBehaviorState(await setCloseBehavior(next));
    } catch (e) {
      setError(String(e));
    }
  }

  // ── 新邮件系统通知 ──
  const [notify, setNotify] = useState<boolean | null>(null);
  useEffect(() => {
    getNotifyNewMail()
      .then(setNotify)
      .catch((e) => setError(String(e)));
  }, []);

  async function handleNotify(next: boolean) {
    try {
      setNotify(await setNotifyNewMail(next));
    } catch (e) {
      setError(String(e));
    }
  }

  // ── 软件更新（UX 参考 auto-desktop）──
  const [updated, setUpdated] = useState(false);
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const [updateProgress, setUpdateProgress] = useState<UpdateProgress | null>(null);
  const [updateMsg, setUpdateMsg] = useState<{ text: string; warn?: boolean } | null>(null);

  const updateButtonLabel = checkingUpdate
    ? updateInfo?.available
      ? "正在安装…"
      : "正在检查…"
    : updateInfo?.available
      ? "安装更新"
      : "检查更新";

  async function handleCheckUpdates() {
    if (updateInfo?.available && !updateInfo.manual) {
      await handleInstallUpdate();
      return;
    }
    setCheckingUpdate(true);
    setUpdated(false);
    setUpdateProgress(null);
    setUpdateMsg(null);
    try {
      const info = await checkForUpdate();
      setUpdateInfo(info);
      if (!info.available) {
        setUpdated(true);
        setUpdateMsg({ text: "已是最新版本" });
        return;
      }
      if (info.manual) {
        setUpdateMsg({
          text: `自动升级不可用（${info.autoError || "未知错误"}），请打开下载页手动升级`,
          warn: true,
        });
        return;
      }
      setUpdateMsg({ text: `发现新版本 v${info.latestVersion}，点击「安装更新」升级` });
    } catch (e) {
      setUpdateMsg({ text: `检查更新失败：${String(e)}`, warn: true });
    } finally {
      setCheckingUpdate(false);
    }
  }

  async function handleInstallUpdate() {
    if (!updateInfo?.available) return;
    setCheckingUpdate(true);
    setUpdateProgress({ phase: "downloading", downloaded: 0 });
    setUpdateMsg(null);
    try {
      const info = await installUpdate({ ...updateInfo, manual: false }, setUpdateProgress);
      setUpdateInfo(info);
      if (!info.available) {
        setUpdated(true);
        setUpdateMsg({ text: "已是最新版本" });
        return;
      }
      if (info.manual) {
        setUpdateMsg({
          text: `自动升级失败（${info.autoError || "未知错误"}），请打开下载页手动升级`,
          warn: true,
        });
        return;
      }
      setUpdateMsg({ text: `已安装 v${info.latestVersion}，应用即将重启` });
    } catch (e) {
      setUpdateMsg({ text: `升级失败：${String(e)}`, warn: true });
    } finally {
      setCheckingUpdate(false);
      setUpdateProgress(null);
    }
  }

  async function handleManualDownload() {
    if (!updateInfo?.available) return;
    await openUrl(updateInfo.downloadUrl || updateInfo.releaseUrl);
  }

  const updateBar = updateProgress ? updateBarState(updateProgress) : null;

  async function switchToLocal() {
    setBusy(true);
    setError(null);
    try {
      const info = await useLocalKey();
      p.onIdentityChanged(info);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

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
            <div style={{ fontSize: 18, fontWeight: 700, color: "var(--ink)" }}>我的签名身份</div>
            <div style={{ fontFamily: "var(--mono)", fontSize: 12, color: "#1E6B49", marginTop: 3 }}>
              {isLedger
                ? `Ledger · secp256k1 · ${p.identity?.ledgerPath ?? ""}`
                : `Ed25519 · 本地生成 · ${p.identity ? p.identity.created.slice(0, 10) : "…"}`}
            </div>
            <div style={{ fontFamily: "var(--mono)", fontSize: 11, color: "var(--mut)", marginTop: 5, wordBreak: "break-all" }}>
              {isLedger ? `地址 ${p.identity?.ledgerAddress ?? ""}` : `指纹 ${p.identity?.fingerprint ?? "…"}`}
            </div>
          </div>
        </div>

        <div className="section-label">签名密钥（发送签名邮件时使用其中一个）</div>
        <div style={{ display: "flex", flexDirection: "column", gap: 10, marginBottom: 14 }}>
          <div className="card-row" style={{ border: "1px solid var(--border-2)", borderRadius: 12, background: "#fff" }}>
            <div
              style={{
                width: 44, height: 30, borderRadius: 6, background: "var(--ink)",
                display: "flex", alignItems: "center", justifyContent: "center",
              }}
            >
              <div style={{ width: 20, height: 14, borderRadius: 2, background: "#0e1217", boxShadow: "inset 0 0 0 1px var(--ink-3)" }} />
            </div>
            <div style={{ flex: 1 }}>
              <div style={{ fontSize: 13.5, fontWeight: 600, color: "var(--ink-2)" }}>SealMail 本地密钥</div>
              <div style={{ fontFamily: "var(--mono)", fontSize: 11, color: "var(--mut)", marginTop: 2 }}>
                Ed25519 · 私钥仅保存在本机 · 无需额外硬件
              </div>
            </div>
            {isLedger ? (
              <button className="btn-ghost" disabled={busy} onClick={switchToLocal}>
                改用本地密钥
              </button>
            ) : (
              <span className="pill jade">使用中</span>
            )}
          </div>

          <div className="card-row" style={{ border: "1px solid var(--border-2)", borderRadius: 12, background: "#fff" }}>
            <div
              style={{
                width: 44, height: 30, borderRadius: 6, background: "var(--ink)", position: "relative",
                display: "flex", alignItems: "center", justifyContent: "center",
              }}
            >
              <div style={{ width: 20, height: 14, borderRadius: 2, background: "#0e1217", boxShadow: "inset 0 0 0 1px var(--ink-3)" }} />
              <div style={{ position: "absolute", right: -4, top: "50%", transform: "translateY(-50%)", width: 7, height: 7, borderRadius: "50%", background: "var(--ink-3)" }} />
            </div>
            <div style={{ flex: 1 }}>
              <div style={{ fontSize: 13.5, fontWeight: 600, color: "var(--ink-2)" }}>Ledger 硬件密钥</div>
              <div style={{ fontFamily: "var(--mono)", fontSize: 11, color: "var(--mut)", marginTop: 2 }}>
                {isLedger
                  ? `secp256k1 · ${shortAddr(p.identity?.ledgerAddress ?? "")} · 每次签名需设备确认`
                  : "secp256k1 · EIP-191 · 私钥永不离开硬件"}
              </div>
            </div>
            {isLedger ? (
              <span className="pill jade">使用中</span>
            ) : (
              <button className="btn-ghost" onClick={() => setLedgerModal(true)}>
                绑定 Ledger
              </button>
            )}
          </div>
        </div>
        {isLedger && (
          <div style={{ fontSize: 11.5, color: "var(--amber)", background: "var(--amber-bg)", border: "1px solid var(--amber-border)", borderRadius: 9, padding: "10px 14px", marginBottom: 16, lineHeight: 1.6 }}>
            使用 Ledger 时，每封签名邮件发送前需要：连接设备 → 解锁 → 打开 Ethereum app → 在设备上确认。
          </div>
        )}
        {error && <div className="form-error" style={{ marginBottom: 16 }}>{error}</div>}

        <div className="section-label" style={{ marginTop: 16 }}>
          可信联系人（已记录密钥指纹 / 地址）
        </div>
        {p.trusted.length === 0 ? (
          <div className="card-list" style={{ padding: "22px 18px", fontSize: 12.5, color: "var(--mut)", lineHeight: 1.6 }}>
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
                  <div style={{ fontSize: 13, fontWeight: 600, color: "var(--ink-2)" }}>
                    {t.name}
                    {t.org ? <span style={{ fontWeight: 400, color: "var(--mut-3)" }}> · {t.org}</span> : null}
                  </div>
                  <div style={{ fontFamily: "var(--mono)", fontSize: 11, color: "var(--mut)", marginTop: 2, wordBreak: "break-all" }}>
                    {t.email} · {t.fingerprint.startsWith("0x") ? shortAddr(t.fingerprint) : shortFpr(t.fingerprint)}
                  </div>
                </div>
                <div style={{ textAlign: "right" }}>
                  <div style={{ fontSize: 11.5, color: "var(--ink-3)", fontWeight: 500 }}>自 {t.since}</div>
                  <div style={{ fontSize: 10.5, color: "var(--mut-3)" }}>{t.verifiedCount} 封已验证</div>
                </div>
                <button className="icon-btn" title="移除可信" onClick={() => p.onRemoveTrusted(t.email)}>
                  ×
                </button>
              </div>
            ))}
          </div>
        )}
        <div className="section-label" style={{ marginTop: 30 }}>
          关于与更新
        </div>
        <div className="card-list" style={{ padding: "16px 18px" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
            <div
              style={{
                width: 36, height: 36, borderRadius: 9, flexShrink: 0,
                background: "radial-gradient(circle at 36% 30%, #4ca67e, #1b5840)",
                boxShadow: "0 0 0 1.5px var(--gold)",
                display: "flex", alignItems: "center", justifyContent: "center",
                fontFamily: "var(--serif)", fontSize: 19, color: "rgba(255,255,255,.92)",
              }}
            >
              印
            </div>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontSize: 13.5, fontWeight: 600, color: "var(--ink-2)" }}>SealMail 信印</div>
              {updated ? (
                <div style={{ fontSize: 11.5, color: "#1E6B49", marginTop: 2 }}>✓ 已是最新版本</div>
              ) : updateInfo?.available ? (
                <div style={{ fontSize: 11.5, color: "var(--amber)", marginTop: 2 }}>
                  ↓ 新版本 v{updateInfo.latestVersion} 可用
                </div>
              ) : (
                <div style={{ fontSize: 11.5, color: "var(--mut)", marginTop: 2, fontFamily: "var(--mono)" }}>
                  版本 {__APP_VERSION__}
                </div>
              )}
            </div>
            {updateInfo?.available && updateInfo.manual ? (
              <button className="btn-ghost" onClick={handleManualDownload}>
                打开下载页面
              </button>
            ) : (
              <button className="btn-ghost" disabled={checkingUpdate} onClick={() => void handleCheckUpdates()}>
                {updateButtonLabel}
              </button>
            )}
          </div>

          {updateProgress && updateBar && (
            <div className="update-progress">
              <div className="update-progress-meta">
                <span>{updateProgress.phase === "installing" ? "正在安装…" : "正在下载更新…"}</span>
                {!updateBar.indeterminate && <span>{updateBar.percent}%</span>}
              </div>
              <div
                className="update-progress-track"
                role="progressbar"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={updateBar.percent ?? undefined}
              >
                <div
                  className={updateBar.indeterminate ? "update-progress-fill indeterminate" : "update-progress-fill"}
                  style={updateBar.indeterminate ? undefined : { width: `${updateBar.percent}%` }}
                />
              </div>
            </div>
          )}

          {updateMsg && (
            <div className={updateMsg.warn ? "form-error" : "form-ok"} style={{ marginTop: 12 }}>
              {updateMsg.text}
            </div>
          )}

          <div
            style={{
              display: "flex", alignItems: "center", gap: 14, marginTop: 16,
              paddingTop: 16, borderTop: "1px solid var(--border-soft)",
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontSize: 13, fontWeight: 600, color: "var(--ink-2)" }}>点击关闭按钮时</div>
              <div style={{ fontSize: 11.5, color: "var(--mut)", marginTop: 2, lineHeight: 1.5 }}>
                隐藏窗口后应用继续在后台运行，点击程序坞图标重新打开（macOS 常规行为）
              </div>
            </div>
            <select
              className="select"
              style={{ width: 130 }}
              value={closeBehavior ?? "hide"}
              disabled={closeBehavior === null}
              onChange={(e) => void handleCloseBehavior(e.target.value as "hide" | "quit")}
            >
              <option value="hide">隐藏窗口</option>
              <option value="quit">退出应用</option>
            </select>
          </div>

          <div
            style={{
              display: "flex", alignItems: "center", gap: 14, marginTop: 16,
              paddingTop: 16, borderTop: "1px solid var(--border-soft)",
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontSize: 13, fontWeight: 600, color: "var(--ink-2)" }}>新邮件系统通知</div>
              <div style={{ fontSize: 11.5, color: "var(--mut)", marginTop: 2, lineHeight: 1.5 }}>
                窗口在后台或隐藏时，收到新邮件弹系统横幅
              </div>
            </div>
            <select
              className="select"
              style={{ width: 130 }}
              value={notify === null ? "on" : notify ? "on" : "off"}
              disabled={notify === null}
              onChange={(e) => void handleNotify(e.target.value === "on")}
            >
              <option value="on">开启</option>
              <option value="off">关闭</option>
            </select>
          </div>
        </div>
      </div>

      {ledgerModal && (
        <LedgerBindModal
          onClose={() => setLedgerModal(false)}
          onBound={(info) => {
            setLedgerModal(false);
            p.onIdentityChanged(info);
          }}
        />
      )}
    </div>
  );
}
