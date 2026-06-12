import { useState } from "react";
import { applyFilters, deleteFilter, saveFilter } from "../api";
import type { Account, FilterRule, FolderInfo } from "../types";

interface Props {
  filters: FilterRule[];
  folders: FolderInfo[];
  accounts: Account[];
  currentAccountId: string;
  onClose: () => void;
  onChanged: (rules: FilterRule[]) => void;
  onApplied: () => void;
}

const FIELD_LABEL: Record<string, string> = { from: "发件人", to: "收件人", subject: "主题", body: "正文" };
const OP_LABEL: Record<string, string> = {
  contains: "包含",
  not_contains: "不包含",
  equals: "等于",
  starts_with: "开头是",
  ends_with: "结尾是",
};

const emptyRule = (): FilterRule => ({
  id: "",
  name: "",
  accountId: null,
  field: "from",
  op: "contains",
  value: "",
  targetFolder: "",
  markRead: false,
  enabled: true,
});

export function FiltersModal(p: Props) {
  const [editing, setEditing] = useState<FilterRule | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [applyMsg, setApplyMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const realFolders = p.folders.filter((f) => f.name !== "__risk__" && f.name !== "INBOX");

  async function doSave() {
    if (!editing) return;
    if (!editing.value.trim()) return setError("请填写匹配内容");
    if (!editing.targetFolder) return setError("请选择目标目录");
    setError(null);
    setBusy(true);
    try {
      const rule = { ...editing, name: editing.name.trim() || `${FIELD_LABEL[editing.field]}${OP_LABEL[editing.op]}「${editing.value}」` };
      const rules = await saveFilter(rule);
      p.onChanged(rules);
      setEditing(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function doDelete(id: string) {
    setBusy(true);
    try {
      const rules = await deleteFilter(id);
      p.onChanged(rules);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function doApply() {
    setBusy(true);
    setApplyMsg(null);
    setError(null);
    try {
      const r = await applyFilters(p.currentAccountId);
      setApplyMsg(
        r.moved === 0
          ? "已执行：收件箱中没有匹配的邮件"
          : `已整理 ${r.moved} 封：\n${r.details.join("\n")}`
      );
      p.onApplied();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="overlay">
      <div className="modal" style={{ width: 620 }}>
        <div className="modal-head">
          <span className="title">过滤规则 · 自动归类</span>
          <button className="modal-close" onClick={p.onClose}>
            ×
          </button>
        </div>
        <div className="modal-body" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          {!editing && (
            <>
              <div style={{ fontSize: 12, color: "var(--mut)", lineHeight: 1.6 }}>
                规则按顺序匹配收件箱中的邮件，命中后移动到目标目录。可手动「立即整理」，每次刷新收件箱后也可以一键执行。
              </div>
              {p.filters.length === 0 && (
                <div className="card-list" style={{ padding: "20px 18px", fontSize: 12.5, color: "var(--mut)" }}>
                  还没有规则。点击下方「新建规则」，例如：发件人 包含 github.com → 移动到「通知」。
                </div>
              )}
              {p.filters.map((r) => (
                <div className="rule-row" key={r.id}>
                  <div className="desc">
                    <b>{r.name}</b>
                    <br />
                    {FIELD_LABEL[r.field]} {OP_LABEL[r.op]} 「{r.value}」 <span className="arrow">→</span> {r.targetFolder}
                    {r.markRead ? " · 标为已读" : ""}
                  </div>
                  <button className="btn-ghost" onClick={() => setEditing({ ...r })}>
                    编辑
                  </button>
                  <button className="icon-btn" title="删除" onClick={() => doDelete(r.id)}>
                    ×
                  </button>
                </div>
              ))}
              <button className="dashed-add" onClick={() => setEditing(emptyRule())}>
                + 新建规则
              </button>
              {applyMsg && <div className="form-ok" style={{ whiteSpace: "pre-wrap" }}>{applyMsg}</div>}
              {error && <div className="form-error">{error}</div>}
            </>
          )}

          {editing && (
            <>
              <div className="field">
                <label>规则名称（可留空自动生成）</label>
                <input className="input" value={editing.name} onChange={(e) => setEditing({ ...editing, name: e.target.value })} />
              </div>
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
                <div className="field">
                  <label>匹配字段</label>
                  <select
                    className="select"
                    value={editing.field}
                    onChange={(e) => setEditing({ ...editing, field: e.target.value as FilterRule["field"] })}
                  >
                    {Object.entries(FIELD_LABEL).map(([k, v]) => (
                      <option key={k} value={k}>
                        {v}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="field">
                  <label>条件</label>
                  <select
                    className="select"
                    value={editing.op}
                    onChange={(e) => setEditing({ ...editing, op: e.target.value as FilterRule["op"] })}
                  >
                    {Object.entries(OP_LABEL).map(([k, v]) => (
                      <option key={k} value={k}>
                        {v}
                      </option>
                    ))}
                  </select>
                </div>
              </div>
              <div className="field">
                <label>匹配内容</label>
                <input
                  className="input"
                  placeholder="例如 github.com / 发票 / urgent"
                  value={editing.value}
                  onChange={(e) => setEditing({ ...editing, value: e.target.value })}
                />
              </div>
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
                <div className="field">
                  <label>移动到目录</label>
                  <select
                    className="select"
                    value={editing.targetFolder}
                    onChange={(e) => setEditing({ ...editing, targetFolder: e.target.value })}
                  >
                    <option value="">选择目录…</option>
                    {realFolders.map((f) => (
                      <option key={f.name} value={f.name}>
                        {f.display}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="field">
                  <label>适用账户</label>
                  <select
                    className="select"
                    value={editing.accountId ?? ""}
                    onChange={(e) => setEditing({ ...editing, accountId: e.target.value || null })}
                  >
                    <option value="">全部账户</option>
                    {p.accounts
                                            .map((a) => (
                        <option key={a.id} value={a.id}>
                          {a.email}
                        </option>
                      ))}
                  </select>
                </div>
              </div>
              <label style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12.5, color: "var(--ink-3)", cursor: "pointer" }}>
                <input
                  type="checkbox"
                  checked={editing.markRead}
                  onChange={(e) => setEditing({ ...editing, markRead: e.target.checked })}
                  style={{ accentColor: "#1E6B49" }}
                />
                移动后标为已读
              </label>
              {error && <div className="form-error">{error}</div>}
            </>
          )}
        </div>
        <div className="modal-foot">
          {!editing ? (
            <>
              <span className="toolbar-note">{p.filters.length} 条规则</span>
              <button className="btn-primary" style={{ height: 40 }} disabled={busy || p.filters.length === 0} onClick={doApply}>
                {busy ? "整理中…" : "立即整理收件箱"}
              </button>
            </>
          ) : (
            <>
              <button className="btn-ghost" style={{ height: 40 }} onClick={() => setEditing(null)}>
                返回
              </button>
              <button className="btn-primary" style={{ height: 40 }} disabled={busy} onClick={doSave}>
                保存规则
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
