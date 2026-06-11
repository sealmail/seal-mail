import { Seal } from "./Seal";
import { riskBanner } from "../trust";
import type { EmailFull, FolderInfo } from "../types";

interface Props {
  mail: EmailFull | null;
  folders: FolderInfo[];
  onReply: () => void;
  onReplyAll: () => void;
  onForward: () => void;
  onMove: (target: string) => void;
  onDelete: () => void;
  onShowRisk: () => void;
}

function fmtSize(n: number) {
  if (n > 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  if (n > 1024) return `${Math.round(n / 1024)} KB`;
  return `${n} B`;
}

export function MessageView(p: Props) {
  if (!p.mail) {
    return (
      <div className="msg-pane">
        <div className="empty-pane">
          <div style={{ fontSize: 26, color: "#C7C1B2" }}>印</div>
          选择一封邮件查看内容与验证结果
        </div>
      </div>
    );
  }
  const m = p.mail;
  const banner = riskBanner(m);
  const moveTargets = p.folders.filter((f) => f.name !== m.meta.folder && f.name !== "__risk__");

  return (
    <div className="msg-pane">
      <div className="msg-scroll">
        <div className="msg-head">
          <div className="msg-subject">{m.meta.subject}</div>
          <div className="msg-head2">
            <div className="msg-fromline">
              <div style={{ paddingTop: 1 }}>
                <Seal trust={m.meta.trust} size={30} />
              </div>
              <div style={{ minWidth: 0 }}>
                <div className="msg-fromname">{m.meta.fromName}</div>
                <div className="msg-addr">{m.meta.fromAddr}</div>
              </div>
            </div>
            <div className="msg-side">
              <span className="msg-date">{m.meta.dateDisplay}</span>
              <div style={{ display: "flex", gap: 7, flexWrap: "wrap", justifyContent: "flex-end" }}>
                <button className="btn-ghost" onClick={p.onReply}>
                  回复
                </button>
                <button className="btn-ghost" onClick={p.onReplyAll}>
                  回复全部
                </button>
                <button className="btn-ghost" onClick={p.onForward}>
                  转发
                </button>
                <select
                  className="btn-ghost"
                  style={{ paddingRight: 6, maxWidth: 104 }}
                  value=""
                  onChange={(e) => e.target.value && p.onMove(e.target.value)}
                >
                  <option value="">移动到…</option>
                  {moveTargets.map((f) => (
                    <option key={f.name} value={f.name}>
                      {f.display}
                    </option>
                  ))}
                </select>
                <button className="btn-ghost" onClick={p.onDelete} title="删除">
                  删除
                </button>
              </div>
            </div>
          </div>
        </div>

        {banner && (
          <div className={`risk-banner ${banner.cls}`}>
            <div className="icon">{banner.icon}</div>
            <div style={{ flex: 1 }}>
              <div className="title">{banner.title}</div>
              <div className="msg">{banner.msg}</div>
              <div className="actions">
                <button className="btn-solid" style={{ background: banner.solid }} onClick={p.onShowRisk}>
                  {banner.btn}
                </button>
              </div>
            </div>
          </div>
        )}

        <div className="msg-body">{m.bodyText || "(无正文)"}</div>

        {m.attachments.length > 0 && (
          <div className="attach-row">
            {m.attachments.map((a, i) => (
              <div className="attach" key={i}>
                <div className="ext">{(a.name.split(".").pop() || "?").toUpperCase().slice(0, 4)}</div>
                <div>
                  <div className="name">{a.name}</div>
                  <div className="info">
                    {fmtSize(a.size)} · {a.mime}
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
