import { useEffect, useMemo, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

/** 整体移除的危险标签（脚本、外部加载、表单提交） */
const REMOVE_TAGS = "script,iframe,object,embed,form,base,meta,link,applet,frame,frameset,audio,video";

/**
 * 邮件 HTML 消毒：
 * - 去掉脚本/iframe/表单等危险标签与全部 on* 事件属性
 * - 去掉 javascript:/vbscript:/data:text/html 协议链接
 * - allowRemote=false 时阻断 http(s) 图片与 style 里的远程 url()（防追踪像素）
 * - cid: 内嵌图片暂不支持，移除避免裂图请求
 */
export function sanitizeEmailHtml(html: string, allowRemote: boolean): { doc: string; blocked: number } {
  const parsed = new DOMParser().parseFromString(html, "text/html");
  let blocked = 0;
  parsed.querySelectorAll(REMOVE_TAGS).forEach((el) => el.remove());

  const walk = (el: Element) => {
    for (const attr of Array.from(el.attributes)) {
      const n = attr.name.toLowerCase();
      const v = attr.value.trim().toLowerCase().replace(/[\s\x00-\x1f]+/g, "");
      if (n.startsWith("on")) {
        el.removeAttribute(attr.name);
      } else if (
        (n === "href" || n === "src" || n === "xlink:href" || n === "action" || n === "formaction") &&
        (v.startsWith("javascript:") || v.startsWith("vbscript:") || v.startsWith("data:text/html"))
      ) {
        el.removeAttribute(attr.name);
      } else if (n === "style" && !allowRemote && /url\s*\(/i.test(attr.value)) {
        blocked++;
        el.setAttribute("style", attr.value.replace(/url\s*\([^)]*\)/gi, "none"));
      } else if (n === "srcset" && !allowRemote) {
        el.removeAttribute(attr.name);
      }
    }
    if (el.tagName === "IMG") {
      const src = el.getAttribute("src") ?? "";
      if (/^(https?:)?\/\//i.test(src)) {
        if (!allowRemote) {
          blocked++;
          el.removeAttribute("src");
          el.setAttribute("data-blocked", "1");
        }
      } else if (src.toLowerCase().startsWith("cid:")) {
        el.removeAttribute("src");
        el.setAttribute("data-blocked", "1");
      }
    }
    Array.from(el.children).forEach(walk);
  };
  walk(parsed.documentElement);

  const style = parsed.createElement("style");
  style.textContent = `
    body { margin: 0; font-family: -apple-system, "PingFang SC", "Microsoft YaHei", sans-serif;
           font-size: 13.5px; line-height: 1.65; color: #2A2E36; word-break: break-word; }
    img { max-width: 100%; height: auto; }
    img[data-blocked] { min-width: 36px; min-height: 20px; background: #F1EDE3; border: 1px dashed #C7C1B2; }
    a { color: #1E6B49; }
    blockquote { border-left: 3px solid #E8E3D8; margin: 8px 0; padding: 2px 12px; color: #6E6A5F; }
    pre { white-space: pre-wrap; }
  `;
  parsed.head.insertBefore(style, parsed.head.firstChild);
  return { doc: "<!doctype html>" + parsed.documentElement.outerHTML, blocked };
}

interface Props {
  html: string;
}

function zoomDeltaForKey(e: KeyboardEvent) {
  const meta = e.metaKey || e.ctrlKey;
  if (!meta || e.altKey) return null;
  if (e.key === "+" || e.key === "=" || e.code === "Equal" || e.code === "NumpadAdd") return 0.1;
  if (e.key === "-" || e.key === "_" || e.code === "Minus" || e.code === "NumpadSubtract") return -0.1;
  if (e.key === "0" || e.code === "Digit0" || e.code === "Numpad0") return 0;
  return null;
}

/** 沙箱 iframe 渲染（无脚本执行；同源仅用于父页面接管链接点击和自适应高度） */
export function HtmlBody(p: Props) {
  const [allowRemote, setAllowRemote] = useState(false);
  const ref = useRef<HTMLIFrameElement>(null);

  useEffect(() => setAllowRemote(false), [p.html]);

  const { doc, blocked } = useMemo(() => sanitizeEmailHtml(p.html, allowRemote), [p.html, allowRemote]);

  function onLoad() {
    const frame = ref.current;
    const d = frame?.contentDocument;
    if (!frame || !d) return;
    const max = Math.max(360, window.innerHeight - frame.getBoundingClientRect().top - 34);
    frame.style.height = `${Math.min(d.documentElement.scrollHeight + 12, max)}px`;
    // 链接一律交给系统浏览器打开
    d.addEventListener(
      "click",
      (ev) => {
        const a = (ev.target as HTMLElement | null)?.closest?.("a");
        if (!a) return;
        ev.preventDefault();
        const href = a.getAttribute("href");
        if (href && /^https?:/i.test(href)) void openUrl(href);
      },
      true
    );
    d.addEventListener(
      "keydown",
      (ev) => {
        const delta = zoomDeltaForKey(ev);
        if (delta === null) return;
        ev.preventDefault();
        window.dispatchEvent(new CustomEvent("sealmail-zoom-delta", { detail: delta }));
      },
      true
    );
  }

  return (
    <div className="html-body-wrap">
      {blocked > 0 && !allowRemote && (
        <div className="img-blocked-bar">
          已阻止 {blocked} 处远程内容（远程图片可被用来追踪你是否打开了邮件）
          <button className="btn-ghost" style={{ height: 24, padding: "0 10px", fontSize: 11 }} onClick={() => setAllowRemote(true)}>
            显示图片
          </button>
        </div>
      )}
      <iframe ref={ref} className="html-body" sandbox="allow-same-origin" srcDoc={doc} onLoad={onLoad} title="邮件正文" />
    </div>
  );
}
