import { useI18n } from "../i18n";
import type { Account, FolderInfo, IdentityInfo } from "../types";

export const RISK_FOLDER = "__risk__";
export const DRAFTS_FOLDER = "__drafts__";
export const UNIFIED_FOLDER = "__unified__";

const FOLDER_ICONS: Record<string, string> = {
  [UNIFIED_FOLDER]: "▦",
  INBOX: "▤",
  [RISK_FOLDER]: "◈",
  [DRAFTS_FOLDER]: "✎",
};

function folderIcon(name: string, display: string) {
  if (FOLDER_ICONS[name]) return FOLDER_ICONS[name];
  const d = display.toLowerCase();
  if (d.includes("sent") || display.includes("已发送") || display.includes("发件")) return "↗";
  if (d.includes("draft") || display.includes("草稿")) return "✎";
  if (d.includes("junk") || d.includes("spam") || display.includes("垃圾")) return "⊘";
  if (d.includes("trash") || d.includes("deleted") || display.includes("已删除")) return "□";
  if (d.includes("archive") || display.includes("归档")) return "▣";
  if (d.includes("notification") || d.includes("notice") || display.includes("通知")) return "🔔";
  return "▢";
}

interface Props {
  width?: number;
  identity: IdentityInfo | null;
  accounts: Account[];
  currentAccountId: string;
  folders: FolderInfo[];
  currentFolder: string;
  riskCount: number;
  inboxUnread: number;
  draftCount: number;
  view: "mail" | "keys";
  ledgerMode: boolean;
  onSelectAccount: (id: string) => void;
  onSelectFolder: (name: string) => void;
  onOpenKeys: () => void;
  onAddAccount: () => void;
  onRemoveAccount: (account: Account) => void;
  onNewFolder: () => void;
  onDeleteFolder: (folder: FolderInfo) => void;
  onOpenFilters: () => void;
}

export function Sidebar(p: Props) {
  const t = useI18n();
  return (
    <div className="sidebar" style={{ width: p.width }}>
      <div className="sidebar-scroll">
        <div className="side-label">{t("邮箱")}</div>
        {p.folders.map((f) => {
          const active = p.view === "mail" && p.currentFolder === f.name;
          const isInbox = f.name === "INBOX";
          const isRisk = f.name === RISK_FOLDER;
          const deletable = !f.role && ![UNIFIED_FOLDER, "INBOX", RISK_FOLDER, DRAFTS_FOLDER].includes(f.name);
          const count = isRisk ? p.riskCount : isInbox ? p.inboxUnread : f.name === DRAFTS_FOLDER ? p.draftCount : 0;
          return (
            <div key={f.name} className="side-row">
              <button
                className={`side-item${active ? " active" : ""}${deletable ? " has-action" : ""}`}
                onClick={() => p.onSelectFolder(f.name)}
              >
                <span className="icon">{folderIcon(f.name, f.display)}</span>
                <span className="label">{t(f.display)}</span>
                {count > 0 && <span className={`count${isRisk ? " red" : ""}`}>{count}</span>}
              </button>
              {deletable && (
                <button
                  className="side-action"
                  title={t("删除目录 {name}", { name: t(f.display) })}
                  onClick={(e) => {
                    e.stopPropagation();
                    p.onDeleteFolder(f);
                  }}
                >
                  ×
                </button>
              )}
            </div>
          );
        })}
        <button className="side-add" onClick={p.onNewFolder}>
          {t("+ 新建目录")}
        </button>

        <div style={{ height: 10 }} />
        <div className="side-label">{t("整理")}</div>
        <button className="side-item" onClick={p.onOpenFilters}>
          <span className="icon">⧉</span>
          <span className="label">{t("过滤规则")}</span>
        </button>

        <div style={{ height: 10 }} />
        <div className="side-label">{t("已连接账户")}</div>
        {p.accounts.map((a) => (
          <div key={a.id} className="account-row-wrap">
            <div
              className={`account-row${a.id === p.currentAccountId ? " active" : ""}`}
              onClick={() => p.onSelectAccount(a.id)}
            >
              <div className="dot" style={{ background: a.id === p.currentAccountId ? "var(--jade)" : "var(--mut-4)" }} />
              <div style={{ minWidth: 0, flex: 1 }}>
                <div className="addr">{a.email}</div>
                <div className="sys">{`${a.protocol === "imap" ? "IMAP" : "POP3"} · ${a.label}`}</div>
              </div>
            </div>
            <button
              className="account-action"
              title={t("删除账户 {name}", { name: a.email })}
              onClick={(e) => {
                e.stopPropagation();
                p.onRemoveAccount(a);
              }}
            >
              ×
            </button>
          </div>
        ))}
        <button className="side-add" onClick={p.onAddAccount}>
          {t("+ 添加账户")}
        </button>
      </div>

      <div className="sidebar-footer">
        <button className={`side-item${p.view === "keys" ? " active" : ""}`} onClick={p.onOpenKeys}>
          <span className="icon">⊟</span>
          <span className="label">{t("设置")}</span>
          {p.identity && <span className="key-status" title={p.ledgerMode ? t("Ledger 已绑定") : t("本地密钥已就绪")} />}
        </button>
      </div>
    </div>
  );
}
