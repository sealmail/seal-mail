import { useEffect, useMemo, useRef, useState } from "react";
import { openExternalUrl } from "../url";

/** 整体移除的危险标签（脚本、外部加载、表单提交、可联网样式/SVG） */
const REMOVE_TAGS = "script,iframe,object,embed,form,base,meta,link,style,svg,math,applet,frame,frameset,audio,video";

const RESOURCE_ATTRS = new Set(["src", "srcset", "poster", "background", "data"]);

function isRemoteUrl(value: string) {
  return /^(https?:)?\/\//i.test(value.trim());
}

function isSvgDataUrl(value: string) {
  return /^data:image\/svg\+xml/i.test(value.trim());
}

/**
 * 邮件 HTML 消毒：
 * - 去掉脚本/iframe/表单等危险标签与全部 on* 事件属性
 * - 去掉 javascript:/vbscript:/data:text/html 协议链接
 * - allowRemote=false 时阻断 http(s) 资源属性与 style 里的远程 url()（防追踪像素）
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
      } else if (!allowRemote && RESOURCE_ATTRS.has(n) && isRemoteUrl(attr.value)) {
        blocked++;
        el.removeAttribute(attr.name);
        el.setAttribute("data-blocked", "1");
      } else if (!allowRemote && (n === "href" || n === "xlink:href") && el.tagName !== "A" && isRemoteUrl(attr.value)) {
        blocked++;
        el.removeAttribute(attr.name);
      } else if (n === "style" && !allowRemote && /url\s*\(/i.test(attr.value)) {
        blocked++;
        el.setAttribute("style", attr.value.replace(/url\s*\([^)]*\)/gi, "none"));
      } else if (n === "srcset" && !allowRemote) {
        el.removeAttribute(attr.name);
      } else if ((n === "src" || n === "href" || n === "xlink:href") && isSvgDataUrl(attr.value)) {
        el.removeAttribute(attr.name);
      } else if (n === "bgcolor" || n === "background") {
        el.removeAttribute(attr.name);
      }
    }
    if (el.tagName === "IMG") {
      const src = el.getAttribute("src") ?? "";
      if (isRemoteUrl(src)) {
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
    html { width: 100% !important; min-width: 0 !important; background: transparent !important; overflow-x: hidden; }
    *, *::before, *::after { box-sizing: border-box; }
    body { margin: 0; font-family: -apple-system, "PingFang SC", "Microsoft YaHei", sans-serif;
           font-size: 13.5px; line-height: 1.65; color: #2A2E36; word-break: break-word;
           width: 100% !important; min-width: 0 !important; background: transparent !important; overflow-x: hidden; }
    body > :first-child { margin-top: 0 !important; }
    body > :last-child { margin-bottom: 0 !important; }
    body, center, table, tbody, thead, tfoot, tr, td, th, div, section, article, main {
      background-color: transparent !important;
      background-image: none !important;
    }
    [width] { max-width: 100% !important; }
    table { max-width: 100% !important; }
    td, th, div, p, section, article { max-width: 100%; }
    img { max-width: 100% !important; height: auto; }
    img[data-blocked] {
      display: none !important; width: 0 !important; height: 0 !important;
      min-width: 0 !important; min-height: 0 !important; margin: 0 !important;
      border: 0 !important; padding: 0 !important;
    }
    a { color: #1E6B49; }
    blockquote { border-left: 3px solid #E8E3D8; margin: 8px 0; padding: 2px 12px; color: #6E6A5F; }
    pre { white-space: pre-wrap; }
  `;
  parsed.head.appendChild(style);
  return { doc: "<!doctype html>" + parsed.documentElement.outerHTML, blocked };
}

interface Props {
  html: string;
}

type ZoomShortcut = { kind: "step"; delta: number } | { kind: "reset" };

function zoomShortcutForKey(e: KeyboardEvent): ZoomShortcut | null {
  const meta = e.metaKey || e.ctrlKey;
  if (!meta || e.altKey) return null;
  if (e.key === "+" || e.key === "=" || e.code === "Equal" || e.code === "NumpadAdd") return { kind: "step", delta: 0.1 };
  if (e.key === "-" || e.key === "_" || e.code === "Minus" || e.code === "NumpadSubtract") return { kind: "step", delta: -0.1 };
  if (e.key === "0" || e.code === "Digit0" || e.code === "Numpad0") return { kind: "reset" };
  return null;
}

/** 沙箱 iframe 渲染（无脚本执行；同源仅用于父页面接管链接点击和自适应高度） */
export function HtmlBody(p: Props) {
  const [allowRemote, setAllowRemote] = useState(false);
  const ref = useRef<HTMLIFrameElement>(null);
  const cleanupRef = useRef<(() => void) | null>(null);

  useEffect(() => setAllowRemote(false), [p.html]);

  const { doc, blocked } = useMemo(() => sanitizeEmailHtml(p.html, allowRemote), [p.html, allowRemote]);
  const frameKey = useMemo(() => `${allowRemote ? "remote" : "safe"}-${doc.length}-${p.html.length}`, [allowRemote, doc, p.html]);

  function onLoad() {
    cleanupRef.current?.();
    cleanupRef.current = null;

    const frame = ref.current;
    const d = frame?.contentDocument;
    if (!frame || !d) return;

    let raf = 0;
    const measure = () => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => {
        frame.style.width = "100%";
        const nextHeight = Math.max(
          120,
          d.documentElement.scrollHeight,
          d.body?.scrollHeight ?? 0,
          d.documentElement.offsetHeight,
          d.body?.offsetHeight ?? 0
        );
        frame.style.height = `${nextHeight + 12}px`;
      });
    };

    measure();
    window.addEventListener("resize", measure);
    window.addEventListener("sealmail-zoom-change", measure);
    d.fonts?.ready.then(measure).catch(() => undefined);
    d.querySelectorAll("img").forEach((img) => {
      img.addEventListener("load", measure);
      img.addEventListener("error", measure);
    });
    const observer = new ResizeObserver(measure);
    observer.observe(d.documentElement);
    if (d.body) observer.observe(d.body);

    // 链接一律交给系统浏览器打开
    const onClick = (ev: MouseEvent) => {
      const a = (ev.target as HTMLElement | null)?.closest?.("a");
      if (!a) return;
      ev.preventDefault();
      void openExternalUrl(a.getAttribute("href"), { label: a.textContent });
    };
    const onKeydown = (ev: KeyboardEvent) => {
      const shortcut = zoomShortcutForKey(ev);
      if (!shortcut) return;
      ev.preventDefault();
      window.dispatchEvent(new CustomEvent("sealmail-zoom-delta", { detail: shortcut }));
      window.parent?.dispatchEvent(new CustomEvent("sealmail-zoom-delta", { detail: shortcut }));
    };
    d.addEventListener("click", onClick, true);
    d.addEventListener("keydown", onKeydown, true);

    cleanupRef.current = () => {
      cancelAnimationFrame(raf);
      observer.disconnect();
      window.removeEventListener("resize", measure);
      window.removeEventListener("sealmail-zoom-change", measure);
      d.querySelectorAll("img").forEach((img) => {
        img.removeEventListener("load", measure);
        img.removeEventListener("error", measure);
      });
      d.removeEventListener("click", onClick, true);
      d.removeEventListener("keydown", onKeydown, true);
    };
  }

  useEffect(() => () => cleanupRef.current?.(), []);

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
      <iframe key={frameKey} ref={ref} className="html-body" sandbox="allow-same-origin" srcDoc={doc} onLoad={onLoad} title="邮件正文" />
    </div>
  );
}
