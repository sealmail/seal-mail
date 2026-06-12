import type { UIEvent } from "react";
import { Seal } from "./Seal";
import { TRUST_LABEL } from "../trust";
import type { EmailMeta } from "../types";

interface Props {
  width?: number;
  title: string;
  messages: EmailMeta[];
  selectedKey: string | null;
  accountLabels?: Record<string, string>;
  loading: boolean;
  syncing: boolean;
  error: string | null;
  filterMode: "all" | "unread" | "flagged";
  unreadCount: number;
  loadedCount: number;
  totalCount: number;
  hasMore: boolean;
  loadingMore: boolean;
  onFilterMode: (m: "all" | "unread" | "flagged") => void;
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

  return (
    <div className="list-pane" style={{ width: p.width }}>
      <div className="list-head">
        <span className="title">{p.title}</span>
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
      <div className="list-scroll" onScroll={handleScroll}>
        {p.loading && <div className="empty-pane">正在读取本地缓存…</div>}
        {!p.loading && p.error && p.messages.length > 0 && <div className="list-error-bar">⚠ {p.error}</div>}
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
        {!p.loading &&
          p.messages.map((m) => {
            const selected = `${m.accountId}/${m.folder}/${m.uid}` === p.selectedKey;
            return (
              <div
                key={`${m.accountId}/${m.folder}/${m.uid}`}
                className={`mail-row${selected ? " selected" : ""}${m.unread ? " unread" : ""}`}
                style={{ borderLeftColor: selected ? BAR_COLOR[m.trust] : "transparent" }}
                onClick={() => p.onSelect(m)}
                onDoubleClick={() => p.onOpenWindow(m)}
              >
                <div className="mail-seal-cell">
                  <Seal trust={m.trust} size={28} />
                </div>
                <div className="mail-main">
                  <div className="top">
                    <div className={`unread-dot${m.unread ? "" : " off"}`} />
                    <span className="from">{m.fromName}</span>
                    <button
                      className={`star-btn${m.flagged ? " on" : ""}`}
                      title={m.flagged ? "取消星标" : "加星标"}
                      onClick={(e) => {
                        e.stopPropagation();
                        p.onToggleFlag(m);
                      }}
                    >
                      {m.flagged ? "★" : "☆"}
                    </button>
                    <span className="time">{m.dateDisplay}</span>
                  </div>
                  <div className="subject">{m.subject}</div>
                  <div className="preview">{m.preview || " "}</div>
                  <div className="tags">
                    <span className={`tag ${m.trust}`}>{TRUST_LABEL[m.trust]}</span>
                    {p.accountLabels?.[m.accountId] && <span className="tag lang">{p.accountLabels[m.accountId]}</span>}
                    {m.risk && <span className="tag risk">⚠ 高风险</span>}
                    <span className="tag lang">{m.lang}</span>
                    {m.hasAttach && <span className="tag lang">📎</span>}
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
