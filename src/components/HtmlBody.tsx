import { useEffect, useMemo, useRef, useState } from "react";
import { openExternalUrl } from "../url";

/** 整体移除的危险标签（脚本、外部加载、表单提交、SVG） */
const REMOVE_TAGS = "script,iframe,object,embed,form,base,meta,link,svg,math,applet,frame,frameset,audio,video";

const RESOURCE_ATTRS = new Set(["src", "srcset", "poster", "background", "data"]);

function isRemoteUrl(value: string) {
  return /^(https?:)?\/\//i.test(value.trim());
}

function isSvgDataUrl(value: string) {
  return /^data:image\/svg\+xml/i.test(value.trim());
}

function sanitizeCss(css: string, allowRemote: boolean) {
  let next = css.replace(/@import[^;]+;?/gi, "");
  next = next.replace(/-moz-binding\s*:[^;]+;?/gi, "");
  if (!allowRemote) next = next.replace(/url\s*\([^)]*\)/gi, "none");
  return next;
}

/**
 * 邮件 HTML 消毒：
 * - 去掉脚本/iframe/表单等危险标签与全部 on* 事件属性
 * - 去掉 javascript:/vbscript:/data:text/html 协议链接
 * - 保留邮件自身 CSS/背景色，allowRemote=false 时阻断 CSS url() 和 http(s) 资源属性（防追踪像素）
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
      } else if (n === "style") {
        const sanitized = sanitizeCss(attr.value, allowRemote);
        if (sanitized !== attr.value) blocked++;
        el.setAttribute("style", sanitized);
      } else if (n === "srcset" && !allowRemote) {
        el.removeAttribute(attr.name);
      } else if ((n === "src" || n === "href" || n === "xlink:href") && isSvgDataUrl(attr.value)) {
        el.removeAttribute(attr.name);
      }
    }
    if (el.tagName === "STYLE") {
      const before = el.textContent ?? "";
      const after = sanitizeCss(before, allowRemote);
      if (after !== before) blocked++;
      el.textContent = after;
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
    /* overflow-y hidden：iframe 高度由父页面按内容精确设置，内部永远不该出现
       纵向滚动条（否则缩放时会闪烁）；超宽邮件仍允许横向滚动 */
    html { width: 100% !important; min-width: 0 !important; overflow-x: auto; overflow-y: hidden; }
    *, *::before, *::after { box-sizing: border-box; }
    body { margin: 0; width: 100% !important; min-width: 0 !important; overflow-x: auto; }
    body > :first-child { margin-top: 0 !important; }
    body > :last-child { margin-bottom: 0 !important; }
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

function currentZoom() {
  const z = parseFloat(localStorage.getItem("sealmail.zoom") ?? "1");
  return Number.isFinite(z) && z > 0 ? z : 1;
}

/* iframe 内容不继承父文档的 zoom，所以要在 iframe 里重放一次。
   标准化 CSS zoom 下百分比宽度自动换算，不要再加 width: calc(100%/zoom) 补偿。 */
function applyFrameZoom(d: Document, zoom: number) {
  if (!d.body) return;
  (d.body.style as CSSStyleDeclaration & { zoom: string }).zoom = String(zoom);
}

function zoomShortcutForKey(e: KeyboardEvent): ZoomShortcut | null {
  const meta = e.metaKey || e.ctrlKey;
  if (!meta || e.altKey) return null;
  if (e.key === "+" || e.key === "=" || e.code === "Equal" || e.code === "NumpadAdd") return { kind: "step", delta: 0.1 };
  if (e.key === "-" || e.key === "_" || e.code === "Minus" || e.code === "NumpadSubtract") return { kind: "step", delta: -0.1 };
  if (e.key === "0" || e.code === "Digit0" || e.code === "Numpad0") return { kind: "reset" };
  return null;
}

function closestAnchor(target: EventTarget | null): HTMLAnchorElement | null {
  if (!(target instanceof Node)) return null;
  const el = target instanceof Element ? target : target.parentElement;
  return el?.closest?.("a") ?? null;
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
        // 高度只用 iframe 内 body 自身坐标系（不含 zoom）的值：iframe 元素的样式
        // 高度被外层 zoom 放大一次，iframe 内容被内层 zoom 放大一次，二者相同正好
        // 抵消，任意缩放比例都精确贴合。documentElement.scrollHeight 是含 zoom 的
        // 渲染像素，混进来会在缩放时算错高度、出现内部滚动条（实测探针结论）。
        const contentCss = Math.max(
          120,
          d.body?.scrollHeight ?? 0,
          d.body?.offsetHeight ?? 0
        );
        frame.style.height = `${contentCss + 12}px`;
      });
    };

    applyFrameZoom(d, currentZoom());
    measure();
    window.addEventListener("resize", measure);
    const onZoomChange = (ev: Event) => {
      const zoom = (ev as CustomEvent<number>).detail;
      applyFrameZoom(d, Number.isFinite(zoom) && zoom > 0 ? zoom : currentZoom());
      measure();
    };
    window.addEventListener("sealmail-zoom-change", onZoomChange);
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
      const a = closestAnchor(ev.target);
      if (!a) return;
      ev.preventDefault();
      ev.stopPropagation();
      void openExternalUrl(a.getAttribute("href"), { label: a.textContent });
    };
    const onAuxClick = (ev: MouseEvent) => {
      const a = closestAnchor(ev.target);
      if (!a) return;
      ev.preventDefault();
      ev.stopPropagation();
      void openExternalUrl(a.getAttribute("href"), { label: a.textContent });
    };
    const onKeydown = (ev: KeyboardEvent) => {
      if (ev.key === "Enter") {
        const a = closestAnchor(ev.target);
        if (a) {
          ev.preventDefault();
          ev.stopPropagation();
          void openExternalUrl(a.getAttribute("href"), { label: a.textContent });
          return;
        }
      }
      const shortcut = zoomShortcutForKey(ev);
      if (!shortcut) return;
      ev.preventDefault();
      window.dispatchEvent(new CustomEvent("sealmail-zoom-delta", { detail: shortcut }));
    };
    d.addEventListener("click", onClick, true);
    d.addEventListener("auxclick", onAuxClick, true);
    d.addEventListener("keydown", onKeydown, true);

    cleanupRef.current = () => {
      cancelAnimationFrame(raf);
      observer.disconnect();
      window.removeEventListener("resize", measure);
      window.removeEventListener("sealmail-zoom-change", onZoomChange);
      d.querySelectorAll("img").forEach((img) => {
        img.removeEventListener("load", measure);
        img.removeEventListener("error", measure);
      });
      d.removeEventListener("click", onClick, true);
      d.removeEventListener("auxclick", onAuxClick, true);
      d.removeEventListener("keydown", onKeydown, true);
    };
  }

  useEffect(() => () => cleanupRef.current?.(), []);

  return (
    <div className="html-body-wrap">
      {blocked > 0 && !allowRemote && (
        <div
          className="img-blocked-chip"
          title={`已阻止 ${blocked} 处远程内容：远程图片可被用来追踪你是否打开了邮件，确认来源可信后再显示`}
        >
          已阻止远程图片
          <button onClick={() => setAllowRemote(true)}>显示</button>
        </div>
      )}
      <iframe key={frameKey} ref={ref} className="html-body" sandbox="allow-same-origin" srcDoc={doc} onLoad={onLoad} title="邮件正文" />
    </div>
  );
}
