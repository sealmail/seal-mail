import { normalizeExternalUrl, openExternalUrl } from "../url";

const URL_RE = /((?:https?:\/\/|www\.)[^\s<>"']+)/gi;
const TRAILING_PUNCT = /[),.;:!?，。；：！？）]+$/;

function splitTrailingPunctuation(url: string): [string, string] {
  const match = url.match(TRAILING_PUNCT);
  if (!match) return [url, ""];
  return [url.slice(0, -match[0].length), match[0]];
}

export function TextBody({ text }: { text: string }) {
  if (!text) return <div className="msg-body">(无正文)</div>;

  const parts: React.ReactNode[] = [];
  let last = 0;
  for (const match of text.matchAll(URL_RE)) {
    const raw = match[0];
    const index = match.index ?? 0;
    if (index > last) parts.push(text.slice(last, index));

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
    if (trailing) parts.push(trailing);
    last = index + raw.length;
  }
  if (last < text.length) parts.push(text.slice(last));

  return <div className="msg-body">{parts}</div>;
}
