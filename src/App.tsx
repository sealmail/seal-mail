import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import * as api from "./api";
import { AccountModal } from "./components/AccountModal";
import { ComposeModal, type ComposePrefill } from "./components/ComposeModal";
import { FiltersModal } from "./components/FiltersModal";
import { KeysView } from "./components/KeysView";
import { MailList } from "./components/MailList";
import { MessageView } from "./components/MessageView";
import { Onboarding } from "./components/Onboarding";
import { ProfileSlideOver } from "./components/ProfileSlideOver";
import { RiskModal } from "./components/RiskModal";
import { RISK_FOLDER, Sidebar } from "./components/Sidebar";
import { VerifyRail } from "./components/VerifyRail";
import type { AppStateView, EmailFull, EmailMeta, FilterRule, FolderInfo, IdentityInfo } from "./types";
import "./styles.css";

function isRisky(m: EmailMeta) {
  return !!m.risk || m.trust === "tampered" || m.trust === "impersonation";
}

export default function App() {
  const [state, setState] = useState<AppStateView | null>(null);
  const [bootError, setBootError] = useState<string | null>(null);
  const [accountId, setAccountId] = useState("");
  const [folders, setFolders] = useState<FolderInfo[]>([]);
  const [folder, setFolder] = useState("INBOX");
  const [messages, setMessages] = useState<EmailMeta[]>([]);
  const [inboxMetas, setInboxMetas] = useState<EmailMeta[]>([]);
  const [selected, setSelected] = useState<EmailFull | null>(null);
  const [view, setView] = useState<"mail" | "keys">("mail");
  const [search, setSearch] = useState("");
  const [loading, setLoading] = useState(false);
  const [listError, setListError] = useState<string | null>(null);

  const [composeOpen, setComposeOpen] = useState(false);
  const [composePrefill, setComposePrefill] = useState<ComposePrefill | undefined>();
  const [accountModal, setAccountModal] = useState(false);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [profileOpen, setProfileOpen] = useState(false);
  const [riskOpen, setRiskOpen] = useState(false);
  const [newFolderOpen, setNewFolderOpen] = useState(false);
  const [newFolderName, setNewFolderName] = useState("");
  const [newFolderErr, setNewFolderErr] = useState<string | null>(null);

  const fetchSeq = useRef(0);

  const accounts = state?.accounts ?? [];
  const trusted = state?.trusted ?? [];
  const hasAccounts = accounts.length > 0;

  // ── 初始化 ──
  useEffect(() => {
    api
      .getState()
      .then((s) => {
        setState(s);
        setAccountId(s.accounts[0]?.id ?? "");
      })
      .catch((e) => setBootError(String(e)));
  }, []);

  const refreshFolders = useCallback(async (accId: string) => {
    const fs = await api.listFolders(accId);
    const withRisk: FolderInfo[] = [
      ...fs.filter((f) => f.name === "INBOX").map((f) => ({ ...f, display: "收件箱" })),
      { name: RISK_FOLDER, display: "高风险" },
      ...fs.filter((f) => f.name !== "INBOX"),
    ];
    setFolders(withRisk);
  }, []);

  // ── 切账户：拉目录 ──
  useEffect(() => {
    if (!accountId) return;
    setFolder("INBOX");
    setSelected(null);
    setListError(null);
    refreshFolders(accountId).catch((e) => setListError(String(e)));
  }, [accountId, refreshFolders]);

  // ── 拉邮件 ──
  const loadMessages = useCallback(async () => {
    if (!accountId) return;
    const seq = ++fetchSeq.current;
    setLoading(true);
    setListError(null);
    try {
      const realFolder = folder === RISK_FOLDER ? "INBOX" : folder;
      let metas = await api.fetchMessages(accountId, realFolder);
      if (seq !== fetchSeq.current) return;
      if (realFolder === "INBOX") setInboxMetas(metas);
      if (folder === RISK_FOLDER) metas = metas.filter(isRisky);
      metas = [...metas].sort((a, b) => b.timestamp - a.timestamp);
      setMessages(metas);
      setSelected((prev) => (prev && metas.some((m) => m.uid === prev.meta.uid) ? prev : null));
    } catch (e) {
      if (seq === fetchSeq.current) {
        setMessages([]);
        setListError(String(e));
      }
    } finally {
      if (seq === fetchSeq.current) setLoading(false);
    }
  }, [accountId, folder]);

  useEffect(() => {
    loadMessages();
  }, [loadMessages]);

  // ── 选中邮件 ──
  async function selectMail(m: EmailMeta) {
    try {
      const full = await api.getMessage(m.accountId, m.folder, m.uid);
      setSelected(full);
      setView("mail");
      if (m.unread) {
        api.setRead(m.accountId, m.folder, m.uid, true).catch(() => {});
        setMessages((ms) => ms.map((x) => (x.uid === m.uid ? { ...x, unread: false } : x)));
        setInboxMetas((ms) => ms.map((x) => (x.uid === m.uid ? { ...x, unread: false } : x)));
      }
    } catch (e) {
      setListError(String(e));
    }
  }

  // ── 搜索（本地过滤：发件人/主题/摘要/地址）──
  const shownMessages = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return messages;
    return messages.filter(
      (m) =>
        m.fromName.toLowerCase().includes(q) ||
        m.fromAddr.toLowerCase().includes(q) ||
        m.subject.toLowerCase().includes(q) ||
        m.preview.toLowerCase().includes(q)
    );
  }, [messages, search]);

  const riskCount = useMemo(() => inboxMetas.filter(isRisky).length, [inboxMetas]);
  const inboxUnread = useMemo(() => inboxMetas.filter((m) => m.unread).length, [inboxMetas]);
  const folderTitle = folders.find((f) => f.name === folder)?.display ?? folder;

  async function handleMove(target: string) {
    if (!selected) return;
    try {
      await api.moveMessage(selected.meta.accountId, selected.meta.folder, selected.meta.uid, target);
      setSelected(null);
      loadMessages();
    } catch (e) {
      setListError(String(e));
    }
  }

  async function handleDelete() {
    if (!selected) return;
    try {
      await api.deleteMessage(selected.meta.accountId, selected.meta.folder, selected.meta.uid);
      setSelected(null);
      loadMessages();
    } catch (e) {
      setListError(String(e));
    }
  }

  function handleReply() {
    if (!selected) return;
    setComposePrefill({
      to: selected.meta.fromAddr,
      subject: selected.meta.subject.startsWith("Re:") ? selected.meta.subject : `Re: ${selected.meta.subject}`,
      body: `\n\n----- 原始邮件 -----\n${selected.bodyText}`,
    });
    setComposeOpen(true);
  }

  function handleForward() {
    if (!selected) return;
    setComposePrefill({
      to: "",
      subject: selected.meta.subject.startsWith("Fwd:") ? selected.meta.subject : `Fwd: ${selected.meta.subject}`,
      body: `\n\n----- 转发邮件（发件人 ${selected.meta.fromName} <${selected.meta.fromAddr}>）-----\n${selected.bodyText}`,
    });
    setComposeOpen(true);
  }

  async function handleTrustSender() {
    if (!selected || selected.verify.status !== "signedUnknown") return;
    try {
      const list = await api.trustSender(selected.meta.fromName, selected.meta.fromAddr, selected.verify.fingerprint);
      setState((s) => (s ? { ...s, trusted: list } : s));
      await loadMessages();
      const refreshed = await api
        .getMessage(selected.meta.accountId, selected.meta.folder, selected.meta.uid)
        .catch(() => null);
      if (refreshed) setSelected(refreshed);
    } catch (e) {
      setListError(String(e));
    }
  }

  async function handleRemoveTrusted(email: string) {
    const list = await api.removeTrusted(email);
    setState((s) => (s ? { ...s, trusted: list } : s));
  }

  function handleIdentityChanged(info: IdentityInfo) {
    setState((s) => (s ? { ...s, identity: info } : s));
  }

  async function handleCreateFolder() {
    const name = newFolderName.trim();
    if (!name) {
      setNewFolderErr("请输入目录名称");
      return;
    }
    try {
      await api.createFolder(accountId, name);
      await refreshFolders(accountId);
      setNewFolderOpen(false);
      setNewFolderName("");
      setNewFolderErr(null);
    } catch (e) {
      setNewFolderErr(String(e));
    }
  }

  const ledgerMode = state?.identity.mode === "ledger";

  return (
    <div className="app">
      <div className="titlebar" data-tauri-drag-region>
        <div className="brand" data-tauri-drag-region>
          <div className="brand-seal" data-tauri-drag-region>印</div>
          <span className="brand-name" data-tauri-drag-region>SealMail</span>
          <span className="brand-cn" data-tauri-drag-region>信印</span>
        </div>
        <div className="search-wrap" data-tauri-drag-region>
          {hasAccounts && (
            <div className="search">
              <svg width="13" height="13" viewBox="0 0 13 13" fill="none">
                <circle cx="5.5" cy="5.5" r="4" stroke="#B3AEA2" strokeWidth="1.4" />
                <path d="M8.5 8.5l3 3" stroke="#B3AEA2" strokeWidth="1.4" strokeLinecap="round" />
              </svg>
              <input placeholder="搜索邮件、发件人或地址…" value={search} onChange={(e) => setSearch(e.target.value)} />
            </div>
          )}
        </div>
        {hasAccounts && (
          <button
            className="btn-primary"
            onClick={() => {
              setComposePrefill(undefined);
              setComposeOpen(true);
            }}
          >
            <span style={{ fontSize: 15, lineHeight: 1, marginTop: -1 }}>✎</span> 写邮件
          </button>
        )}
      </div>

      {bootError && <div className="demo-banner">初始化失败：{bootError}</div>}

      {!hasAccounts ? (
        // ── 首次使用引导 ──
        view === "keys" && state ? (
          <div className="main">
            <KeysView
              identity={state.identity}
              trusted={trusted}
              onBack={() => setView("mail")}
              onRemoveTrusted={handleRemoveTrusted}
              onIdentityChanged={handleIdentityChanged}
            />
          </div>
        ) : (
          <Onboarding
            identity={state?.identity ?? null}
            onAddAccount={() => setAccountModal(true)}
            onOpenKeys={() => setView("keys")}
          />
        )
      ) : (
        <div className="main">
          <Sidebar
            identity={state?.identity ?? null}
            accounts={accounts}
            currentAccountId={accountId}
            folders={folders}
            currentFolder={folder}
            riskCount={riskCount}
            inboxUnread={inboxUnread}
            view={view}
            ledgerMode={ledgerMode}
            onSelectAccount={(id) => {
              setAccountId(id);
              setView("mail");
            }}
            onSelectFolder={(f) => {
              setFolder(f);
              setView("mail");
            }}
            onOpenKeys={() => setView("keys")}
            onAddAccount={() => setAccountModal(true)}
            onNewFolder={() => setNewFolderOpen(true)}
            onOpenFilters={() => setFiltersOpen(true)}
          />

          {view === "keys" ? (
            <KeysView
              identity={state?.identity ?? null}
              trusted={trusted}
              onBack={() => setView("mail")}
              onRemoveTrusted={handleRemoveTrusted}
              onIdentityChanged={handleIdentityChanged}
            />
          ) : (
            <>
              <MailList
                title={folderTitle}
                messages={shownMessages}
                selectedUid={selected?.meta.uid ?? null}
                loading={loading}
                error={listError}
                onSelect={selectMail}
                onRefresh={loadMessages}
              />
              <MessageView
                mail={selected}
                folders={folders}
                onReply={handleReply}
                onForward={handleForward}
                onMove={handleMove}
                onDelete={handleDelete}
                onShowRisk={() => setRiskOpen(true)}
              />
              <VerifyRail
                mail={selected}
                onOpenProfile={() => setProfileOpen(true)}
                onTrustSender={handleTrustSender}
              />
            </>
          )}
        </div>
      )}

      {composeOpen && hasAccounts && (
        <ComposeModal
          accounts={accounts}
          currentAccountId={accountId}
          identity={state?.identity ?? null}
          prefill={composePrefill}
          onClose={() => setComposeOpen(false)}
        />
      )}

      {accountModal && (
        <AccountModal
          onClose={() => setAccountModal(false)}
          onAdded={async (acc) => {
            setAccountModal(false);
            const s = await api.getState();
            setState(s);
            setAccountId(acc.id);
            setView("mail");
          }}
        />
      )}

      {filtersOpen && state && (
        <FiltersModal
          filters={state.filters}
          folders={folders}
          accounts={accounts}
          currentAccountId={accountId}
          onClose={() => setFiltersOpen(false)}
          onChanged={(rules: FilterRule[]) => setState((s) => (s ? { ...s, filters: rules } : s))}
          onApplied={loadMessages}
        />
      )}

      {profileOpen && selected && (
        <ProfileSlideOver mail={selected} trusted={trusted} onClose={() => setProfileOpen(false)} />
      )}

      {riskOpen && selected && <RiskModal mail={selected} onClose={() => setRiskOpen(false)} />}

      {newFolderOpen && (
        <div className="overlay" onClick={() => setNewFolderOpen(false)}>
          <div className="modal" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <span className="title">新建目录</span>
              <button className="modal-close" onClick={() => setNewFolderOpen(false)}>
                ×
              </button>
            </div>
            <div className="modal-body" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              <div className="field">
                <label>目录名称</label>
                <input
                  className="input"
                  placeholder="例如：重要客户 / 发票 / 通知"
                  value={newFolderName}
                  autoFocus
                  onChange={(e) => setNewFolderName(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && handleCreateFolder()}
                />
              </div>
              <div style={{ fontSize: 11, color: "#A39E91", lineHeight: 1.6 }}>
                IMAP 账户会在邮件服务器上创建真实目录；POP3 账户使用本地目录。配合「过滤规则」可把某一类邮件自动归入该目录。
              </div>
              {newFolderErr && <div className="form-error">{newFolderErr}</div>}
            </div>
            <div className="modal-foot">
              <span />
              <button className="btn-primary" style={{ height: 40 }} onClick={handleCreateFolder}>
                创建
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
