import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
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
import { DraftsPane } from "./components/DraftsPane";
import { DRAFTS_FOLDER, RISK_FOLDER, Sidebar } from "./components/Sidebar";
import { VerifyRail } from "./components/VerifyRail";
import type { AppStateView, Draft, EmailFull, EmailMeta, FilterRule, FolderInfo, IdentityInfo } from "./types";
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
  // 验证面板默认折叠成图标条，用户主动展开后记住偏好
  const [railOpen, setRailOpen] = useState(() => localStorage.getItem("sealmail.railOpen") === "1");
  const [search, setSearch] = useState("");
  const [unreadOnly, setUnreadOnly] = useState(false);
  // 界面缩放（Cmd+/-/0），WebKit 支持非标准 zoom 属性
  const [zoom, setZoom] = useState(() => {
    const z = parseFloat(localStorage.getItem("sealmail.zoom") ?? "1");
    return Number.isFinite(z) && z >= 0.7 && z <= 1.6 ? z : 1;
  });
  const [loading, setLoading] = useState(false);
  const [listError, setListError] = useState<string | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  const [composeOpen, setComposeOpen] = useState(false);
  const [composePrefill, setComposePrefill] = useState<ComposePrefill | undefined>();
  const [composeDraft, setComposeDraft] = useState<Draft | undefined>();
  const [drafts, setDrafts] = useState<Draft[]>([]);
  const [accountModal, setAccountModal] = useState(false);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [profileOpen, setProfileOpen] = useState(false);
  const [riskOpen, setRiskOpen] = useState(false);
  const [newFolderOpen, setNewFolderOpen] = useState(false);
  const [newFolderName, setNewFolderName] = useState("");
  const [newFolderErr, setNewFolderErr] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

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
      { name: DRAFTS_FOLDER, display: "草稿" },
      ...fs.filter((f) => f.name !== "INBOX"),
    ];
    setFolders(withRisk);
  }, []);

  const loadDrafts = useCallback(() => {
    api.listDrafts().then(setDrafts).catch((e) => console.error("读取草稿失败", e));
  }, []);

  useEffect(loadDrafts, [loadDrafts]);

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
    if (folder === DRAFTS_FOLDER) return; // 草稿是本地数据，不走邮件拉取
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

  // ── 新邮件推送（后端 IMAP IDLE / POP3 轮询发出 new-mail 事件）──
  useEffect(() => {
    const unlisten = listen<{ accountId: string }>("new-mail", (e) => {
      if (e.payload.accountId === accountId) loadMessages();
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [accountId, loadMessages]);

  // ── 界面缩放（持久化）──
  useEffect(() => {
    (document.body.style as CSSStyleDeclaration & { zoom: string }).zoom = String(zoom);
    localStorage.setItem("sealmail.zoom", String(zoom));
  }, [zoom]);

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

  // ── 搜索（本地过滤：发件人/主题/摘要/地址）+ 未读过滤 ──
  const shownMessages = useMemo(() => {
    const q = search.trim().toLowerCase();
    let list = messages;
    if (unreadOnly) list = list.filter((m) => m.unread);
    if (!q) return list;
    return list.filter(
      (m) =>
        m.fromName.toLowerCase().includes(q) ||
        m.fromAddr.toLowerCase().includes(q) ||
        m.subject.toLowerCase().includes(q) ||
        m.preview.toLowerCase().includes(q)
    );
  }, [messages, search, unreadOnly]);

  const riskCount = useMemo(() => inboxMetas.filter(isRisky).length, [inboxMetas]);
  const inboxUnread = useMemo(() => inboxMetas.filter((m) => m.unread).length, [inboxMetas]);
  const listUnread = useMemo(() => messages.filter((m) => m.unread).length, [messages]);
  const folderTitle = folders.find((f) => f.name === folder)?.display ?? folder;

  function markLocal(uids: number[], unread: boolean) {
    const set = new Set(uids);
    const patch = (ms: EmailMeta[]) => ms.map((x) => (set.has(x.uid) ? { ...x, unread } : x));
    setMessages(patch);
    setInboxMetas(patch);
  }

  async function handleMarkUnread() {
    if (!selected) return;
    try {
      await api.setRead(selected.meta.accountId, selected.meta.folder, selected.meta.uid, false);
      markLocal([selected.meta.uid], true);
    } catch (e) {
      setListError(String(e));
    }
  }

  async function handleMarkAllRead() {
    const uids = messages.filter((m) => m.unread).map((m) => m.uid);
    if (uids.length === 0) return;
    const realFolder = folder === RISK_FOLDER ? "INBOX" : folder;
    try {
      await api.markRead(accountId, realFolder, uids);
      markLocal(uids, false);
    } catch (e) {
      setListError(String(e));
    }
  }

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

  // 当前选中邮件所在目录是否是回收站（回收站内删除 = 物理删除，需确认）
  const selectedInTrash = !!selected && folders.find((f) => f.name === selected.meta.folder)?.role === "trash";

  function handleDelete() {
    if (!selected) return;
    if (selectedInTrash) {
      setConfirmDelete(true);
      return;
    }
    doDelete(false);
  }

  async function doDelete(permanent: boolean) {
    if (!selected) return;
    setConfirmDelete(false);
    try {
      await api.deleteMessage(selected.meta.accountId, selected.meta.folder, selected.meta.uid, permanent);
      setSelected(null);
      loadMessages();
      // 第一次软删除可能刚在服务器上创建了回收站目录，刷新目录列表
      if (!permanent) refreshFolders(accountId).catch(() => {});
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

  function handleReplyAll() {
    if (!selected) return;
    const own = (accounts.find((a) => a.id === selected.meta.accountId)?.email ?? "").toLowerCase();
    const notSelf = (a: string) => a.trim() && a.trim().toLowerCase() !== own;
    // 回复全部：To = 原发件人 + 原 To（去掉自己），Cc = 原 Cc（去掉自己）
    const to = [selected.meta.fromAddr, ...selected.to.filter(notSelf)].filter(
      (v, i, arr) => arr.findIndex((x) => x.toLowerCase() === v.toLowerCase()) === i
    );
    setComposePrefill({
      to: to.join(", "),
      cc: selected.cc.filter(notSelf).join(", "),
      subject: selected.meta.subject.startsWith("Re:") ? selected.meta.subject : `Re: ${selected.meta.subject}`,
      body: `\n\n----- 原始邮件 -----\n${selected.bodyText}`,
    });
    setComposeOpen(true);
  }

  function toggleRail() {
    setRailOpen((o) => {
      localStorage.setItem("sealmail.railOpen", o ? "0" : "1");
      return !o;
    });
  }

  // ── 全局键盘快捷键 ──
  const anyModalOpen =
    composeOpen || accountModal || filtersOpen || profileOpen || riskOpen || newFolderOpen || confirmDelete;
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const meta = e.metaKey || e.ctrlKey;
      // 缩放任何时候都可用
      if (meta && (e.key === "=" || e.key === "+")) {
        e.preventDefault();
        setZoom((z) => Math.min(1.6, Math.round((z + 0.1) * 10) / 10));
        return;
      }
      if (meta && e.key === "-") {
        e.preventDefault();
        setZoom((z) => Math.max(0.7, Math.round((z - 0.1) * 10) / 10));
        return;
      }
      if (meta && e.key === "0") {
        e.preventDefault();
        setZoom(1);
        return;
      }
      if (anyModalOpen || !hasAccounts) return;
      if (meta && e.key.toLowerCase() === "n") {
        e.preventDefault();
        setComposePrefill(undefined);
        setComposeDraft(undefined);
        setComposeOpen(true);
        return;
      }
      if (meta && e.key.toLowerCase() === "f") {
        e.preventDefault();
        searchRef.current?.focus();
        return;
      }
      if (meta && e.key.toLowerCase() === "r" && selected) {
        e.preventDefault();
        if (e.shiftKey) handleReplyAll();
        else handleReply();
        return;
      }
      // 以下快捷键在输入框聚焦时不生效
      const t = e.target as HTMLElement;
      if (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable) return;
      if (view !== "mail") return;
      if ((e.key === "Delete" || e.key === "Backspace") && selected) {
        e.preventDefault();
        handleDelete();
        return;
      }
      if (e.key === "ArrowDown" || e.key === "j" || e.key === "ArrowUp" || e.key === "k") {
        e.preventDefault();
        const down = e.key === "ArrowDown" || e.key === "j";
        if (shownMessages.length === 0) return;
        const idx = selected ? shownMessages.findIndex((m) => m.uid === selected.meta.uid) : -1;
        const next = idx < 0 ? 0 : Math.min(shownMessages.length - 1, Math.max(0, idx + (down ? 1 : -1)));
        if (shownMessages[next] && shownMessages[next].uid !== selected?.meta.uid) selectMail(shownMessages[next]);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

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
              <input ref={searchRef} placeholder="搜索邮件、发件人或地址…" value={search} onChange={(e) => setSearch(e.target.value)} />
            </div>
          )}
        </div>
        {hasAccounts && (
          <button
            className="btn-primary"
            onClick={() => {
              setComposePrefill(undefined);
              setComposeDraft(undefined);
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
            draftCount={drafts.filter((d) => d.accountId === accountId).length}
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
          ) : folder === DRAFTS_FOLDER ? (
            <DraftsPane
              drafts={drafts.filter((d) => d.accountId === accountId)}
              onOpen={(d) => {
                setComposeDraft(d);
                setComposePrefill(undefined);
                setComposeOpen(true);
              }}
              onDelete={async (d) => {
                try {
                  await api.deleteDraft(d.id);
                  loadDrafts();
                } catch (e) {
                  setListError(String(e));
                }
              }}
            />
          ) : (
            <>
              <MailList
                title={folderTitle}
                messages={shownMessages}
                selectedUid={selected?.meta.uid ?? null}
                loading={loading}
                error={listError}
                unreadOnly={unreadOnly}
                unreadCount={listUnread}
                onToggleUnreadOnly={() => setUnreadOnly((v) => !v)}
                onMarkAllRead={handleMarkAllRead}
                onSelect={selectMail}
                onRefresh={loadMessages}
              />
              <MessageView
                mail={selected}
                folders={folders}
                onReply={handleReply}
                onReplyAll={handleReplyAll}
                onForward={handleForward}
                onMove={handleMove}
                onDelete={handleDelete}
                onShowRisk={() => setRiskOpen(true)}
                onTrustSender={handleTrustSender}
                onMarkUnread={handleMarkUnread}
              />
              <VerifyRail
                mail={selected}
                open={railOpen}
                onToggle={toggleRail}
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
          draft={composeDraft}
          onClose={() => {
            setComposeOpen(false);
            setComposeDraft(undefined);
            loadDrafts();
          }}
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

      {confirmDelete && selected && (
        <div className="overlay" onClick={() => setConfirmDelete(false)}>
          <div className="modal" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <span className="title">永久删除</span>
              <button className="modal-close" onClick={() => setConfirmDelete(false)}>
                ×
              </button>
            </div>
            <div className="modal-body" style={{ fontSize: 13, lineHeight: 1.7, color: "#6E6A5F" }}>
              「{selected.meta.subject}」已在回收站中，继续删除将<b style={{ color: "#9A2C1D" }}>从服务器上永久移除，无法恢复</b>。
            </div>
            <div className="modal-foot">
              <button className="btn-ghost" style={{ height: 40 }} onClick={() => setConfirmDelete(false)}>
                取消
              </button>
              <button
                className="btn-primary"
                style={{ height: 40, background: "#9A2C1D" }}
                onClick={() => doDelete(true)}
              >
                永久删除
              </button>
            </div>
          </div>
        </div>
      )}

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
