import { normalizeExternalUrl, openExternalUrl } from "../url";

const URL_RE = /((?:https?:\/\/|www\.)[^\s<>"']+)/gi;
const TRAILING_PUNCT = /[),.;:!?，。；：！？）]+$/;
const NEWLINE_RE = /\r\n|\r|\n/g;

function splitTrailingPunctuation(url: string): [string, string] {
  const match = url.match(TRAILING_PUNCT);
  if (!match) return [url, ""];
  return [url.slice(0, -match[0].length), match[0]];
}

export function TextBody({ text }: { text: string }) {
  if (!text) return <div className="msg-body">(无正文)</div>;

  const parts: React.ReactNode[] = [];
  let key = 0;
  let last = 0;
  const pushText = (value: string) => {
    let start = 0;
    for (const newline of value.matchAll(NEWLINE_RE)) {
      const index = newline.index ?? 0;
      if (index > start) parts.push(value.slice(start, index));
      parts.push(<br key={`br-${key++}`} />);
      start = index + newline[0].length;
    }
    if (start < value.length) parts.push(value.slice(start));
  };

  for (const match of text.matchAll(URL_RE)) {
    const raw = match[0];
    const index = match.index ?? 0;
    if (index > last) pushText(text.slice(last, index));

    const [url, trailing] = splitTrailingPunctuation(raw);
    const href = normalizeExternalUrl(url) ?? url;
    parts.push(
      <a
        key={`${index}-${url}`}
        href={href}
        onClick={(e) => {
          e.preventDefault();
          void openExternalUrl(url, { label: url });
        }}
      >
        {url}
      </a>
    );
    if (trailing) pushText(trailing);
    last = index + raw.length;
  }
  if (last < text.length) pushText(text.slice(last));

  return (
    <pre
      className="msg-body"
      onCopy={(e) => {
        const selected = window.getSelection()?.toString();
        e.clipboardData.setData("text/plain", selected || text);
        e.preventDefault();
      }}
    >
      {parts}
    </pre>
  );
}
