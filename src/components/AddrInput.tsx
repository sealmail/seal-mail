import { useI18n } from "../i18n";
import { useEffect, useRef, useState } from "react";
import { listContacts } from "../api";
import type { Contact } from "../types";

interface Props {
  value: string;
  placeholder: string;
  onChange: (v: string) => void;
}

/**
 * 收件人输入框 + 联系人自动补全。
 * 取光标前最后一个分隔符（, ; 空格）之后的片段作为查询词，
 * 选中后用纯地址替换该片段（发送解析按分隔符拆，不支持 Name <addr> 形式）。
 */
export function AddrInput(p: Props) {
  const t = useI18n();
  const [hits, setHits] = useState<Contact[]>([]);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const ref = useRef<HTMLInputElement>(null);
  const seq = useRef(0);

  function fragment(v: string): string {
    const m = v.match(/[^,;，；\s]*$/);
    return m ? m[0] : "";
  }

  useEffect(() => {
    const q = fragment(p.value).trim();
    if (!q) {
      setOpen(false);
      setHits([]);
      return;
    }
    const mySeq = ++seq.current;
    listContacts(q)
      .then((list) => {
        if (mySeq !== seq.current) return;
        // 已经输入完整且唯一命中的地址就不用再提示了
        const filtered = list.filter((c) => c.email.toLowerCase() !== q.toLowerCase());
        setHits(filtered);
        setOpen(filtered.length > 0);
        setActive(0);
      })
      .catch(() => {});
  }, [p.value]);

  function pick(c: Contact) {
    const frag = fragment(p.value);
    const head = p.value.slice(0, p.value.length - frag.length);
    p.onChange(`${head}${c.email}, `);
    setOpen(false);
    ref.current?.focus();
  }

  const addrs = p.value
    .split(/[,;，；\s]+/)
    .map((x) => x.trim())
    .filter(Boolean);
  const complete = /[,;，；\s]$/.test(p.value);
  const lastLooksReady = !!addrs[addrs.length - 1]?.match(/^[^\s@]+@[^\s@]+\.[^\s@]+$/);
  const preview = complete || lastLooksReady ? addrs : addrs.slice(0, -1);

  function onKeyDown(e: React.KeyboardEvent) {
    if (!open) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((i) => Math.min(hits.length - 1, i + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((i) => Math.max(0, i - 1));
    } else if (e.key === "Enter" || e.key === "Tab") {
      e.preventDefault();
      if (hits[active]) pick(hits[active]);
    } else if (e.key === "Escape") {
      setOpen(false);
    }
  }

  return (
    <div className="addr-wrap">
      <input
        ref={ref}
        className="input mono"
        placeholder={p.placeholder}
        value={p.value}
        onChange={(e) => p.onChange(e.target.value)}
        onKeyDown={onKeyDown}
        onBlur={() => setTimeout(() => setOpen(false), 120)}
      />
      {preview.length > 0 && (
        <div className="addr-preview" aria-live="polite">
          {preview.slice(0, 3).map((addr) => (
            <span className="addr-chip" key={addr}>
              {addr}
            </span>
          ))}
          {preview.length > 3 && <span className="addr-chip muted">+{preview.length - 3}</span>}
          <span className="addr-count">{t("已识别 {n} 个地址", { n: preview.length })}</span>
        </div>
      )}
      {open && (
        <div className="addr-pop">
          {hits.map((c, i) => (
            <div
              key={c.email}
              className={`addr-item${i === active ? " on" : ""}`}
              onMouseDown={(e) => {
                e.preventDefault();
                pick(c);
              }}
              onMouseEnter={() => setActive(i)}
            >
              <span className="addr-name">{c.name || c.email}</span>
              {c.name && <span className="addr-mail">{c.email}</span>}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
