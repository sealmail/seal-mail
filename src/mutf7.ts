/**
 * IMAP modified UTF-7 (RFC 3501 §5.1.3) 解码。
 * 服务器目录名里的中文等非 ASCII 以 `&<改良 base64>-` 传输；
 * 显示时解码，与服务器交互仍用原始名。
 */
export function decodeMutf7(input: string): string {
  let out = "";
  let i = 0;
  while (i < input.length) {
    const ch = input[i];
    if (ch !== "&") {
      out += ch;
      i += 1;
      continue;
    }
    const end = input.indexOf("-", i + 1);
    if (end < 0) {
      out += input.slice(i);
      break;
    }
    const seg = input.slice(i + 1, end);
    i = end + 1;
    if (seg.length === 0) {
      // "&-" → 字面量 '&'
      out += "&";
      continue;
    }
    const b64 = seg.replace(/,/g, "/");
    const pad = b64.length % 4 === 0 ? "" : "=".repeat(4 - (b64.length % 4));
    try {
      const binary = atob(b64 + pad);
      if (binary.length % 2 !== 0) {
        out += `&${seg}-`;
        continue;
      }
      const units: number[] = [];
      for (let j = 0; j < binary.length; j += 2) {
        units.push((binary.charCodeAt(j) << 8) | binary.charCodeAt(j + 1));
      }
      out += String.fromCharCode(...units);
    } catch {
      out += `&${seg}-`;
    }
  }
  return out;
}

/** 列表/侧栏目录显示名：优先用 FolderInfo.display，否则解码 mutf7 */
export function folderTitle(name: string, display?: string | null): string {
  const d = display?.trim();
  if (d) return d;
  return decodeMutf7(name);
}
