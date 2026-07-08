import { useI18n } from "../i18n";
import type { Draft } from "../types";

interface Props {
  drafts: Draft[];
  onOpen: (d: Draft) => void;
  onDelete: (d: Draft) => void;
}

function fmtTime(ts: number) {
  const d = new Date(ts * 1000);
  const now = new Date();
  const sameDay = d.toDateString() === now.toDateString();
  return sameDay
    ? d.toLocaleTimeString("zh-CN", { hour: "2-digit", minute: "2-digit" })
    : d.toLocaleDateString("zh-CN", { month: "numeric", day: "numeric" });
}

export function DraftsPane(p: Props) {
  const t = useI18n();
  return (
    <div className="list-pane" style={{ flex: 1, borderRight: "none" }}>
      <div className="list-head">
        <span className="title">{t("草稿")}</span>
        <span className="meta">{t("{n} 篇", { n: p.drafts.length })}</span>
      </div>
      <div className="list-scroll">
        {p.drafts.length === 0 && (
          <div className="empty-pane">
            <div style={{ fontSize: 22, color: "var(--mut-4)" }}>✎</div>
            {t("没有草稿。写信时会自动保存，关掉也不丢。")}
          </div>
        )}
        {p.drafts.map((d) => (
          <div key={d.id} className="mail-row" onClick={() => p.onOpen(d)}>
            <div style={{ flex: 1, minWidth: 0 }}>
              <div className="top">
                <span className="from">{d.to || t("（未填收件人）")}</span>
                <span className="time">{fmtTime(d.updatedAt)}</span>
              </div>
              <div className="subject">{d.subject || t("（无主题）")}</div>
              <div className="preview">{d.body.split("\n").find((l) => l.trim()) || " "}</div>
            </div>
            <button
              className="icon-btn"
              title={t("删除草稿")}
              onClick={(e) => {
                e.stopPropagation();
                p.onDelete(d);
              }}
            >
              ×
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}
