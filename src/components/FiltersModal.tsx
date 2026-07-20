import { useI18n } from "../i18n";
import { useState } from "react";
import { applyFilters, deleteFilter, saveFilter } from "../api";
import { folderTitle } from "../mutf7";
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
  const t = useI18n();
  const [editing, setEditing] = useState<FilterRule | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [applyMsg, setApplyMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const realFolders = p.folders.filter((f) => f.name !== "__risk__" && f.name !== "INBOX");

  // 规则里存的是 IMAP 原始目录名（Modified UTF-7，如 &V4NXPpCuTvY- = 垃圾邮件），展示时解码
  const folderLabel = (name: string) =>
    t(folderTitle(name, p.folders.find((f) => f.name === name)?.display));

  async function doSave() {
    if (!editing) return;
    if (!editing.value.trim()) return setError(t("请填写匹配内容"));
    if (!editing.targetFolder) return setError(t("请选择目标目录"));
    setError(null);
    setBusy(true);
    try {
      const rule = { ...editing, name: editing.name.trim() || `${t(FIELD_LABEL[editing.field])}${t(OP_LABEL[editing.op])}「${editing.value}」` };
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
          ? t("已执行：收件箱中没有匹配的邮件")
          : t("已整理 {n} 封：", { n: r.moved }) + `\n${r.details.join("\n")}`
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
          <span className="title">{t("过滤规则 · 自动归类")}</span>
          <button className="modal-close" onClick={p.onClose}>
            ×
          </button>
        </div>
        <div className="modal-body" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          {!editing && (
            <>
              <div style={{ fontSize: 12, color: "var(--mut)", lineHeight: 1.6 }}>
                {t("规则按顺序匹配收件箱中的邮件，命中后移动到目标目录。新邮件到达时自动应用；「立即整理」会对收件箱现有邮件全量执行一遍。")}
              </div>
              {p.filters.length === 0 && (
                <div className="card-list" style={{ padding: "20px 18px", fontSize: 12.5, color: "var(--mut)" }}>
                  {t("还没有规则。点击下方「新建规则」，例如：发件人 包含 github.com → 移动到「通知」。")}
                </div>
              )}
              {p.filters.map((r) => (
                <div className="rule-row" key={r.id}>
                  <div className="desc">
                    <b>{r.name}</b>
                    <br />
                    {t(FIELD_LABEL[r.field])} {t(OP_LABEL[r.op])} 「{r.value}」 <span className="arrow">→</span> {folderLabel(r.targetFolder)}
                    {r.markRead ? ` · ${t("标为已读")}` : ""}
                  </div>
                  <button className="btn-ghost" onClick={() => setEditing({ ...r })}>
                    {t("编辑")}
                  </button>
                  <button className="icon-btn" title={t("删除")} onClick={() => doDelete(r.id)}>
                    ×
                  </button>
                </div>
              ))}
              <button className="dashed-add" onClick={() => setEditing(emptyRule())}>
                {t("+ 新建规则")}
              </button>
              {applyMsg && <div className="form-ok" style={{ whiteSpace: "pre-wrap" }}>{applyMsg}</div>}
              {error && <div className="form-error">{error}</div>}
            </>
          )}

          {editing && (
            <>
              <div className="field">
                <label>{t("规则名称（可留空自动生成）")}</label>
                <input className="input" value={editing.name} onChange={(e) => setEditing({ ...editing, name: e.target.value })} />
              </div>
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
                <div className="field">
                  <label>{t("匹配字段")}</label>
                  <select
                    className="select"
                    value={editing.field}
                    onChange={(e) => setEditing({ ...editing, field: e.target.value as FilterRule["field"] })}
                  >
                    {Object.entries(FIELD_LABEL).map(([k, v]) => (
                      <option key={k} value={k}>
                        {t(v)}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="field">
                  <label>{t("条件")}</label>
                  <select
                    className="select"
                    value={editing.op}
                    onChange={(e) => setEditing({ ...editing, op: e.target.value as FilterRule["op"] })}
                  >
                    {Object.entries(OP_LABEL).map(([k, v]) => (
                      <option key={k} value={k}>
                        {t(v)}
                      </option>
                    ))}
                  </select>
                </div>
              </div>
              <div className="field">
                <label>{t("匹配内容")}</label>
                <input
                  className="input"
                  placeholder={t("例如 github.com / 发票 / urgent")}
                  value={editing.value}
                  onChange={(e) => setEditing({ ...editing, value: e.target.value })}
                />
              </div>
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
                <div className="field">
                  <label>{t("移动到目录")}</label>
                  <select
                    className="select"
                    value={editing.targetFolder}
                    onChange={(e) => setEditing({ ...editing, targetFolder: e.target.value })}
                  >
                    <option value="">{t("选择目录…")}</option>
                    {realFolders.map((f) => (
                      <option key={f.name} value={f.name}>
                        {t(f.display)}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="field">
                  <label>{t("适用账户")}</label>
                  <select
                    className="select"
                    value={editing.accountId ?? ""}
                    onChange={(e) => setEditing({ ...editing, accountId: e.target.value || null })}
                  >
                    <option value="">{t("全部账户")}</option>
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
                  style={{ accentColor: "var(--tone-jade)" }}
                />
                {t("移动后标为已读")}
              </label>
              {error && <div className="form-error">{error}</div>}
            </>
          )}
        </div>
        <div className="modal-foot">
          {!editing ? (
            <>
              <span className="toolbar-note">{t("{n} 条规则", { n: p.filters.length })}</span>
              <button className="btn-primary" style={{ height: 40 }} disabled={busy || p.filters.length === 0} onClick={doApply}>
                {busy ? t("整理中…") : t("立即整理收件箱")}
              </button>
            </>
          ) : (
            <>
              <button className="btn-ghost" style={{ height: 40 }} onClick={() => setEditing(null)}>
                {t("返回")}
              </button>
              <button className="btn-primary" style={{ height: 40 }} disabled={busy} onClick={doSave}>
                {t("保存规则")}
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
