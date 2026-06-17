import type { UIEvent } from "react";
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
  error: string | null;
  notice?: string | null;
  filterMode: "all" | "unread" | "flagged";
  categoryMode: MailCategory;
  categoryCounts: Record<MailCategory, number>;
  categoryUnreadCounts: Record<MailCategory, number>;
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
}

const BAR_COLOR: Record<string, string> = {
  verified: "#087443",
  signedUnknown: "#657164",
  unsigned: "#929b93",
  tampered: "#9f2f24",
  impersonation: "#9f2f24",
};

export function MailList(p: Props) {
  function handleScroll(e: UIEvent<HTMLDivElement>) {
    if (!p.hasMore || p.loadingMore || p.loading || p.error) return;
    const el = e.currentTarget;
    if (el.scrollHeight - el.scrollTop - el.clientHeight < 180) p.onLoadMore();
  }

  const rows = Array.from(
    p.messages
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
      selected: sorted.some((m) => `${m.accountId}/${m.folder}/${m.uid}` === p.selectedKey),
      from: senders.length <= 2 ? senders.join(", ") : `${senders.slice(0, 2).join(", ")} 等`,
      flagged: sorted.some((m) => m.flagged),
      hasAttach: sorted.some((m) => m.hasAttach),
      risk: sorted.find((m) => m.risk)?.risk ?? latest.risk,
    };
  });

  return (
    <div className="list-pane" style={{ width: p.width }}>
      <div className="list-head">
        <div className="title">{p.title ?? "邮件"}</div>
        <span className="meta">
          {p.syncing && <span className="sync-dot" title="同步中" />}
          <span className="cache-count" title="当前筛选显示 / 本地已缓存">
            显示 {p.loadedCount.toLocaleString()} · 缓存 {p.totalCount.toLocaleString()}
          </span>
          {p.unreadCount > 0 && (
            <button className="icon-btn" title="全部标为已读" onClick={p.onMarkAllRead}>
              ✓✓
            </button>
          )}
          <button className="icon-btn" title="刷新" onClick={p.onRefresh}>
            ↻
          </button>
        </span>
      </div>
      <div className="list-filterbar">
        <div className="filter-segs">
          <button className={`seg${p.filterMode === "all" ? " on" : ""}`} onClick={() => p.onFilterMode("all")}>
            全部
          </button>
          <button className={`seg${p.filterMode === "unread" ? " on" : ""}`} onClick={() => p.onFilterMode("unread")}>
            未读{p.unreadCount > 0 ? ` ${p.unreadCount}` : ""}
          </button>
          <button className={`seg${p.filterMode === "flagged" ? " on" : ""}`} onClick={() => p.onFilterMode("flagged")}>
            ★ 星标
          </button>
        </div>
        <span className="list-count">{p.hasMore ? "可继续加载" : "已缓存"}</span>
      </div>
      <div className="list-categorybar">
        {(["personal", "business", "ads", "all"] as const).map((c) => (
          <button className={`category-seg${p.categoryMode === c ? " on" : ""}`} key={c} onClick={() => p.onCategoryMode(c)}>
            {CATEGORY_LABEL[c]}
            <span>{p.categoryCounts[c]}</span>
            {p.categoryUnreadCounts[c] > 0 && <b className="category-unread">{p.categoryUnreadCounts[c]}</b>}
          </button>
        ))}
      </div>
      <div className="list-scroll" onScroll={handleScroll}>
        {p.loading && p.messages.length === 0 && <div className="empty-pane">正在读取本地缓存…</div>}
        {p.error && p.messages.length > 0 && <div className="list-error-bar">⚠ {p.error}</div>}
        {!p.error && p.notice && <div className="list-notice-bar">{p.notice}</div>}
        {!p.loading && p.error && p.messages.length === 0 && (
          <div className="empty-pane">
            <div style={{ fontSize: 20 }}>⚠</div>
            {p.error}
            <button className="btn-ghost" onClick={p.onRefresh}>
              重试
            </button>
          </div>
        )}
        {!p.loading && !p.error && p.messages.length === 0 && (
          <div className="empty-pane">
            <div style={{ fontSize: 22, color: "var(--mut-4)" }}>▤</div>
            此目录暂无邮件
          </div>
        )}
        {rows.map((row) => {
          const m = row.latest;
          const category = classifyMail(m);
          return (
            <div
              key={row.key}
              className={`mail-row${row.selected ? " selected" : ""}${row.unreadCount > 0 ? " unread" : ""}`}
              style={{ borderLeftColor: row.selected ? BAR_COLOR[m.trust] : "transparent" }}
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
                    title={row.flagged ? "取消星标" : "加星标"}
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
                  <span className={`tag ${m.trust}`}>{TRUST_LABEL[m.trust]}</span>
                  {p.accountLabels?.[m.accountId] && <span className="tag lang">{p.accountLabels[m.accountId]}</span>}
                  {row.risk && <span className="tag risk">⚠ 高风险</span>}
                  <span className={`tag category ${category}`}>{CATEGORY_TAG[category]}</span>
                  <span className="tag lang">{m.lang}</span>
                  {row.hasAttach && <span className="tag lang">📎</span>}
                </div>
              </div>
            </div>
          );
        })}
        {!p.loading && p.hasMore && (
          <button className="load-more" onClick={p.onLoadMore} disabled={p.loadingMore}>
            {p.loadingMore ? "正在加载…" : "加载更早的邮件"}
          </button>
        )}
      </div>
    </div>
  );
}
