import { useEffect, useMemo, useRef, useState, type UIEvent } from "react";
import type { AppError } from "../errors";
import { useI18n } from "../i18n";
import { CATEGORY_LABEL, CATEGORY_TAG, classifyMail, type MailCategory } from "../mailCategory";
import { Seal } from "./Seal";
import { TRUST_LABEL } from "../trust";
import type { EmailMeta } from "../types";

interface Props {
  width?: number;
  title?: string;
  messages: EmailMeta[];
  selectedKey: string | null;
  accountLabels?: Record<string, string>;
  loading: boolean;
  syncing: boolean;
  error: AppError | string | null;
  notice?: string | null;
  filterMode: "all" | "unread" | "flagged";
  categoryMode: MailCategory;
  categoryCounts: Record<MailCategory, number>;
  unreadCount: number;
  loadedCount: number;
  totalCount: number;
  hasMore: boolean;
  loadingMore: boolean;
  onFilterMode: (m: "all" | "unread" | "flagged") => void;
  onCategoryMode: (m: MailCategory) => void;
  onMarkAllRead: () => void;
  onToggleFlag: (m: EmailMeta) => void;
  onLoadMore: () => void;
  onSelect: (m: EmailMeta) => void;
  onOpenWindow: (m: EmailMeta) => void;
  onRefresh: () => void;
  onDismissError?: () => void;
  onReauth?: () => void;
}

function errorText(error: AppError | string): string {
  return typeof error === "string" ? error : error.message;
}

function errorIsAuth(error: AppError | string): boolean {
  return typeof error !== "string" && error.kind === "auth";
}

const BAR_COLOR: Record<string, string> = {
  verified: "#087443",
  signedUnknown: "#657164",
  unsigned: "#929b93",
  tampered: "#9f2f24",
  impersonation: "#9f2f24",
};

/** 固定行高（与 .mail-row 视觉一致），虚拟列表用 */
const ROW_HEIGHT = 92;
const OVERSCAN = 8;

type ThreadRow = {
  key: string;
  latest: EmailMeta;
  count: number;
  unreadCount: number;
  selected: boolean;
  from: string;
  flagged: boolean;
  hasAttach: boolean;
  risk: EmailMeta["risk"];
};

function groupThreads(messages: EmailMeta[], selectedKey: string | null, t: (k: string, vars?: Record<string, string>) => string): ThreadRow[] {
  return Array.from(
    messages
      .reduce((groups, m) => {
        const key = `${m.accountId}/${m.folder}/${m.threadId || m.messageId || m.uid}`;
        const existing = groups.get(key);
        if (existing) existing.push(m);
        else groups.set(key, [m]);
        return groups;
      }, new Map<string, EmailMeta[]>())
      .values()
  ).map((group) => {
    const sorted = [...group].sort((a, b) => b.timestamp - a.timestamp);
    const latest = sorted[0];
    const senders = [...new Set(sorted.map((m) => m.fromName).filter(Boolean))];
    return {
      key: `${latest.accountId}/${latest.folder}/${latest.threadId || latest.messageId || latest.uid}`,
      latest,
      count: sorted.length,
      unreadCount: sorted.filter((m) => m.unread).length,
      selected: sorted.some((m) => `${m.accountId}/${m.folder}/${m.uid}` === selectedKey),
      from: senders.length <= 2 ? senders.join(", ") : t("{names} 等", { names: senders.slice(0, 2).join(", ") }),
      flagged: sorted.some((m) => m.flagged),
      hasAttach: sorted.some((m) => m.hasAttach),
      risk: sorted.find((m) => m.risk)?.risk ?? latest.risk,
    };
  });
}

export function MailList(p: Props) {
  const t = useI18n();
  const scrollRef = useRef<HTMLDivElement>(null);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportH, setViewportH] = useState(600);

  const rows = useMemo(() => groupThreads(p.messages, p.selectedKey, t), [p.messages, p.selectedKey, t]);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const measure = () => setViewportH(el.clientHeight || 600);
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // 筛选/目录变化时滚回顶部
  useEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = 0;
    setScrollTop(0);
  }, [p.filterMode, p.categoryMode, p.title]);

  // j/k 或程序选中后，把选中行滚进可视区（虚拟列表不会自动 scrollIntoView）。
  // 只在 selectedKey 真正变化时滚动：后台同步刷新 rows 不能把用户手动滚走的
  // 列表又拽回选中行。
  const scrolledForKeyRef = useRef<string | null>(null);
  useEffect(() => {
    if (p.selectedKey === scrolledForKeyRef.current) return;
    if (!p.selectedKey) {
      scrolledForKeyRef.current = null;
      return;
    }
    const el = scrollRef.current;
    if (!el) return;
    const idx = rows.findIndex((r) => r.selected);
    if (idx < 0) return; // 选中行还没进列表（rows 稍后更新），届时再滚
    scrolledForKeyRef.current = p.selectedKey;
    const rowTop = idx * ROW_HEIGHT;
    const rowBottom = rowTop + ROW_HEIGHT;
    const viewTop = el.scrollTop;
    const viewBottom = viewTop + el.clientHeight;
    if (rowTop < viewTop) {
      el.scrollTop = rowTop;
      setScrollTop(rowTop);
    } else if (rowBottom > viewBottom) {
      const next = Math.max(0, rowBottom - el.clientHeight);
      el.scrollTop = next;
      setScrollTop(next);
    }
  }, [p.selectedKey, rows]);

  function handleScroll(e: UIEvent<HTMLDivElement>) {
    const el = e.currentTarget;
    setScrollTop(el.scrollTop);
    // 接近底部且服务器还有更早邮件时，触发从网络补缓存（本地列表本身已是全量）
    if (!p.hasMore || p.loadingMore || p.loading || p.error) return;
    if (el.scrollHeight - el.scrollTop - el.clientHeight < 240) p.onLoadMore();
  }

  const totalH = rows.length * ROW_HEIGHT;
  const start = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN);
  const visibleCount = Math.ceil(viewportH / ROW_HEIGHT) + OVERSCAN * 2;
  const end = Math.min(rows.length, start + visibleCount);
  const padTop = start * ROW_HEIGHT;
  const padBottom = Math.max(0, totalH - end * ROW_HEIGHT);
  const visibleRows = rows.slice(start, end);

  return (
    <div className="list-pane" style={{ width: p.width }}>
      <div className="list-head">
        <div className="title">{p.title ?? t("邮件")}</div>
        <span className="meta">
          {p.syncing && (
            <span className="sync-chip" title={t("正在与服务器同步新邮件")}>
              {t("同步中")}
            </span>
          )}
          <span className="cache-count" title={t("当前筛选显示 / 本地已缓存")}>
            {t("显示 {a} · 缓存 {b}", { a: p.loadedCount.toLocaleString(), b: p.totalCount.toLocaleString() })}
          </span>
          {p.unreadCount > 0 && (
            <button className="icon-btn" title={t("全部标为已读")} onClick={p.onMarkAllRead}>
              ✓✓
            </button>
          )}
          <button className="icon-btn" title={t("刷新")} onClick={p.onRefresh}>
            ↻
          </button>
        </span>
      </div>
      <div className="list-filterbar">
        <div className="filter-segs">
          <button className={`seg${p.filterMode === "all" ? " on" : ""}`} onClick={() => p.onFilterMode("all")}>
            {t("全部")}
          </button>
          <button className={`seg${p.filterMode === "unread" ? " on" : ""}`} onClick={() => p.onFilterMode("unread")}>
            {t("未读")}{p.unreadCount > 0 ? ` ${p.unreadCount}` : ""}
          </button>
          <button className={`seg${p.filterMode === "flagged" ? " on" : ""}`} onClick={() => p.onFilterMode("flagged")}>
            {t("★ 星标")}
          </button>
        </div>
        <span className="list-count">{p.hasMore ? t("可从服务器补全更早邮件") : t("已缓存")}</span>
      </div>
      <div className="list-categorybar">
        {(["personal", "business", "ads", "all"] as const).map((c) => (
          <button className={`category-seg${p.categoryMode === c ? " on" : ""}`} key={c} onClick={() => p.onCategoryMode(c)}>
            {t(CATEGORY_LABEL[c])}
            <span>{p.categoryCounts[c]}</span>
          </button>
        ))}
      </div>
      <div className="list-scroll" ref={scrollRef} onScroll={handleScroll}>
        {p.loading && p.messages.length === 0 && <div className="empty-pane">{t("正在读取本地缓存…")}</div>}
        {p.error && p.messages.length > 0 && (
          <div className="list-error-bar">
            <span className="list-error-text">⚠ {errorText(p.error)}</span>
            <span className="list-error-actions">
              {errorIsAuth(p.error) && p.onReauth && (
                <button type="button" className="list-error-btn" onClick={p.onReauth}>
                  {t("重新授权")}
                </button>
              )}
              {p.onDismissError && (
                <button type="button" className="list-error-btn" onClick={p.onDismissError} title={t("收起错误")}>
                  ×
                </button>
              )}
            </span>
          </div>
        )}
        {!p.error && p.notice && <div className="list-notice-bar">{p.notice}</div>}
        {!p.loading && p.error && p.messages.length === 0 && (
          <div className="empty-pane">
            <div style={{ fontSize: 20 }}>⚠</div>
            {errorText(p.error)}
            <div style={{ display: "flex", gap: 8, marginTop: 8 }}>
              {errorIsAuth(p.error) && p.onReauth && (
                <button className="btn-primary" style={{ height: 34 }} onClick={p.onReauth}>
                  {t("重新授权")}
                </button>
              )}
              <button className="btn-ghost" onClick={p.onRefresh}>
                {t("重试")}
              </button>
            </div>
          </div>
        )}
        {!p.loading && !p.error && p.messages.length === 0 && (
          <div className="empty-pane">
            <div style={{ fontSize: 22, color: "var(--mut-4)" }}>▤</div>
            {t("此目录暂无邮件")}
          </div>
        )}
        {rows.length > 0 && (
          <div className="list-virtual" style={{ height: totalH, position: "relative" }}>
            <div style={{ height: padTop }} />
            {visibleRows.map((row) => {
              const m = row.latest;
              const category = classifyMail(m);
              return (
                <div
                  key={row.key}
                  className={`mail-row${row.selected ? " selected" : ""}${row.unreadCount > 0 ? " unread" : ""}`}
                  style={{
                    borderLeftColor: row.selected ? BAR_COLOR[m.trust] : "transparent",
                    height: ROW_HEIGHT,
                    boxSizing: "border-box",
                  }}
                  onClick={() => p.onSelect(m)}
                  onDoubleClick={() => p.onOpenWindow(m)}
                >
                  {row.unreadCount > 0 && <div className="mail-unread-dot" />}
                  <div className="mail-seal-cell">
                    <Seal trust={m.trust} size={28} />
                  </div>
                  <div className="mail-main">
                    <div className="top">
                      <span className="from">{row.from || m.fromName}</span>
                      <button
                        className={`star-btn${row.flagged ? " on" : ""}`}
                        title={row.flagged ? t("取消星标") : t("加星标")}
                        onClick={(e) => {
                          e.stopPropagation();
                          p.onToggleFlag(m);
                        }}
                      >
                        {row.flagged ? "★" : "☆"}
                      </button>
                      <span className="time">
                        {row.count > 1 && <b className="thread-count">{row.count}</b>}
                        {m.dateDisplay}
                      </span>
                    </div>
                    <div className="subject">{m.subject}</div>
                    <div className="preview">{m.preview || " "}</div>
                    <div className="tags">
                      <span className={`tag ${m.trust}`}>{t(TRUST_LABEL[m.trust])}</span>
                      {p.accountLabels?.[m.accountId] && <span className="tag lang">{p.accountLabels[m.accountId]}</span>}
                      {row.risk && <span className="tag risk">⚠ {t("高风险")}</span>}
                      <span className={`tag category ${category}`}>{t(CATEGORY_TAG[category])}</span>
                      <span className="tag lang">{m.lang}</span>
                      {row.hasAttach && <span className="tag lang">📎</span>}
                    </div>
                  </div>
                </div>
              );
            })}
            <div style={{ height: padBottom }} />
          </div>
        )}
        {!p.loading && p.hasMore && (
          <button className="load-more" onClick={p.onLoadMore} disabled={p.loadingMore}>
            {p.loadingMore ? t("正在加载…") : t("从服务器加载更早的邮件")}
          </button>
        )}
      </div>
    </div>
  );
}
