import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { AppIcon } from "./AppIcon";
import { Seal } from "./Seal";
import {
  getCloseBehavior,
  getLanguagePref,
  getNotifyNewMail,
  getThemePref,
  setCloseBehavior,
  setLanguagePref,
  setNotifyNewMail,
  setThemePref,
  useLocalKey,
  type ThemePref,
} from "../api";
import { applyLangPref, useI18n, type LangPref } from "../i18n";
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

  // ── 界面语言 ──
  const t = useI18n();
  const [langPref, setLangPref] = useState<LangPref | null>(null);
  useEffect(() => {
    getLanguagePref()
      .then(setLangPref)
      .catch((e) => setError(String(e)));
  }, []);

  async function handleLanguage(next: LangPref) {
    try {
      const saved = await setLanguagePref(next);
      setLangPref(saved);
      applyLangPref(saved);
    } catch (e) {
      setError(String(e));
    }
  }

  // ── 外观主题 ──
  const [themePref, setThemePrefState] = useState<ThemePref | null>(null);
  useEffect(() => {
    getThemePref()
      .then(setThemePrefState)
      .catch((e) => setError(String(e)));
  }, []);

  function applyTheme(theme: ThemePref) {
    const dark =
      theme === "dark" ||
      (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches);
    document.documentElement.setAttribute("data-theme", dark ? "dark" : "light");
  }

  async function handleTheme(next: ThemePref) {
    try {
      const saved = await setThemePref(next);
      setThemePrefState(saved);
      applyTheme(saved);
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
      ? t("正在安装…")
      : t("正在检查…")
    : updateInfo?.available
      ? t("安装更新")
      : t("检查更新");

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
        setUpdateMsg({ text: t("已是最新版本") });
        return;
      }
      if (info.manual) {
        setUpdateMsg({
          text: t("自动升级不可用（{err}），请打开下载页手动升级", { err: info.autoError || t("未知错误") }),
          warn: true,
        });
        return;
      }
      setUpdateMsg({ text: t("发现新版本 v{v}，点击「安装更新」升级", { v: info.latestVersion }) });
    } catch (e) {
      setUpdateMsg({ text: t("检查更新失败：") + String(e), warn: true });
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
        setUpdateMsg({ text: t("已是最新版本") });
        return;
      }
      if (info.manual) {
        setUpdateMsg({
          text: t("自动升级失败（{err}），请打开下载页手动升级", { err: info.autoError || t("未知错误") }),
          warn: true,
        });
        return;
      }
      setUpdateMsg({ text: t("已安装 v{v}，应用即将重启", { v: info.latestVersion }) });
    } catch (e) {
      setUpdateMsg({ text: t("升级失败：") + String(e), warn: true });
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
          {t("← 返回收件箱")}
        </button>
        <h1 className="keys-title">{t("身份与密钥")}</h1>
        <p className="keys-sub">
          {t("你的签名密钥决定收件人看到的封印。可信联系人记录用于识别冒充——即使对方头像、显示名、域名都对得上。")}
        </p>

        <div className="keys-hero">
          <AppIcon className="keys-hero-icon" />
          <div style={{ minWidth: 0 }}>
            <div style={{ fontSize: 18, fontWeight: 700, color: "var(--ink)" }}>{t("我的签名身份")}</div>
            <div style={{ fontFamily: "var(--mono)", fontSize: 12, color: "#1E6B49", marginTop: 3 }}>
              {isLedger
                ? `Ledger · secp256k1 · ${p.identity?.ledgerPath ?? ""}`
                : `Ed25519 · ${t("本地生成")} · ${p.identity ? p.identity.created.slice(0, 10) : "…"}`}
            </div>
            <div style={{ fontFamily: "var(--mono)", fontSize: 11, color: "var(--mut)", marginTop: 5, wordBreak: "break-all" }}>
              {isLedger ? `${t("地址")} ${p.identity?.ledgerAddress ?? ""}` : `${t("指纹")} ${p.identity?.fingerprint ?? "…"}`}
            </div>
          </div>
        </div>

        <div className="section-label">{t("签名密钥（发送签名邮件时使用其中一个）")}</div>
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
              <div style={{ fontSize: 13.5, fontWeight: 600, color: "var(--ink-2)" }}>{t("SealMail 本地密钥")}</div>
              <div style={{ fontFamily: "var(--mono)", fontSize: 11, color: "var(--mut)", marginTop: 2 }}>
                {t("Ed25519 · 私钥仅保存在本机 · 无需额外硬件")}
              </div>
            </div>
            {isLedger ? (
              <button className="btn-ghost" disabled={busy} onClick={switchToLocal}>
                {t("改用本地密钥")}
              </button>
            ) : (
              <span className="pill jade">{t("使用中")}</span>
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
              <div style={{ fontSize: 13.5, fontWeight: 600, color: "var(--ink-2)" }}>{t("Ledger 硬件密钥")}</div>
              <div style={{ fontFamily: "var(--mono)", fontSize: 11, color: "var(--mut)", marginTop: 2 }}>
                {isLedger
                  ? `secp256k1 · ${shortAddr(p.identity?.ledgerAddress ?? "")} · ${t("每次签名需设备确认")}`
                  : t("secp256k1 · EIP-191 · 私钥永不离开硬件")}
              </div>
            </div>
            {isLedger ? (
              <span className="pill jade">{t("使用中")}</span>
            ) : (
              <button className="btn-ghost" onClick={() => setLedgerModal(true)}>
                {t("绑定 Ledger")}
              </button>
            )}
          </div>
        </div>
        {isLedger && (
          <div style={{ fontSize: 11.5, color: "var(--amber)", background: "var(--amber-bg)", border: "1px solid var(--amber-border)", borderRadius: 9, padding: "10px 14px", marginBottom: 16, lineHeight: 1.6 }}>
            {t("使用 Ledger 时，每封签名邮件发送前需要：连接设备 → 解锁 → 打开 Ethereum app → 在设备上确认。")}
          </div>
        )}
        {error && <div className="form-error" style={{ marginBottom: 16 }}>{error}</div>}

        <div className="section-label" style={{ marginTop: 16 }}>
          {t("可信联系人（已记录密钥指纹 / 地址）")}
        </div>
        {p.trusted.length === 0 ? (
          <div className="card-list" style={{ padding: "22px 18px", fontSize: 12.5, color: "var(--mut)", lineHeight: 1.6 }}>
            {t("还没有可信联系人。当你收到签名有效的邮件时，可在右侧验证面板将对方加入可信——之后任何冒充该联系人的邮件都会被标红。")}
          </div>
        ) : (
          <div className="card-list">
            {p.trusted.map((c) => (
              <div className="card-row" key={c.email}>
                <div style={{ paddingTop: 1 }}>
                  <Seal trust="verified" size={26} />
                </div>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: 13, fontWeight: 600, color: "var(--ink-2)" }}>
                    {c.name}
                    {c.org ? <span style={{ fontWeight: 400, color: "var(--mut-3)" }}> · {c.org}</span> : null}
                  </div>
                  <div style={{ fontFamily: "var(--mono)", fontSize: 11, color: "var(--mut)", marginTop: 2, wordBreak: "break-all" }}>
                    {c.email} · {c.fingerprint.startsWith("0x") ? shortAddr(c.fingerprint) : shortFpr(c.fingerprint)}
                  </div>
                </div>
                <div style={{ textAlign: "right" }}>
                  <div style={{ fontSize: 11.5, color: "var(--ink-3)", fontWeight: 500 }}>{t("自")} {c.since}</div>
                  <div style={{ fontSize: 10.5, color: "var(--mut-3)" }}>{t("{n} 封已验证", { n: c.verifiedCount })}</div>
                </div>
                <button className="icon-btn" title={t("移除可信")} onClick={() => p.onRemoveTrusted(c.email)}>
                  ×
                </button>
              </div>
            ))}
          </div>
        )}
        <div className="section-label" style={{ marginTop: 30 }}>
          {t("关于与更新")}
        </div>
        <div className="card-list" style={{ padding: "16px 18px" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 14 }}>
            <AppIcon className="about-icon" />
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontSize: 13.5, fontWeight: 600, color: "var(--ink-2)" }}>{t("SealMail 信印")}</div>
              {updated ? (
                <div style={{ fontSize: 11.5, color: "#1E6B49", marginTop: 2 }}>✓ {t("已是最新版本")}</div>
              ) : updateInfo?.available ? (
                <div style={{ fontSize: 11.5, color: "var(--amber)", marginTop: 2 }}>
                  ↓ {t("新版本 v{v} 可用", { v: updateInfo.latestVersion })}
                </div>
              ) : (
                <div style={{ fontSize: 11.5, color: "var(--mut)", marginTop: 2, fontFamily: "var(--mono)" }}>
                  {t("版本")} {__APP_VERSION__}
                </div>
              )}
            </div>
            {updateInfo?.available && updateInfo.manual ? (
              <button className="btn-ghost" onClick={handleManualDownload}>
                {t("打开下载页面")}
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
                <span>{updateProgress.phase === "installing" ? t("正在安装…") : t("正在下载更新…")}</span>
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
              <div style={{ fontSize: 13, fontWeight: 600, color: "var(--ink-2)" }}>{t("点击关闭按钮时")}</div>
              <div style={{ fontSize: 11.5, color: "var(--mut)", marginTop: 2, lineHeight: 1.5 }}>
                {t("隐藏窗口后应用继续在后台运行，点击程序坞图标重新打开（macOS 常规行为）")}
              </div>
            </div>
            <select
              className="select"
              style={{ width: 130 }}
              value={closeBehavior ?? "hide"}
              disabled={closeBehavior === null}
              onChange={(e) => void handleCloseBehavior(e.target.value as "hide" | "quit")}
            >
              <option value="hide">{t("隐藏窗口")}</option>
              <option value="quit">{t("退出应用")}</option>
            </select>
          </div>

          <div
            style={{
              display: "flex", alignItems: "center", gap: 14, marginTop: 16,
              paddingTop: 16, borderTop: "1px solid var(--border-soft)",
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontSize: 13, fontWeight: 600, color: "var(--ink-2)" }}>{t("界面语言")}</div>
              <div style={{ fontSize: 11.5, color: "var(--mut)", marginTop: 2, lineHeight: 1.5 }}>
                {t("界面文案与系统通知的语言；「跟随系统」按系统语言自动选择")}
              </div>
            </div>
            <select
              className="select"
              style={{ width: 130 }}
              value={langPref ?? "system"}
              disabled={langPref === null}
              onChange={(e) => void handleLanguage(e.target.value as LangPref)}
            >
              <option value="system">{t("跟随系统")}</option>
              <option value="zh">中文</option>
              <option value="en">English</option>
            </select>
          </div>

          <div
            style={{
              display: "flex", alignItems: "center", gap: 14, marginTop: 16,
              paddingTop: 16, borderTop: "1px solid var(--border-soft)",
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontSize: 13, fontWeight: 600, color: "var(--ink-2)" }}>{t("外观主题")}</div>
              <div style={{ fontSize: 11.5, color: "var(--mut)", marginTop: 2, lineHeight: 1.5 }}>
                {t("浅色、深色或跟随系统外观")}
              </div>
            </div>
            <select
              className="select"
              style={{ width: 130 }}
              value={themePref ?? "system"}
              disabled={themePref === null}
              onChange={(e) => void handleTheme(e.target.value as ThemePref)}
            >
              <option value="system">{t("跟随系统")}</option>
              <option value="light">{t("浅色")}</option>
              <option value="dark">{t("深色")}</option>
            </select>
          </div>

          <div
            style={{
              display: "flex", alignItems: "center", gap: 14, marginTop: 16,
              paddingTop: 16, borderTop: "1px solid var(--border-soft)",
            }}
          >
            <div style={{ flex: 1, minWidth: 0 }}>
              <div style={{ fontSize: 13, fontWeight: 600, color: "var(--ink-2)" }}>{t("新邮件系统通知")}</div>
              <div style={{ fontSize: 11.5, color: "var(--mut)", marginTop: 2, lineHeight: 1.5 }}>
                {t("窗口在后台或隐藏时，收到新邮件弹系统横幅")}
              </div>
            </div>
            <select
              className="select"
              style={{ width: 130 }}
              value={notify === null ? "on" : notify ? "on" : "off"}
              disabled={notify === null}
              onChange={(e) => void handleNotify(e.target.value === "on")}
            >
              <option value="on">{t("开启")}</option>
              <option value="off">{t("关闭")}</option>
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
