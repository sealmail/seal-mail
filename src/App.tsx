import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import * as api from "./api";
import { AccountModal } from "./components/AccountModal";
import { ComposeModal, type ComposePrefill } from "./components/ComposeModal";
import { FiltersModal } from "./components/FiltersModal";
import { HtmlBody } from "./components/HtmlBody";
import { KeysView } from "./components/KeysView";
import { MailList } from "./components/MailList";
import { MessageView } from "./components/MessageView";
import { Onboarding } from "./components/Onboarding";
import { ProfileSlideOver } from "./components/ProfileSlideOver";
import { RiskModal } from "./components/RiskModal";
import { DraftsPane } from "./components/DraftsPane";
import { DRAFTS_FOLDER, RISK_FOLDER, UNIFIED_FOLDER, Sidebar } from "./components/Sidebar";
import { VerifyRail } from "./components/VerifyRail";
import { Seal } from "./components/Seal";
import type { AppStateView, Draft, EmailFull, EmailMeta, FilterRule, FolderInfo, IdentityInfo } from "./types";
import "./styles.css";

function isRisky(m: EmailMeta) {
  return !!m.risk || m.trust === "tampered" || m.trust === "impersonation";
}

const BUILTIN_FOLDERS: FolderInfo[] = [
  { name: UNIFIED_FOLDER, display: "统一收件箱" },
  { name: "INBOX", display: "收件箱" },
  { name: RISK_FOLDER, display: "高风险" },
  { name: DRAFTS_FOLDER, display: "草稿" },
];

function mailKey(m: Pick<EmailMeta, "accountId" | "folder" | "uid">) {
  return `${m.accountId}/${m.folder}/${m.uid}`;
}

function clamp(n: number, min: number, max: number) {
  return Math.min(max, Math.max(min, n));
}

function PaneResizer({
  title,
  onStart,
  onDrag,
}: {
  title: string;
  onStart: () => void;
  onDrag: (deltaX: number) => void;
}) {
  function onPointerDown(e: React.PointerEvent) {
    e.preventDefault();
    onStart();
    const startX = e.clientX;
    const previousCursor = document.body.style.cursor;
    const previousSelect = document.body.style.userSelect;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    const move = (ev: PointerEvent) => onDrag(ev.clientX - startX);
    const up = () => {
      document.body.style.cursor = previousCursor;
      document.body.style.userSelect = previousSelect;
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up, { once: true });
  }

  return <div className="pane-resizer" title={title} onPointerDown={onPointerDown} />;
}

function PopoutApp({ storageKey }: { storageKey: string }) {
  const [mail] = useState<EmailFull | null>(() => {
    try {
      const raw = localStorage.getItem(storageKey);
      return raw ? (JSON.parse(raw) as EmailFull) : null;
    } catch {
      return null;
    }
  });
  const [htmlMode, setHtmlMode] = useState<boolean | null>(null);

  if (!mail) {
    return (
      <div className="popout-shell">
        <div className="empty-pane">这封邮件窗口的数据已经过期，请从主窗口重新打开。</div>
      </div>
    );
  }

  const hasHtml = !!mail.bodyHtml;
  const signed = mail.verify.status !== "unsigned";
  const showHtml = hasHtml && (htmlMode ?? !signed);

  return (
    <div className="popout-shell">
      <div className="popout-head">
        <div className="popout-subject">{mail.meta.subject}</div>
        <div className="popout-from">
          <Seal trust={mail.meta.trust} size={28} />
          <div style={{ minWidth: 0 }}>
            <div className="msg-fromname">{mail.meta.fromName}</div>
            <div className="msg-addr">{mail.meta.fromAddr}</div>
          </div>
          <span className="msg-date">{mail.meta.dateDisplay}</span>
        </div>
      </div>
      <div className="popout-body">
        {hasHtml && (
          <div className="body-toolbar">
            {signed && showHtml && <span className="body-note">签名校验针对纯文本正文，HTML 版式仅供参考</span>}
            <button className="btn-ghost" style={{ height: 24, padding: "0 10px", fontSize: 11 }} onClick={() => setHtmlMode(!showHtml)}>
              {showHtml ? "查看纯文本" : "查看 HTML 版式"}
            </button>
          </div>
        )}
        {showHtml ? <HtmlBody html={mail.bodyHtml as string} /> : <div className="msg-body">{mail.bodyText || "(无正文)"}</div>}
      </div>
    </div>
  );
}

export default function App() {
  const popoutKey = new URLSearchParams(window.location.search).get("popout");
  if (popoutKey) return <PopoutApp storageKey={popoutKey} />;
  return <MailApp />;
}

function MailApp() {
  const [state, setState] = useState<AppStateView | null>(null);
  const [bootError, setBootError] = useState<string | null>(null);
  const [accountId, setAccountId] = useState("");
  const [folders, setFolders] = useState<FolderInfo[]>(BUILTIN_FOLDERS);
  const [folder, setFolder] = useState("INBOX");
  const [messages, setMessages] = useState<EmailMeta[]>([]);
  const [inboxMetas, setInboxMetas] = useState<EmailMeta[]>([]);
  const [selected, setSelected] = useState<EmailFull | null>(null);
  const [thread, setThread] = useState<EmailMeta[]>([]);
  const [view, setView] = useState<"mail" | "keys">("mail");
  // 验证面板默认折叠成图标条，用户主动展开后记住偏好
  const [railOpen, setRailOpen] = useState(() => localStorage.getItem("sealmail.railOpen") === "1");
  const [search, setSearch] = useState("");
  const [filterMode, setFilterMode] = useState<"all" | "unread" | "flagged">("all");
  const [total, setTotal] = useState(0);
  const [syncing, setSyncing] = useState(false);
  // 界面缩放（Cmd+/-/0），WebKit 支持非标准 zoom 属性
  const [zoom, setZoom] = useState(() => {
    const z = parseFloat(localStorage.getItem("sealmail.zoom") ?? "1");
    return Number.isFinite(z) && z >= 0.7 && z <= 1.6 ? z : 1;
  });
  const [sidebarWidth, setSidebarWidth] = useState(() => {
    const n = Number(localStorage.getItem("sealmail.sidebarWidth") ?? 228);
    return Number.isFinite(n) ? clamp(n, 176, 280) : 228;
  });
  const [listWidth, setListWidth] = useState(() => {
    const n = Number(localStorage.getItem("sealmail.listWidth") ?? 348);
    return Number.isFinite(n) ? clamp(n, 300, 480) : 348;
  });
  const [railWidth, setRailWidth] = useState(() => {
    const n = Number(localStorage.getItem("sealmail.railWidth") ?? 288);
    return Number.isFinite(n) ? clamp(n, 240, 420) : 288;
  });
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [olderExhausted, setOlderExhausted] = useState(false);
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
    const inbox = fs.find((f) => f.name === "INBOX");
    const withRisk: FolderInfo[] = [
      { name: UNIFIED_FOLDER, display: "统一收件箱" },
      inbox ? { ...inbox, display: "收件箱" } : { name: "INBOX", display: "收件箱" },
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
    setThread([]);
    setListError(null);
    refreshFolders(accountId).catch((e) => {
      setFolders(BUILTIN_FOLDERS);
      setListError(String(e));
    });
  }, [accountId, refreshFolders]);

  // ── 拉邮件：本地缓存秒出 → 后台增量同步 → 回填 ──
  const PAGE = 200;
  const AUTO_CACHE_TARGET = 1000;
  const AUTO_BACKFILL_ROUNDS = 4;
  const loadedRef = useRef(0);
  const sidebarDragBase = useRef(sidebarWidth);
  const listDragBase = useRef(listWidth);
  const railDragBase = useRef(railWidth);

  const loadCached = useCallback(
    async (count: number) => {
      if (folder === UNIFIED_FOLDER) {
        const pages = await Promise.all(accounts.map((a) => api.listCached(a.id, "INBOX", 0, count)));
        const metas = pages.flatMap((p) => p.metas).sort((a, b) => b.timestamp - a.timestamp);
        loadedRef.current = metas.length;
        setTotal(pages.reduce((sum, p) => sum + p.total, 0));
        setInboxMetas(metas);
        setMessages(metas);
        setSelected((prev) => (prev && metas.some((m) => mailKey(m) === mailKey(prev.meta)) ? prev : null));
        return;
      }
      const realFolder = folder === RISK_FOLDER ? "INBOX" : folder;
      const res = await api.listCached(accountId, realFolder, 0, count);
      loadedRef.current = res.metas.length;
      setTotal(res.total);
      if (realFolder === "INBOX") setInboxMetas(res.metas);
      let metas = folder === RISK_FOLDER ? res.metas.filter(isRisky) : res.metas;
      metas = [...metas].sort((a, b) => b.timestamp - a.timestamp);
      setMessages(metas);
      setSelected((prev) => (prev && metas.some((m) => mailKey(m) === mailKey(prev.meta)) ? prev : null));
    },
    [accountId, accounts, folder]
  );

  const backfillOlderToTarget = useCallback(
    async (cachedTotal: number) => {
      if (folder === RISK_FOLDER || folder === DRAFTS_FOLDER || cachedTotal >= AUTO_CACHE_TARGET) return;
      let nextTotal = cachedTotal;
      for (let round = 0; round < AUTO_BACKFILL_ROUNDS && nextTotal < AUTO_CACHE_TARGET; round += 1) {
        if (folder === UNIFIED_FOLDER) {
          const results = await Promise.all(accounts.map((a) => api.syncOlderMessages(a.id, "INBOX")));
          if (results.every((r) => r.added === 0)) {
            setOlderExhausted(true);
            return;
          }
          nextTotal = results.reduce((sum, r) => sum + r.total, 0);
        } else {
          const realFolder = folder === RISK_FOLDER ? "INBOX" : folder;
          const res = await api.syncOlderMessages(accountId, realFolder);
          if (res.added === 0) {
            setOlderExhausted(true);
            return;
          }
          nextTotal = res.total;
        }
      }
    },
    [accountId, accounts, folder]
  );

  const loadMessages = useCallback(async () => {
    if (!accountId) return;
    if (folder === DRAFTS_FOLDER) return; // 草稿是本地数据，不走邮件拉取
    const seq = ++fetchSeq.current;
    setListError(null);
    // 1) 本地缓存先上屏（离线也能看）
    setLoading(loadedRef.current === 0);
    try {
      await loadCached(Math.max(loadedRef.current, PAGE));
    } catch (e) {
      if (seq === fetchSeq.current) setListError(String(e));
    } finally {
      if (seq === fetchSeq.current) setLoading(false);
    }
    if (seq !== fetchSeq.current) return;
    // 2) 后台与服务器增量同步，再回填
    setSyncing(true);
    try {
      let syncedTotal = 0;
      if (folder === UNIFIED_FOLDER) {
        const settled = await Promise.allSettled(accounts.map((a) => api.syncMessages(a.id, "INBOX")));
        const failed = settled.find((r): r is PromiseRejectedResult => r.status === "rejected");
        if (failed) throw failed.reason;
        syncedTotal = settled.reduce((sum, r) => (r.status === "fulfilled" ? sum + r.value.total : sum), 0);
      } else {
        const realFolder = folder === RISK_FOLDER ? "INBOX" : folder;
        const res = await api.syncMessages(accountId, realFolder);
        syncedTotal = res.total;
      }
      await backfillOlderToTarget(syncedTotal);
      if (seq === fetchSeq.current) await loadCached(Math.max(loadedRef.current, PAGE));
    } catch (e) {
      if (seq === fetchSeq.current) setListError(`同步失败（本地缓存仍可用）：${e}`);
    } finally {
      if (seq === fetchSeq.current) setSyncing(false);
    }
  }, [accountId, accounts, backfillOlderToTarget, folder, loadCached]);

  useEffect(() => {
    loadedRef.current = 0;
    setOlderExhausted(false);
    loadMessages();
  }, [loadMessages]);

  async function handleLoadMore() {
    if (loadingMore) return;
    setLoadingMore(true);
    try {
      if (loadedRef.current < total) {
        await loadCached(loadedRef.current + PAGE);
        return;
      }
      const realFolder = folder === RISK_FOLDER ? "INBOX" : folder;
      if (folder === UNIFIED_FOLDER) {
        const results = await Promise.all(accounts.map((a) => api.syncOlderMessages(a.id, "INBOX")));
        if (results.every((r) => r.added === 0)) setOlderExhausted(true);
      } else {
        const res = await api.syncOlderMessages(accountId, realFolder);
        if (res.added === 0) setOlderExhausted(true);
      }
      await loadCached(loadedRef.current + PAGE);
    } catch (e) {
      setListError(String(e));
    } finally {
      setLoadingMore(false);
    }
  }

  // ── 新邮件推送（后端 IMAP IDLE / POP3 轮询发出 new-mail 事件）──
  useEffect(() => {
    const unlisten = listen<{ accountId: string }>("new-mail", (e) => {
      if (folder === UNIFIED_FOLDER || e.payload.accountId === accountId) loadMessages();
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [accountId, folder, loadMessages]);

  // ── 界面缩放（持久化）──
  useEffect(() => {
    (document.body.style as CSSStyleDeclaration & { zoom: string }).zoom = String(zoom);
    localStorage.setItem("sealmail.zoom", String(zoom));
  }, [zoom]);

  useEffect(() => localStorage.setItem("sealmail.sidebarWidth", String(sidebarWidth)), [sidebarWidth]);
  useEffect(() => localStorage.setItem("sealmail.listWidth", String(listWidth)), [listWidth]);
  useEffect(() => localStorage.setItem("sealmail.railWidth", String(railWidth)), [railWidth]);

  // ── 选中邮件 ──
  async function selectMail(m: EmailMeta) {
    try {
      const full = await api.getMessage(m.accountId, m.folder, m.uid);
      setSelected(full);
      setThread([]);
      setView("mail");
      if (m.unread) {
        api.setRead(m.accountId, m.folder, m.uid, true).catch(() => {});
        setMessages((ms) => ms.map((x) => (mailKey(x) === mailKey(m) ? { ...x, unread: false } : x)));
        setInboxMetas((ms) => ms.map((x) => (mailKey(x) === mailKey(m) ? { ...x, unread: false } : x)));
      }
    } catch (e) {
      setListError(String(e));
    }
  }

  async function openMailWindow(m: EmailMeta) {
    try {
      const full = await api.getMessage(m.accountId, m.folder, m.uid);
      const key = `sealmail.popout.${m.accountId}.${m.folder}.${m.uid}.${Date.now()}`;
      localStorage.setItem(key, JSON.stringify(full));
      const label = `mail-${m.accountId}-${m.uid}-${Date.now()}`.replace(/[^a-zA-Z0-9_-]/g, "-");
      const win = new WebviewWindow(label, {
        url: `/?popout=${encodeURIComponent(key)}`,
        title: full.meta.subject || "邮件",
        width: 920,
        height: 760,
        minWidth: 680,
        minHeight: 520,
      });
      win.once("tauri://created", () => {
        if (m.unread) selectMail(m);
      });
      win.once("tauri://error", (e) => setListError(String(e.payload)));
    } catch (e) {
      setListError(String(e));
    }
  }

  useEffect(() => {
    if (!selected) {
      setThread([]);
      return;
    }
    let cancelled = false;
    api
      .listThread(selected.meta.accountId, selected.meta.folder, selected.meta.threadId)
      .then((rows) => {
        if (!cancelled) setThread(rows);
      })
      .catch((e) => {
        if (!cancelled) {
          setThread([]);
          console.error("读取会话线程失败", e);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [selected?.meta.accountId, selected?.meta.folder, selected?.meta.threadId, selected?.meta.uid]);

  // ── 搜索（本地过滤：发件人/主题/摘要/地址）+ 未读/星标过滤 ──
  const shownMessages = useMemo(() => {
    const q = search.trim().toLowerCase();
    let list = messages;
    if (filterMode === "unread") list = list.filter((m) => m.unread);
    if (filterMode === "flagged") list = list.filter((m) => m.flagged);
    if (!q) return list;
    return list.filter(
      (m) =>
        m.fromName.toLowerCase().includes(q) ||
        m.fromAddr.toLowerCase().includes(q) ||
        m.subject.toLowerCase().includes(q) ||
        m.preview.toLowerCase().includes(q)
    );
  }, [messages, search, filterMode]);

  const riskCount = useMemo(() => inboxMetas.filter(isRisky).length, [inboxMetas]);
  const inboxUnread = useMemo(() => inboxMetas.filter((m) => m.unread).length, [inboxMetas]);
  const listUnread = useMemo(() => messages.filter((m) => m.unread).length, [messages]);
  const folderTitle = folders.find((f) => f.name === folder)?.display ?? folder;

  function markLocal(keys: string[], unread: boolean) {
    const set = new Set(keys);
    const patch = (ms: EmailMeta[]) => ms.map((x) => (set.has(mailKey(x)) ? { ...x, unread } : x));
    setMessages(patch);
    setInboxMetas(patch);
  }

  async function handleMarkUnread() {
    if (!selected) return;
    try {
      await api.setRead(selected.meta.accountId, selected.meta.folder, selected.meta.uid, false);
      markLocal([mailKey(selected.meta)], true);
    } catch (e) {
      setListError(String(e));
    }
  }

  async function handleMarkAllRead() {
    const unread = messages.filter((m) => m.unread);
    if (unread.length === 0) return;
    try {
      const groups = new Map<string, EmailMeta[]>();
      for (const m of unread) {
        const key = folder === RISK_FOLDER ? `${m.accountId}\0INBOX` : `${m.accountId}\0${m.folder}`;
        groups.set(key, [...(groups.get(key) ?? []), m]);
      }
      await Promise.all(
        [...groups.entries()].map(([key, rows]) => {
          const [accId, fld] = key.split("\0");
          return api.markRead(accId, fld, rows.map((m) => m.uid));
        })
      );
      markLocal(unread.map(mailKey), false);
    } catch (e) {
      setListError(String(e));
    }
  }

  async function handleToggleFlag(m: EmailMeta) {
    const next = !m.flagged;
    try {
      await api.setFlagged(m.accountId, m.folder, m.uid, next);
      const patch = (ms: EmailMeta[]) => ms.map((x) => (mailKey(x) === mailKey(m) ? { ...x, flagged: next } : x));
      setMessages(patch);
      setInboxMetas(patch);
      setSelected((prev) =>
        prev && mailKey(prev.meta) === mailKey(m) ? { ...prev, meta: { ...prev.meta, flagged: next } } : prev
      );
    } catch (e) {
      setListError(String(e));
    }
  }

  async function handleMove(target: string) {
    if (!selected) return;
    try {
      await api.moveMessage(selected.meta.accountId, selected.meta.folder, selected.meta.uid, target);
      setSelected(null);
      setThread([]);
      loadMessages();
    } catch (e) {
      setListError(String(e));
    }
  }

  async function handleArchive() {
    if (!selected) return;
    try {
      await api.archiveMessage(selected.meta.accountId, selected.meta.folder, selected.meta.uid);
      setSelected(null);
      setThread([]);
      loadMessages();
      refreshFolders(accountId).catch(() => {});
    } catch (e) {
      setListError(String(e));
    }
  }

  // 当前选中邮件所在目录是否是回收站（回收站内删除 = 物理删除，需确认）
  const selectedInTrash = !!selected && folders.find((f) => f.name === selected.meta.folder)?.role === "trash";
  const selectedInArchive = !!selected && folders.find((f) => f.name === selected.meta.folder)?.role === "archive";

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
      setThread([]);
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
        const idx = selected ? shownMessages.findIndex((m) => mailKey(m) === mailKey(selected.meta)) : -1;
        const next = idx < 0 ? 0 : Math.min(shownMessages.length - 1, Math.max(0, idx + (down ? 1 : -1)));
        if (shownMessages[next] && (!selected || mailKey(shownMessages[next]) !== mailKey(selected.meta))) {
          selectMail(shownMessages[next]);
        }
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
  const accountLabels = useMemo(
    () => Object.fromEntries(accounts.map((a) => [a.id, a.email])),
    [accounts]
  );

  return (
    <div className="app">
      <div className="titlebar" data-tauri-drag-region>
        <div className="titlebar-left" data-tauri-drag-region />
        <div className="search-wrap" data-tauri-drag-region>
          {hasAccounts && (
            <div className="search">
              <svg width="13" height="13" viewBox="0 0 13 13" fill="none">
                <circle cx="5.5" cy="5.5" r="4" stroke="var(--mut-4)" strokeWidth="1.4" />
                <path d="M8.5 8.5l3 3" stroke="var(--mut-4)" strokeWidth="1.4" strokeLinecap="round" />
              </svg>
              <input ref={searchRef} placeholder="搜索邮件、发件人或地址…" value={search} onChange={(e) => setSearch(e.target.value)} />
              {search && (
                <button className="search-clear" onClick={() => setSearch("")} title="清空搜索">
                  ×
                </button>
              )}
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
            width={sidebarWidth}
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
          <PaneResizer
            title="拖动调整侧栏宽度"
            onStart={() => {
              sidebarDragBase.current = sidebarWidth;
            }}
            onDrag={(dx) => setSidebarWidth(clamp(sidebarDragBase.current + dx, 176, 280))}
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
                width={listWidth}
                title={folderTitle}
                messages={shownMessages}
                selectedKey={selected ? mailKey(selected.meta) : null}
                accountLabels={folder === UNIFIED_FOLDER ? accountLabels : undefined}
                loading={loading}
                syncing={syncing}
                error={listError}
                filterMode={filterMode}
                unreadCount={listUnread}
                loadedCount={shownMessages.length}
                totalCount={folder === RISK_FOLDER ? messages.length : total}
                hasMore={folder !== RISK_FOLDER && (loadedRef.current < total || !olderExhausted)}
                loadingMore={loadingMore}
                onFilterMode={setFilterMode}
                onMarkAllRead={handleMarkAllRead}
                onToggleFlag={handleToggleFlag}
                onLoadMore={handleLoadMore}
                onSelect={selectMail}
                onOpenWindow={openMailWindow}
                onRefresh={loadMessages}
              />
              <PaneResizer
                title="拖动调整列表宽度"
                onStart={() => {
                  listDragBase.current = listWidth;
                }}
                onDrag={(dx) => setListWidth(clamp(listDragBase.current + dx, 300, 480))}
              />
              <MessageView
                mail={selected}
                thread={thread}
                folders={folders}
                onOpenThreadMail={selectMail}
                onReply={handleReply}
                onReplyAll={handleReplyAll}
                onForward={handleForward}
                onMove={handleMove}
                canMove={folder !== UNIFIED_FOLDER}
                canArchive={!selectedInArchive}
                onArchive={handleArchive}
                onDelete={handleDelete}
                onShowRisk={() => setRiskOpen(true)}
                onTrustSender={handleTrustSender}
                onMarkUnread={handleMarkUnread}
                onToggleFlag={() => selected && handleToggleFlag(selected.meta)}
              />
              <PaneResizer
                title="拖动调整验证栏宽度"
                onStart={() => {
                  railDragBase.current = railWidth;
                }}
                onDrag={(dx) => setRailWidth(clamp(railDragBase.current - dx, 240, 420))}
              />
              <VerifyRail
                mail={selected}
                open={railOpen}
                width={railWidth}
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
            <div className="modal-body" style={{ fontSize: 13, lineHeight: 1.7, color: "var(--mut)" }}>
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
              <div style={{ fontSize: 11, color: "var(--mut-3)", lineHeight: 1.6 }}>
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
