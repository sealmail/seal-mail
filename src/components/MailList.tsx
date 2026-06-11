import { Seal } from "./Seal";
import { TRUST_LABEL } from "../trust";
import type { EmailMeta } from "../types";

interface Props {
  title: string;
  messages: EmailMeta[];
  selectedUid: number | null;
  loading: boolean;
  error: string | null;
  onSelect: (m: EmailMeta) => void;
  onRefresh: () => void;
}

const BAR_COLOR: Record<string, string> = {
  verified: "#1E6B49",
  signedUnknown: "#C99B4E",
  unsigned: "#C7C1B2",
  tampered: "#9A2C1D",
  impersonation: "#9A2C1D",
};

export function MailList(p: Props) {
  return (
    <div className="list-pane">
      <div className="list-head">
        <span className="title">{p.title}</span>
        <span className="meta">
          {p.messages.length} 封
          <button className="icon-btn" title="刷新" onClick={p.onRefresh}>
            ↻
          </button>
        </span>
      </div>
      <div className="list-scroll">
        {p.loading && <div className="empty-pane">正在拉取邮件…</div>}
        {!p.loading && p.error && (
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
            <div style={{ fontSize: 22, color: "#C7C1B2" }}>▤</div>
            此目录暂无邮件
          </div>
        )}
        {!p.loading &&
          !p.error &&
          p.messages.map((m) => {
            const selected = m.uid === p.selectedUid;
            return (
              <div
                key={`${m.accountId}/${m.folder}/${m.uid}`}
                className={`mail-row${selected ? " selected" : ""}${m.unread ? " unread" : ""}`}
                style={{ borderLeftColor: selected ? BAR_COLOR[m.trust] : "transparent" }}
                onClick={() => p.onSelect(m)}
              >
                <div style={{ paddingTop: 2 }}>
                  <Seal trust={m.trust} size={28} />
                </div>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div className="top">
                    {m.unread && <div className="unread-dot" />}
                    <span className="from">{m.fromName}</span>
                    <span className="time">{m.dateDisplay}</span>
                  </div>
                  <div className="subject">{m.subject}</div>
                  <div className="preview">{m.preview || " "}</div>
                  <div className="tags">
                    <span className={`tag ${m.trust}`}>{TRUST_LABEL[m.trust]}</span>
                    {m.risk && <span className="tag risk">⚠ 高风险</span>}
                    <span className="tag lang">{m.lang}</span>
                    {m.hasAttach && <span className="tag lang">📎</span>}
                  </div>
                </div>
              </div>
            );
          })}
      </div>
    </div>
  );
}
