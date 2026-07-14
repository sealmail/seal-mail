import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { addPluginListener } from "@tauri-apps/api/core";
import { WebviewWindow, getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import * as api from "./api";
import { applyLangPref, t, useI18n } from "./i18n";
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
import { TextBody } from "./components/TextBody";
import { DRAFTS_FOLDER, RISK_FOLDER, UNIFIED_FOLDER, Sidebar } from "./components/Sidebar";
import { Seal } from "./components/Seal";
import { classifyMail, type MailCategory } from "./mailCategory";
import { LatestRequest, type RequestToken } from "./latestRequest";
import type { Account, AppStateView, Draft, EmailFull, EmailMeta, FilterRule, FolderInfo, IdentityInfo, NotificationMailTarget } from "./types";
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

function threadKey(m: Pick<EmailMeta, "accountId" | "folder" | "threadId" | "messageId" | "uid">) {
  return `${m.accountId}/${m.folder}/${m.threadId || m.messageId || m.uid}`;
}

function defaultShowHtml(mail: EmailFull) {
  return !!mail.bodyHtml?.trim();
}

function clamp(n: number, min: number, max: number) {
  return Math.min(max, Math.max(min, n));
}

type ZoomShortcut = { kind: "step"; delta: number } | { kind: "reset" };
const MIN_ZOOM = 0.5;
const MAX_ZOOM = 2.4;

function zoomShortcutForKey(e: KeyboardEvent): ZoomShortcut | null {
  const meta = e.metaKey || e.ctrlKey;
  if (!meta || e.altKey) return null;
  if (e.key === "+" || e.key === "=" || e.code === "Equal" || e.code === "NumpadAdd") return { kind: "step", delta: 0.1 };
  if (e.key === "-" || e.key === "_" || e.code === "Minus" || e.code === "NumpadSubtract") return { kind: "step", delta: -0.1 };
  if (e.key === "0" || e.code === "Digit0" || e.code === "Numpad0") return { kind: "reset" };
  return null;
}

function useZoomShortcuts(opts: { persist?: boolean } = {}) {
  // persist=false（邮件子窗口）：缩放只作用于本窗口且不落盘——初始值继承主窗口
  // 设置,但快捷键调整是临时的,关窗即弃,不能反向污染 sealmail.zoom
  const persist = opts.persist !== false;
  const [zoom, setZoom] = useState(() => {
    const z = parseFloat(localStorage.getItem("sealmail.zoom") ?? "1");
    return Number.isFinite(z) && z >= MIN_ZOOM && z <= MAX_ZOOM ? z : 1;
  });

  useEffect(() => {
    // 原生 pageZoom（WKWebView setPageZoom / WebView2 ZoomFactor）：视口级缩放，
    // 百分比布局和 iframe 内容随视口一起缩放，无需任何 CSS zoom 布局补偿。
    // 不要改回 document.body.style.zoom——Tauri 的 WKWebView 是旧版 CSS zoom
    // 语义（百分比不按 zoom 换算），整个 app 会溢出窗口右缘和底缘；Safari 探针
    // 是新版标准化语义，测不出这个问题。
    getCurrentWebviewWindow()
      .setZoom(zoom)
      .catch((err) => console.error("setZoom 失败（检查 core:webview:allow-set-webview-zoom 权限）", err));
    if (persist) localStorage.setItem("sealmail.zoom", String(zoom));
    window.dispatchEvent(new CustomEvent("sealmail-zoom-change", { detail: zoom }));
  }, [zoom, persist]);

  useEffect(() => {
    function applyZoomShortcut(shortcut: ZoomShortcut) {
      if (shortcut.kind === "reset") setZoom(1);
      else setZoom((z) => clamp(Math.round((z + shortcut.delta) * 10) / 10, MIN_ZOOM, MAX_ZOOM));
    }

    function onKey(e: KeyboardEvent) {
      const shortcut = zoomShortcutForKey(e);
      if (!shortcut) return;
      e.preventDefault();
      e.stopPropagation();
      applyZoomShortcut(shortcut);
    }

    function onFrameShortcut(e: Event) {
      const shortcut = (e as CustomEvent<ZoomShortcut>).detail;
      if (shortcut?.kind === "reset" || shortcut?.kind === "step") applyZoomShortcut(shortcut);
    }

    window.addEventListener("keydown", onKey, true);
    window.addEventListener("sealmail-zoom-delta", onFrameShortcut as EventListener);
    // macOS 原生菜单（显示 > 放大/缩小/实际大小）：切换 app 回来后 WKWebView 可能
    // 丢失 first responder，页面收不到 keydown，菜单加速键不受此影响
    const unlistenMenu = getCurrentWebviewWindow().listen<ZoomShortcut>("sealmail-menu-zoom", (e) => {
      applyZoomShortcut(e.payload);
    });
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("sealmail-zoom-delta", onFrameShortcut as EventListener);
      void unlistenMenu.then((f) => f());
    };
  }, []);

  return { zoom, setZoom };
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
  useZoomShortcuts({ persist: false });
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
        <div className="empty-pane">{t("这封邮件窗口的数据已经过期，请从主窗口重新打开。")}</div>
      </div>
    );
  }

  const hasHtml = !!mail.bodyHtml;
  const signed = mail.verify.status !== "unsigned";
  const showHtml = hasHtml && (htmlMode ?? defaultShowHtml(mail));

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
            {signed && showHtml && <span className="body-note">{t("签名校验针对纯文本正文，HTML 版式仅供参考")}</span>}
            <button className="btn-ghost" style={{ height: 24, padding: "0 10px", fontSize: 11 }} onClick={() => setHtmlMode(!showHtml)}>
              {showHtml ? t("查看纯文本") : t("查看 HTML 版式")}
            </button>
          </div>
        )}
        {showHtml ? <HtmlBody html={mail.bodyHtml as string} /> : <TextBody text={mail.bodyText} />}
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
  useI18n();
  const [state, setState] = useState<AppStateView | null>(null);
  const [bootError, setBootError] = useState<string | null>(null);
  const [accountId, setAccountId] = useState("");
  const [folders, setFolders] = useState<FolderInfo[]>(BUILTIN_FOLDERS);
  const [folder, setFolder] = useState("INBOX");
  const [messages, setMessages] = useState<EmailMeta[]>([]);
  const [inboxMetas, setInboxMetas] = useState<EmailMeta[]>([]);
  const [inboxUnreadByAccount, setInboxUnreadByAccount] = useState<Record<string, number>>({});
  const [selected, setSelected] = useState<EmailFull | null>(null);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [thread, setThread] = useState<EmailMeta[]>([]);
  // 会话正文缓存：mailKey → 全文。只按需加载（选中封 + 首末几封），长会话中间的
  // 邮件渲染占位卡片、点击再取——此前一次性拉取整条会话所有正文是切换卡顿的主因。
  const [threadFulls, setThreadFulls] = useState<Record<string, EmailFull>>({});
  const [view, setView] = useState<"mail" | "keys">("mail");
  const [search, setSearch] = useState("");
  const [filterMode, setFilterMode] = useState<"all" | "unread" | "flagged">("all");
  const [categoryMode, setCategoryMode] = useState<MailCategory>("all");
  const [total, setTotal] = useState(0);
  const [syncing, setSyncing] = useState(false);
  // 界面缩放（Cmd+/-/0），走原生 pageZoom
  useZoomShortcuts();
  const [sidebarWidth, setSidebarWidth] = useState(() => {
    const n = Number(localStorage.getItem("sealmail.sidebarWidth") ?? 228);
    return Number.isFinite(n) ? clamp(n, 176, 280) : 228;
  });
  const [listWidth, setListWidth] = useState(() => {
    const n = Number(localStorage.getItem("sealmail.listWidth") ?? 380);
    return Number.isFinite(n) ? clamp(n, 320, 520) : 380;
  });
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [olderExhausted, setOlderExhausted] = useState(false);
  const [listError, setListError] = useState<string | null>(null);
  const [loadMoreNotice, setLoadMoreNotice] = useState<string | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  const [composeOpen, setComposeOpen] = useState(false);
  const [composePrefill, setComposePrefill] = useState<ComposePrefill | undefined>();
  const [composeDraft, setComposeDraft] = useState<Draft | undefined>();
  const [drafts, setDrafts] = useState<Draft[]>([]);
  const [accountModal, setAccountModal] = useState(false);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [profileOpen, setProfileOpen] = useState(false);
  const [riskOpen, setRiskOpen] = useState(false);
  // 已在风险弹窗里勾选确认的邮件（Message-ID 优先，持久化到本机）：横幅收起为一行提示
  const [riskAcked, setRiskAcked] = useState<Set<string>>(() => {
    try {
      return new Set(JSON.parse(localStorage.getItem("sealmail.riskAcked") ?? "[]") as string[]);
    } catch {
      return new Set();
    }
  });
  const riskKey = (m: EmailFull) => m.meta.messageId || mailKey(m.meta);
  function ackRisk(m: EmailFull) {
    setRiskAcked((prev) => {
      const next = new Set(prev);
      next.add(riskKey(m));
      // 只保留最近 500 条，避免无限增长
      const arr = [...next].slice(-500);
      localStorage.setItem("sealmail.riskAcked", JSON.stringify(arr));
      return new Set(arr);
    });
    setRiskOpen(false);
  }
  const [newFolderOpen, setNewFolderOpen] = useState(false);
  const [newFolderName, setNewFolderName] = useState("");
  const [newFolderErr, setNewFolderErr] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [folderToDelete, setFolderToDelete] = useState<FolderInfo | null>(null);
  const [deleteFolderErr, setDeleteFolderErr] = useState<string | null>(null);
  const [accountToRemove, setAccountToRemove] = useState<Account | null>(null);
  const [removeAccountErr, setRemoveAccountErr] = useState<string | null>(null);

  const fetchRequests = useRef(new LatestRequest());
  const selectSeq = useRef(0);
  // 始终指向最新已加载列表，供 selectMail 本地筛会话用（避免后端全量扫描整个目录）
  const messagesRef = useRef<EmailMeta[]>(messages);
  messagesRef.current = messages;
  // 同会话内切换时复用已加载的 metas 与正文缓存（ref 避免闭包拿到旧值）
  const threadRef = useRef<EmailMeta[]>(thread);
  threadRef.current = thread;
  const threadFullsRef = useRef<Record<string, EmailFull>>(threadFulls);
  threadFullsRef.current = threadFulls;
  const clearSelection = useCallback(() => {
    selectSeq.current += 1;
    setSelected(null);
    setSelectedKey(null);
    setThread([]);
    setThreadFulls({});
  }, []);
  const retainSelection = useCallback((metas: EmailMeta[]) => {
    const visible = new Set(metas.map(mailKey));
    setSelected((prev) => (prev && visible.has(mailKey(prev.meta)) ? prev : null));
    setSelectedKey((prev) => (prev && visible.has(prev) ? prev : null));
  }, []);

  const accounts = state?.accounts ?? [];
  const trusted = state?.trusted ?? [];
  const hasAccounts = accounts.length > 0;

  // ── 初始化 ──
  useEffect(() => {
    // 界面语言尽早生效（默认中文，英文用户首屏可能闪一下中文）
    api.getLanguagePref().then(applyLangPref).catch(() => {});
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

  useEffect(() => {
    let cancelled = false;
    Promise.all(
      accounts.map(async (account) => {
        const result = await api.listCached(account.id, "INBOX", 0, 0);
        return [account.id, result.unreadCount] as const;
      })
    )
      .then((counts) => {
        if (cancelled) return;
        setInboxUnreadByAccount(Object.fromEntries(counts));
      })
      .catch((e) => console.error("读取统一收件箱未读数失败", e));
    return () => {
      cancelled = true;
    };
  }, [accounts]);

  const refreshInboxUnread = useCallback(async (accId: string) => {
    const result = await api.listCached(accId, "INBOX", 0, 0);
    setInboxUnreadByAccount((counts) => ({ ...counts, [accId]: result.unreadCount }));
  }, []);

  // ── 切账户：拉目录 ──
  useEffect(() => {
    if (!accountId) return;
    setFolder("INBOX");
    clearSelection();
    setListError(null);
    refreshFolders(accountId).catch((e) => {
      setFolders(BUILTIN_FOLDERS);
      setListError(String(e));
    });
  }, [accountId, clearSelection, refreshFolders]);

  // ── 拉邮件：本地全量 meta 一次上屏 → 后台增量同步/回填更早邮件 ──
  // limit=0 表示目录内本地全量；列表不做分页。网络同步慢一次，缓存齐后即可离线浏览。
  const FULL_LOCAL = 0;
  const MAX_OLDER_BACKFILL_ROUNDS = 50;
  const loadedRef = useRef(0);
  const sidebarDragBase = useRef(sidebarWidth);
  const listDragBase = useRef(listWidth);

  const loadCached = useCallback(
    async (request: RequestToken) => {
      if (folder === UNIFIED_FOLDER) {
        const pages = await Promise.all(accounts.map((a) => api.listCached(a.id, "INBOX", 0, FULL_LOCAL)));
        if (!fetchRequests.current.isCurrent(request)) return false;
        const metas = pages.flatMap((p) => p.metas).sort((a, b) => b.timestamp - a.timestamp);
        loadedRef.current = metas.length;
        setTotal(pages.reduce((sum, p) => sum + p.total, 0));
        setInboxUnreadByAccount(
          Object.fromEntries(pages.map((page, index) => [accounts[index].id, page.unreadCount]))
        );
        setInboxMetas(metas);
        setMessages(metas);
        retainSelection(metas);
        return true;
      }
      const realFolder = folder === RISK_FOLDER ? "INBOX" : folder;
      const res = await api.listCached(accountId, realFolder, 0, FULL_LOCAL);
      if (!fetchRequests.current.isCurrent(request)) return false;
      loadedRef.current = res.metas.length;
      setTotal(res.total);
      if (realFolder === "INBOX") {
        setInboxMetas(res.metas);
        setInboxUnreadByAccount((counts) => ({ ...counts, [accountId]: res.unreadCount }));
      }
      let metas = folder === RISK_FOLDER ? res.metas.filter(isRisky) : res.metas;
      metas = [...metas].sort((a, b) => b.timestamp - a.timestamp);
      setMessages(metas);
      retainSelection(metas);
      return true;
    },
    [accountId, accounts, folder, retainSelection]
  );

  /** 从服务器把更早邮件拉进本地库，直到没有更多（或达上限）。 */
  const backfillOlderUntilExhausted = useCallback(
    async (request: RequestToken) => {
      if (folder === RISK_FOLDER || folder === DRAFTS_FOLDER) return;
      for (let round = 0; round < MAX_OLDER_BACKFILL_ROUNDS; round += 1) {
        if (!fetchRequests.current.isCurrent(request)) return;
        if (folder === UNIFIED_FOLDER) {
          const results = await Promise.all(accounts.map((a) => api.syncOlderMessages(a.id, "INBOX")));
          if (!fetchRequests.current.isCurrent(request)) return;
          if (results.every((r) => r.added === 0)) {
            setOlderExhausted(true);
            return;
          }
        } else {
          const realFolder = folder === RISK_FOLDER ? "INBOX" : folder;
          const res = await api.syncOlderMessages(accountId, realFolder);
          if (!fetchRequests.current.isCurrent(request)) return;
          if (res.added === 0) {
            setOlderExhausted(true);
            return;
          }
        }
        // 每轮回填后刷新全量列表，用户能看到更早邮件陆续出现
        await loadCached(request);
      }
    },
    [accountId, accounts, folder, loadCached]
  );

  const loadMessages = useCallback(async () => {
    if (!accountId) return;
    if (folder === DRAFTS_FOLDER) return; // 草稿是本地数据，不走邮件拉取
    const request = fetchRequests.current.begin();
    setListError(null);
    setLoadMoreNotice(null);
    // 1) 本地全量 meta 先上屏（不碰网络、不解析 raw）
    setLoading(true);
    try {
      await loadCached(request);
    } catch (e) {
      if (fetchRequests.current.isCurrent(request)) setListError(String(e));
    } finally {
      if (fetchRequests.current.isCurrent(request)) setLoading(false);
    }
    if (!fetchRequests.current.isCurrent(request)) return;
    // 2) 后台与服务器增量同步，再尽量把本地缓存补全
    setSyncing(true);
    try {
      if (folder === UNIFIED_FOLDER) {
        const settled = await Promise.allSettled(accounts.map((a) => api.syncMessages(a.id, "INBOX")));
        const failed = settled.find((r): r is PromiseRejectedResult => r.status === "rejected");
        if (failed) throw failed.reason;
      } else {
        const realFolder = folder === RISK_FOLDER ? "INBOX" : folder;
        await api.syncMessages(accountId, realFolder);
      }
      if (!fetchRequests.current.isCurrent(request)) return;
      await loadCached(request);
      if (!fetchRequests.current.isCurrent(request)) return;
      await backfillOlderUntilExhausted(request);
      if (fetchRequests.current.isCurrent(request)) await loadCached(request);
    } catch (e) {
      if (fetchRequests.current.isCurrent(request)) setListError(t("同步失败（本地缓存仍可用）：") + e);
    } finally {
      if (fetchRequests.current.isCurrent(request)) setSyncing(false);
    }
  }, [accountId, accounts, backfillOlderUntilExhausted, folder, loadCached, t]);

  useEffect(() => {
    loadedRef.current = 0;
    setOlderExhausted(false);
    setLoadMoreNotice(null);
    loadMessages();
  }, [loadMessages]);

  /** 仅从服务器拉取更早邮件进本地缓存（本地列表已是全量，不再做分页）。 */
  async function handleLoadMore() {
    if (loadingMore || folder === RISK_FOLDER) return;
    const request = fetchRequests.current.begin();
    setLoadingMore(true);
    setLoadMoreNotice(null);
    try {
      let exhausted = false;
      if (folder === UNIFIED_FOLDER) {
        const results = await Promise.all(accounts.map((a) => api.syncOlderMessages(a.id, "INBOX")));
        if (!fetchRequests.current.isCurrent(request)) return;
        exhausted = results.every((r) => r.added === 0);
      } else {
        const realFolder = folder === RISK_FOLDER ? "INBOX" : folder;
        const res = await api.syncOlderMessages(accountId, realFolder);
        if (!fetchRequests.current.isCurrent(request)) return;
        exhausted = res.added === 0;
      }
      if (exhausted) {
        setOlderExhausted(true);
        setLoadMoreNotice(t("没有更早的邮件了"));
      }
      await loadCached(request);
    } catch (e) {
      if (fetchRequests.current.isCurrent(request)) setListError(String(e));
    } finally {
      if (fetchRequests.current.isCurrent(request)) setLoadingMore(false);
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

  useEffect(() => localStorage.setItem("sealmail.sidebarWidth", String(sidebarWidth)), [sidebarWidth]);
  useEffect(() => localStorage.setItem("sealmail.listWidth", String(listWidth)), [listWidth]);

  // ── 选中邮件 ──
  // 会话正文按需加载：短会话全量；长会话只先取首封 + 末尾几封 + 选中封
  const EAGER_THREAD_BODIES = 6;

  async function selectMail(m: EmailMeta, opts: { markRead?: boolean } = {}) {
    const key = mailKey(m);
    const seq = ++selectSeq.current;
    // 同会话内切换：metas 与正文缓存直接复用，不清空避免整屏闪烁
    const sameThread =
      !!m.threadId &&
      threadRef.current.length > 0 &&
      threadRef.current[0]?.threadId === m.threadId &&
      threadRef.current.some((x) => mailKey(x) === key);
    const cached = sameThread ? threadFullsRef.current[key] : undefined;
    setSelectedKey(key);
    setSelected(cached ?? null);
    if (!sameThread) {
      setThread([]);
      setThreadFulls({});
    }
    setView("mail");
    const shouldMarkRead = opts.markRead !== false && m.unread;
    if (shouldMarkRead) {
      api
        .setRead(m.accountId, m.folder, m.uid, true)
        .then(() => refreshInboxUnread(m.accountId))
        .catch((e) => setListError(String(e)));
      setMessages((ms) => ms.map((x) => (mailKey(x) === key ? { ...x, unread: false } : x)));
      setInboxMetas((ms) => ms.map((x) => (mailKey(x) === key ? { ...x, unread: false } : x)));
    }
    try {
      const full = cached ?? (await api.getMessage(m.accountId, m.folder, m.uid));
      if (seq !== selectSeq.current) return;
      const selectedFull = shouldMarkRead ? { ...full, meta: { ...full.meta, unread: false } } : full;
      setSelected(selectedFull);

      // 同会话邮件优先从「已加载列表」本地筛选——后端 list_thread 会把整个目录的邮件
      // 全部读出来逐封解析。绝大多数会话的邮件都在已加载范围内；
      // 只有本地还没有这封（如点通知直达一封未加载的邮件）才回退到后端按需取整条会话。
      let threadMetas: EmailMeta[];
      if (sameThread) {
        threadMetas = threadRef.current;
      } else {
        const localThread = messagesRef.current.filter(
          (x) => x.accountId === m.accountId && x.folder === m.folder && x.threadId === m.threadId
        );
        if (m.threadId && localThread.some((x) => mailKey(x) === key)) {
          threadMetas = localThread;
        } else {
          threadMetas = await api.listThread(m.accountId, m.folder, m.threadId).catch(() => [m]);
          if (seq !== selectSeq.current) return;
        }
      }
      const sortedThread = threadMetas
        .map((item) => (mailKey(item) === key && shouldMarkRead ? { ...item, unread: false } : item))
        .sort((a, b) => a.timestamp - b.timestamp);
      // 列表行按会话聚合未读点,打开会话必须整条标已读:只标被点击的最新一封,
      // 会话里更早的未读邮件会让未读点永远清不掉(且再次点击时最新一封已读,不再触发标记)
      let normalizedThread = sortedThread;
      if (opts.markRead !== false) {
        const unreadInThread = sortedThread.filter((x) => x.unread);
        if (unreadInThread.length > 0) {
          api
            .markRead(m.accountId, m.folder, unreadInThread.map((x) => x.uid))
            .then(() => refreshInboxUnread(m.accountId))
            .catch((e) => setListError(String(e)));
          const unreadKeys = new Set(unreadInThread.map(mailKey));
          const clearUnread = (ms: EmailMeta[]) => ms.map((x) => (unreadKeys.has(mailKey(x)) ? { ...x, unread: false } : x));
          setMessages(clearUnread);
          setInboxMetas(clearUnread);
          normalizedThread = clearUnread(sortedThread);
        }
      }
      setThread(normalizedThread);

      // 只急加载一小撮正文：选中封 + 首封 + 末尾三封；其余占位卡片点击再取
      const eager = new Set<string>([key]);
      if (normalizedThread.length <= EAGER_THREAD_BODIES) {
        normalizedThread.forEach((item) => eager.add(mailKey(item)));
      } else {
        eager.add(mailKey(normalizedThread[0]));
        normalizedThread.slice(-3).forEach((item) => eager.add(mailKey(item)));
      }
      const known = sameThread ? threadFullsRef.current : {};
      const missing = normalizedThread.filter(
        (item) => eager.has(mailKey(item)) && mailKey(item) !== key && !known[mailKey(item)]
      );
      const loaded = await Promise.all(
        missing.map(async (item) => {
          try {
            return await api.getMessage(item.accountId, item.folder, item.uid);
          } catch {
            return null; // 单封失败只影响该占位卡片，可点击重试
          }
        })
      );
      if (seq !== selectSeq.current) return;
      setThreadFulls((s) => {
        const next = { ...s, [key]: selectedFull };
        for (const f of loaded) {
          if (f) next[mailKey(f.meta)] = f;
        }
        return next;
      });
    } catch (e) {
      if (seq === selectSeq.current) setListError(String(e));
    }
  }

  // 长会话中被折叠的正文：点击占位卡片时按需加载
  async function loadThreadFull(item: EmailMeta) {
    const key = mailKey(item);
    if (threadFullsRef.current[key]) return;
    const seq = selectSeq.current;
    try {
      const full = await api.getMessage(item.accountId, item.folder, item.uid);
      if (seq === selectSeq.current) setThreadFulls((s) => ({ ...s, [key]: full }));
    } catch (e) {
      if (seq === selectSeq.current) setListError(String(e));
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
        title: full.meta.subject || t("邮件"),
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

  async function openNotificationMail(target: NotificationMailTarget) {
    const targetFolder = target.folder || "INBOX";
    setAccountId(target.accountId);
    setFolder(targetFolder);
    setSearch("");
    setFilterMode("all");
    setCategoryMode("all");
    setView("mail");
    setListError(null);
    setLoading(true);
    setSyncing(true);
    // 本地缓存里定位目标邮件；命中则立即打开
    const locate = async (): Promise<EmailMeta | undefined> => {
      const res = await api.listCached(target.accountId, targetFolder, 0, FULL_LOCAL);
      const metas = [...res.metas].sort((a, b) => b.timestamp - a.timestamp);
      loadedRef.current = metas.length;
      setTotal(res.total);
      if (targetFolder === "INBOX") {
        setInboxMetas(metas);
        setInboxUnreadByAccount((counts) => ({ ...counts, [target.accountId]: res.unreadCount }));
      }
      setMessages(metas);
      retainSelection(metas);
      return target.uid != null
        ? metas.find((m) => m.uid === target.uid)
        : metas.find((m) => target.messageId && m.messageId === target.messageId);
    };
    try {
      // 本地缓存优先：点通知要立刻见到邮件，不能等网络。通知里的邮件绝大多数
      // 已被 watcher/上次同步写进缓存；只有缓存真没有才阻塞式同步一次再找。
      // 目录刷新和列表同步放后台补，慢网络/断网都不挡打开。
      let meta = await locate();
      if (!meta) {
        await api.syncMessages(target.accountId, targetFolder).catch((e) => {
          console.warn("同步通知邮件失败，尝试从本地缓存打开", e);
        });
        meta = await locate();
        // 还找不到：邮件很可能已被过滤规则移出该目录，按 Message-ID 全目录定位后重进
        if (!meta && target.messageId) {
          const loc = await api.locateMessage(target.accountId, target.messageId).catch(() => null);
          if (loc && (loc.folder !== targetFolder || loc.uid !== target.uid)) {
            return openNotificationMail({ ...target, folder: loc.folder, uid: loc.uid });
          }
        }
      } else {
        api.syncMessages(target.accountId, targetFolder).then(() => loadMessages()).catch(() => {});
      }
      refreshFolders(target.accountId).catch(() => {});

      if (meta) {
        await selectMail(meta);
      } else if (target.uid != null) {
        const full = await api.getMessage(target.accountId, targetFolder, target.uid);
        if (full.meta.unread) api.setRead(target.accountId, targetFolder, target.uid, true).catch(() => {});
        setSelectedKey(mailKey(full.meta));
        const selectedFull = full.meta.unread ? { ...full, meta: { ...full.meta, unread: false } } : full;
        setSelected(selectedFull);
        setThread([selectedFull.meta]);
        setThreadFulls({ [mailKey(selectedFull.meta)]: selectedFull });
      } else {
        setListError(t("已打开 SealMail，但没有在本地缓存中找到这封通知邮件。"));
      }
    } catch (e) {
      setListError(t("打开通知邮件失败：") + e);
    } finally {
      setLoading(false);
      setSyncing(false);
    }
  }

  // 始终指向最新的 openNotificationMail，避免监听器闭包过期；监听器只注册一次，
  // 不再随 accountId 反复解绑/重绑（那个间隙曾导致点击通知的事件被丢掉）。
  const openNotificationMailRef = useRef(openNotificationMail);
  openNotificationMailRef.current = openNotificationMail;

  // 点击系统通知后定位邮件：改为「前端主动拉取」而非依赖后端一次性 emit。
  // 无论应用通过哪种方式被带到前台（点通知/点 Dock/窗口聚焦），都会触发一次拉取；
  // 后端仅在确有待打开目标且被成功取走时才消费它，事件丢失也不会白白吃掉目标。
  useEffect(() => {
    let cancelled = false;
    let pulling = false;
    async function pullPendingNotification() {
      if (pulling) return; // 同一时刻只拉一次，避免重复触发
      pulling = true;
      try {
        const target = await api.openPendingNotificationMail();
        if (!cancelled && target) openNotificationMailRef.current(target);
      } catch (e) {
        if (!cancelled) setListError(String(e));
      } finally {
        pulling = false;
      }
    }

    // 1) 启动兜底：进程内若已记录待打开目标（如 webview 重载）也能补上
    pullPendingNotification();

    // 2) 窗口被激活/重新可见——点击通知后系统把窗口带到前台时最可靠的信号
    const onFocus = () => pullPendingNotification();
    const onVisible = () => {
      if (document.visibilityState === "visible") pullPendingNotification();
    };
    window.addEventListener("focus", onFocus);
    document.addEventListener("visibilitychange", onVisible);

    // 3) 后端在 Focused/Reopen/Opened 时发来的提醒（暖启动主路径）
    const unlistenPoke = listen("notification-activated", () => pullPendingNotification());
    // 4) 移动端原生通知点击事件（桌面端不会触发，保留以兼容移动端）
    const unlistenAction = addPluginListener("notification", "actionPerformed", () =>
      pullPendingNotification()
    );

    return () => {
      cancelled = true;
      window.removeEventListener("focus", onFocus);
      document.removeEventListener("visibilitychange", onVisible);
      unlistenPoke.then((f) => f());
      unlistenAction.then((listener) => listener.unregister());
    };
  }, []);

  // ── 搜索（本地过滤：发件人/主题/摘要/地址）+ 未读/星标过滤 ──
  const shownMessages = useMemo(() => {
    const q = search.trim().toLowerCase();
    let list = messages;
    if (categoryMode !== "all") list = list.filter((m) => classifyMail(m) === categoryMode);
    if (filterMode === "unread") list = list.filter((m) => m.unread || (selectedKey !== null && mailKey(m) === selectedKey));
    if (filterMode === "flagged") list = list.filter((m) => m.flagged);
    if (!q) return list;
    return list.filter(
      (m) =>
        m.fromName.toLowerCase().includes(q) ||
        m.fromAddr.toLowerCase().includes(q) ||
        m.subject.toLowerCase().includes(q) ||
        m.preview.toLowerCase().includes(q)
    );
  }, [messages, search, categoryMode, filterMode, selectedKey]);

  const shownThreadMessages = useMemo(() => {
    return Array.from(
      shownMessages
        .reduce((groups, m) => {
          const key = threadKey(m);
          const existing = groups.get(key);
          if (existing) existing.push(m);
          else groups.set(key, [m]);
          return groups;
        }, new Map<string, EmailMeta[]>())
        .values()
    )
      .map((group) => [...group].sort((a, b) => b.timestamp - a.timestamp)[0])
      .sort((a, b) => b.timestamp - a.timestamp);
  }, [shownMessages]);

  useEffect(() => {
    setLoadMoreNotice(null);
  }, [accountId, folder, search, categoryMode, filterMode]);

  useEffect(() => {
    if (view !== "mail" || folder === DRAFTS_FOLDER || loading) return;
    if (shownThreadMessages.length === 0) {
      setSelected(null);
      setSelectedKey(null);
      return;
    }
    if (!selectedKey || !shownMessages.some((m) => mailKey(m) === selectedKey)) {
      selectMail(shownThreadMessages[0], { markRead: false });
    }
  }, [folder, loading, selectedKey, shownMessages, shownThreadMessages, view]);

  const riskUnread = useMemo(() => inboxMetas.filter((m) => m.unread && isRisky(m)).length, [inboxMetas]);
  // 收件箱角标：当前账户 INBOX 的 DB 全量 COUNT（不是「列表窗口里数一下」）
  const inboxUnread = inboxUnreadByAccount[accountId] ?? 0;
  const unifiedUnread = useMemo(() => {
    const activeAccountIds = new Set(accounts.map((account) => account.id));
    return Object.entries(inboxUnreadByAccount).reduce(
      (sum, [id, unread]) => (activeAccountIds.has(id) ? sum + unread : sum),
      0
    );
  }, [accounts, inboxUnreadByAccount]);
  // 列表「未读」筛选旁数字：当前已加载的本地全量列表（与缓存一致）
  const listUnread = useMemo(() => messages.filter((m) => m.unread).length, [messages]);
  const categoryCounts = useMemo(() => {
    const counts: Record<MailCategory, number> = { all: messages.length, personal: 0, business: 0, ads: 0 };
    messages.forEach((m) => {
      counts[classifyMail(m)]++;
    });
    return counts;
  }, [messages]);
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
      await refreshInboxUnread(selected.meta.accountId);
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
      await Promise.all([...groups.keys()].map((key) => refreshInboxUnread(key.split("\0")[0])));
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
      clearSelection();
      loadMessages();
    } catch (e) {
      setListError(String(e));
    }
  }

  async function handleArchive() {
    if (!selected) return;
    try {
      await api.archiveMessage(selected.meta.accountId, selected.meta.folder, selected.meta.uid);
      clearSelection();
      loadMessages();
      refreshFolders(accountId).catch(() => {});
    } catch (e) {
      setListError(String(e));
    }
  }

  function preferredBlockFolder() {
    return (
      folders.find((f) => f.role === "junk") ??
      folders.find((f) => /垃圾|junk|spam/i.test(`${f.display} ${f.name}`)) ??
      folders.find((f) => f.role === "trash") ??
      folders.find((f) => /已删除|回收站|trash|deleted/i.test(`${f.display} ${f.name}`))
    );
  }

  async function handleBlockSender() {
    if (!selected) return;
    const target = preferredBlockFolder();
    if (!target) {
      setListError(t("没有找到垃圾邮件或已删除邮件目录，请先创建一个目录后再屏蔽发件人。"));
      return;
    }
    const email = selected.meta.fromAddr.trim().toLowerCase();
    try {
      const rules = await api.saveFilter({
        id: "",
        name: t("屏蔽 {email}", { email }),
        accountId: selected.meta.accountId,
        field: "from",
        op: "contains",
        value: email,
        targetFolder: target.name,
        markRead: true,
        enabled: true,
      });
      setState((s) => (s ? { ...s, filters: rules } : s));
      if (selected.meta.folder !== target.name) {
        await api.moveMessage(selected.meta.accountId, selected.meta.folder, selected.meta.uid, target.name);
      }
      clearSelection();
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
      clearSelection();
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

  // ── 全局键盘快捷键 ──
  const anyModalOpen =
    composeOpen || accountModal || filtersOpen || profileOpen || riskOpen || newFolderOpen || confirmDelete || !!folderToDelete;
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const meta = e.metaKey || e.ctrlKey;
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
        if (shownThreadMessages.length === 0) return;
        const currentThread = selected ? threadKey(selected.meta) : null;
        const idx = currentThread ? shownThreadMessages.findIndex((m) => threadKey(m) === currentThread) : -1;
        const next = idx < 0 ? 0 : Math.min(shownThreadMessages.length - 1, Math.max(0, idx + (down ? 1 : -1)));
        if (shownThreadMessages[next] && threadKey(shownThreadMessages[next]) !== currentThread) {
          selectMail(shownThreadMessages[next]);
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
      if (refreshed) {
        setSelected(refreshed);
        setSelectedKey(mailKey(refreshed.meta));
      }
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
      setNewFolderErr(t("请输入目录名称"));
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

  async function handleDeleteFolder() {
    if (!folderToDelete) return;
    setDeleteFolderErr(null);
    try {
      await api.deleteFolder(accountId, folderToDelete.name);
      const deletedCurrent = folder === folderToDelete.name;
      if (deletedCurrent) {
        setFolder("INBOX");
        clearSelection();
      }
      await refreshFolders(accountId);
      setFolderToDelete(null);
      if (!deletedCurrent) await loadMessages();
    } catch (e) {
      setDeleteFolderErr(String(e));
    }
  }

  async function handleRemoveAccount() {
    if (!accountToRemove) return;
    setRemoveAccountErr(null);
    try {
      await api.removeAccount(accountToRemove.id);
      const nextState = await api.getState();
      setState(nextState);
      setAccountToRemove(null);
      clearSelection();
      setMessages([]);
      setInboxMetas([]);
      setThread([]);
      setTotal(0);
      setSearch("");
      setListError(null);
      setFolder("INBOX");
      setView("mail");

      const nextAccount =
        nextState.accounts.find((a) => a.id !== accountToRemove.id) ?? nextState.accounts[0];
      setAccountId(nextAccount?.id ?? "");
      if (!nextAccount) setFolders(BUILTIN_FOLDERS);
    } catch (e) {
      setRemoveAccountErr(String(e));
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
              <input ref={searchRef} placeholder={t("搜索邮件、发件人或地址…")} value={search} onChange={(e) => setSearch(e.target.value)} />
              {search && (
                <button className="search-clear" onClick={() => setSearch("")} title={t("清空搜索")}>
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
            <span style={{ fontSize: 15, lineHeight: 1, marginTop: -1 }}>✎</span> {t("写邮件")}
          </button>
        )}
      </div>

      {bootError && <div className="demo-banner">{t("初始化失败：")}{bootError}</div>}

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
            riskCount={riskUnread}
            unifiedUnread={unifiedUnread}
            inboxUnread={inboxUnread}
            draftCount={drafts.filter((d) => d.accountId === accountId).length}
            view={view}
            ledgerMode={ledgerMode}
            onSelectAccount={(id) => {
              fetchRequests.current.invalidate();
              setLoading(false);
              setLoadingMore(false);
              setSyncing(false);
              setAccountId(id);
              setView("mail");
            }}
            onSelectFolder={(f) => {
              fetchRequests.current.invalidate();
              setLoading(false);
              setLoadingMore(false);
              setSyncing(false);
              setFolder(f);
              clearSelection();
              setView("mail");
            }}
            onOpenKeys={() => setView("keys")}
            onAddAccount={() => setAccountModal(true)}
            onRemoveAccount={(account) => {
              setRemoveAccountErr(null);
              setAccountToRemove(account);
            }}
            onNewFolder={() => setNewFolderOpen(true)}
            onDeleteFolder={(f) => {
              setDeleteFolderErr(null);
              setFolderToDelete(f);
            }}
            onOpenFilters={() => setFiltersOpen(true)}
          />
          <PaneResizer
            title={t("拖动调整侧栏宽度")}
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
                title={t(folders.find((f) => f.name === folder)?.display ?? folder)}
                messages={shownMessages}
                selectedKey={selectedKey}
                accountLabels={folder === UNIFIED_FOLDER ? accountLabels : undefined}
                loading={loading}
                syncing={syncing}
                error={listError}
                notice={loadMoreNotice}
                filterMode={filterMode}
                categoryMode={categoryMode}
                categoryCounts={categoryCounts}
                unreadCount={listUnread}
                loadedCount={shownThreadMessages.length}
                totalCount={folder === RISK_FOLDER ? messages.length : total}
                hasMore={folder !== RISK_FOLDER && !olderExhausted}
                loadingMore={loadingMore}
                onFilterMode={setFilterMode}
                onCategoryMode={setCategoryMode}
                onMarkAllRead={handleMarkAllRead}
                onToggleFlag={handleToggleFlag}
                onLoadMore={handleLoadMore}
                onSelect={selectMail}
                onOpenWindow={openMailWindow}
                onRefresh={loadMessages}
              />
              <PaneResizer
                title={t("拖动调整列表宽度")}
                onStart={() => {
                  listDragBase.current = listWidth;
                }}
                onDrag={(dx) => setListWidth(clamp(listDragBase.current + dx, 320, 520))}
              />
              <MessageView
                mail={selected}
                thread={thread}
                threadFulls={threadFulls}
                folders={folders}
                onOpenThreadMail={selectMail}
                onLoadThreadMail={loadThreadFull}
                onReply={handleReply}
                onReplyAll={handleReplyAll}
                onForward={handleForward}
                onMove={handleMove}
                canMove={folder !== UNIFIED_FOLDER}
                canArchive={!selectedInArchive}
                onArchive={handleArchive}
                onDelete={handleDelete}
                onShowRisk={() => setRiskOpen(true)}
                riskAcked={!!selected && riskAcked.has(riskKey(selected))}
                onTrustSender={handleTrustSender}
                onOpenProfile={() => setProfileOpen(true)}
                onMarkUnread={handleMarkUnread}
                onToggleFlag={() => selected && handleToggleFlag(selected.meta)}
                onBlockSender={handleBlockSender}
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

      {riskOpen && selected && <RiskModal mail={selected} onClose={() => setRiskOpen(false)} onConfirm={() => ackRisk(selected)} />}

      {confirmDelete && selected && (
        <div className="overlay" onClick={() => setConfirmDelete(false)}>
          <div className="modal" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <span className="title">{t("永久删除")}</span>
              <button className="modal-close" onClick={() => setConfirmDelete(false)}>
                ×
              </button>
            </div>
            <div className="modal-body" style={{ fontSize: 13, lineHeight: 1.7, color: "var(--mut)" }}>
              「{selected.meta.subject}」{t("已在回收站中，继续删除将")}<b style={{ color: "#9A2C1D" }}>{t("从服务器上永久移除，无法恢复")}</b>。
            </div>
            <div className="modal-foot">
              <button className="btn-ghost" style={{ height: 40 }} onClick={() => setConfirmDelete(false)}>
                {t("取消")}
              </button>
              <button
                className="btn-primary"
                style={{ height: 40, background: "#9A2C1D" }}
                onClick={() => doDelete(true)}
              >
                {t("永久删除")}
              </button>
            </div>
          </div>
        </div>
      )}

      {folderToDelete && (
        <div className="overlay" onClick={() => setFolderToDelete(null)}>
          <div className="modal" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <span className="title">{t("删除目录")}</span>
              <button className="modal-close" onClick={() => setFolderToDelete(null)}>
                ×
              </button>
            </div>
            <div className="modal-body" style={{ fontSize: 13, lineHeight: 1.7, color: "var(--mut)" }}>
              {t("将移除目录「{name}」。如果服务器允许删除，会同时删除该目录中的邮件；如果服务器拒绝删除，会从侧栏隐藏这个目录。", { name: folderToDelete.display })}
              {deleteFolderErr && (
                <div className="form-error" style={{ marginTop: 12, overflowWrap: "anywhere" }}>
                  {deleteFolderErr}
                </div>
              )}
            </div>
            <div className="modal-foot">
              <button className="btn-ghost" style={{ height: 40 }} onClick={() => setFolderToDelete(null)}>
                {t("取消")}
              </button>
              <button
                className="btn-primary"
                style={{ height: 40, background: "#9A2C1D" }}
                onClick={handleDeleteFolder}
              >
                {t("删除目录")}
              </button>
            </div>
          </div>
        </div>
      )}

      {accountToRemove && (
        <div className="overlay" onClick={() => setAccountToRemove(null)}>
          <div className="modal" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <span className="title">{t("删除账户")}</span>
              <button className="modal-close" onClick={() => setAccountToRemove(null)}>
                ×
              </button>
            </div>
            <div className="modal-body" style={{ fontSize: 13, lineHeight: 1.7, color: "var(--mut)" }}>
              {t("将从本机移除账户")} <b style={{ color: "var(--ink-2)" }}>{accountToRemove.email}</b> {t("的配置和登录凭据。这不会删除邮箱服务器上的邮件。")}
              {removeAccountErr && (
                <div className="form-error" style={{ marginTop: 12, overflowWrap: "anywhere" }}>
                  {removeAccountErr}
                </div>
              )}
            </div>
            <div className="modal-foot">
              <button className="btn-ghost" style={{ height: 40 }} onClick={() => setAccountToRemove(null)}>
                {t("取消")}
              </button>
              <button
                className="btn-primary"
                style={{ height: 40, background: "#9A2C1D" }}
                onClick={handleRemoveAccount}
              >
                {t("删除账户")}
              </button>
            </div>
          </div>
        </div>
      )}

      {newFolderOpen && (
        <div className="overlay" onClick={() => setNewFolderOpen(false)}>
          <div className="modal" style={{ width: 420 }} onClick={(e) => e.stopPropagation()}>
            <div className="modal-head">
              <span className="title">{t("新建目录")}</span>
              <button className="modal-close" onClick={() => setNewFolderOpen(false)}>
                ×
              </button>
            </div>
            <div className="modal-body" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
              <div className="field">
                <label>{t("目录名称")}</label>
                <input
                  className="input"
                  placeholder={t("例如：重要客户 / 发票 / 通知")}
                  value={newFolderName}
                  autoFocus
                  onChange={(e) => setNewFolderName(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && handleCreateFolder()}
                />
              </div>
              <div style={{ fontSize: 11, color: "var(--mut-3)", lineHeight: 1.6 }}>
                {t("IMAP 账户会在邮件服务器上创建真实目录；POP3 账户使用本地目录。配合「过滤规则」可把某一类邮件自动归入该目录。")}
              </div>
              {newFolderErr && <div className="form-error">{newFolderErr}</div>}
            </div>
            <div className="modal-foot">
              <span />
              <button className="btn-primary" style={{ height: 40 }} onClick={handleCreateFolder}>
                {t("创建")}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
