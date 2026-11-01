// Markdown files in the file preview, shown formatted. The file comes from a
// server and the app sits next to native commands, so nothing in it is
// trusted: raw HTML is dropped, links open in the system browser (or not at
// all), and images aren't fetched, so opening a README doesn't phone home.

import { ImageOff } from "lucide-react";
import { memo, useLayoutEffect, useRef, useState, type ComponentProps } from "react";
import Markdown, { type Components, type ExtraProps, type Options } from "react-markdown";
import remarkGfm from "remark-gfm";
import { backend } from "../lib/backend";
import { cn } from "../lib/format";

/** Links that may leave the app, for the system browser. Anything else stays put. */
const EXTERNAL = /^(?:https?:|mailto:)/i;
/** Tables, task lists, strikethrough and bare links, as on GitHub. */
const PLUGINS = [remarkGfm];

type Block = { type: string; position?: { start: { line: number }; end: { line: number } }; properties?: Record<string, unknown> };

/** Marks each top-level block with the lines of the file it comes from, so a range of lines can be found on screen. */
function sourceLines() {
  return (tree: { children: Block[] }) => {
    for (const n of tree.children) {
      if (n.type === "element" && n.position) n.properties = { ...n.properties, dataLs: n.position.start.line, dataLe: n.position.end.line };
    }
  };
}
const REHYPE = [sourceLines] as Options["rehypePlugins"];

function Link({ href, title, children }: ComponentProps<"a"> & ExtraProps) {
  const external = !!href && EXTERNAL.test(href);
  return (
    <a
      href={href}
      title={title ? `${title}\n${href ?? ""}` : href}
      // Never let the webview itself follow a link.
      onClick={(e) => {
        e.preventDefault();
        if (external) void backend.openExternal(href).catch(() => {});
      }}
      onAuxClick={(e) => e.preventDefault()}
      onDragStart={(e) => e.preventDefault()}
      className={cn("underline-offset-2", external ? "text-accent hover:underline" : "text-fg underline decoration-ink/[0.3]")}
    >
      {children}
    </a>
  );
}

/** An image is its alt text in a quiet chip; where it would load from shows on hover. */
function Picture({ src, alt }: ComponentProps<"img"> & ExtraProps) {
  const from = typeof src === "string" ? src : "";
  return (
    <span
      title={from ? `Image not loaded: ${from}` : "Image not loaded"}
      className="inline-flex max-w-full items-center gap-1 rounded-[5px] bg-ink/[0.05] px-1.5 align-middle text-[11.5px] leading-[20px] text-fg-3 ring-1 ring-inset ring-ink/[0.07]"
    >
      <ImageOff size={12} className="shrink-0" />
      <span className="truncate">{alt || "Image"}</span>
    </span>
  );
}

const components: Components = {
  h1: ({ node: _, ...p }) => <h1 {...p} className="mt-7 mb-3 border-b border-line pb-1.5 text-[20px] leading-snug font-semibold" />,
  // Footnotes come with an h2 that only screen readers get (`sr-only`).
  h2: ({ node: _, className, ...p }) => (
    <h2 {...p} className={cn("mt-6 mb-2.5 border-b border-line pb-1 text-[16.5px] leading-snug font-semibold", className)} />
  ),
  h3: ({ node: _, ...p }) => <h3 {...p} className="mt-5 mb-2 text-[14.5px] leading-snug font-semibold" />,
  h4: ({ node: _, ...p }) => <h4 {...p} className="mt-4 mb-1.5 text-[13.5px] font-semibold" />,
  h5: ({ node: _, ...p }) => <h5 {...p} className="mt-4 mb-1.5 text-[13px] font-semibold text-fg-2" />,
  h6: ({ node: _, ...p }) => <h6 {...p} className="mt-4 mb-1.5 text-[12.5px] font-semibold text-fg-3" />,
  p: ({ node: _, ...p }) => <p {...p} className="my-3" />,
  a: Link,
  img: Picture,
  strong: ({ node: _, ...p }) => <strong {...p} className="font-semibold" />,
  blockquote: ({ node: _, ...p }) => <blockquote {...p} className="my-3 border-l-[3px] border-ink/[0.14] pl-3.5 text-fg-2 [&>p]:my-2" />,
  hr: ({ node: _, ...p }) => <hr {...p} className="my-6 border-0 border-t border-line-strong" />,
  // A list in a list sits closer than one between paragraphs.
  ul: ({ node: _, className, ...p }) => (
    <ul {...p} className={cn("my-3 list-disc pl-5 [&_ol]:my-1 [&_ul]:my-1", className)} />
  ),
  ol: ({ node: _, ...p }) => <ol {...p} className="my-3 list-decimal pl-5 [&_ol]:my-1 [&_ul]:my-1" />,
  // A task item draws its checkbox where the bullet would be; plain items in the same list keep theirs.
  li: ({ node: _, className, ...p }) => (
    <li {...p} className={cn("my-1 pl-0.5 marker:text-fg-3 [&>p]:my-1", className?.includes("task-list-item") && "-ml-5 list-none")} />
  ),
  // Task list boxes show what the file says; they don't change it.
  input: ({ node: _, ...p }) => <input {...p} readOnly className="mr-1.5 size-3.5 -translate-y-px align-middle accent-[var(--ui-accent)]" />,
  // Inline code; inside a block, the block's own look takes over.
  code: ({ node: _, className, ...p }) => (
    <code {...p} className={cn("rounded-[4px] bg-ink/[0.07] px-[5px] py-px font-mono text-[0.88em]", className)} />
  ),
  // Long lines scroll sideways rather than wrap.
  pre: ({ node: _, ...p }) => (
    <pre
      {...p}
      className="my-3 overflow-x-auto rounded-[8px] bg-ink/[0.045] px-3.5 py-3 font-mono text-[12px] leading-[1.6] ring-1 ring-inset ring-ink/[0.06] [&>code]:bg-transparent [&>code]:p-0 [&>code]:text-[1em]"
    />
  ),
  // A wide table keeps its columns and scrolls; a long cell wraps at a readable width.
  table: ({ node: _, ...p }) => (
    <div className="my-3 overflow-x-auto">
      <table {...p} className="w-max border-collapse text-[12.5px] leading-normal" />
    </div>
  ),
  th: ({ node: _, children, ...p }) => (
    <th {...p} className="border border-line-strong bg-ink/[0.04] px-2.5 py-1.5 text-left font-semibold">
      <div className="max-w-[26rem]">{children}</div>
    </th>
  ),
  td: ({ node: _, children, ...p }) => (
    <td {...p} className="border border-line-strong px-2.5 py-1.5 align-top text-fg-2">
      <div className="max-w-[26rem]">{children}</div>
    </td>
  ),
};

/**
 * The formatted file. Drawn again only when its text changes. With `lines`
 * (first and last, from a path like "notes.md:12-40") it opens at them, the
 * blocks they make up marked by one band behind them.
 */
export const MarkdownView = memo(function MarkdownView({ text, lines }: { text: string; lines?: [number, number] }) {
  const scroller = useRef<HTMLDivElement>(null);
  const article = useRef<HTMLElement>(null);
  const [band, setBand] = useState<{ top: number; height: number } | null>(null);
  const [first, last] = lines ?? [0, 0];
  useLayoutEffect(() => {
    const box = scroller.current;
    const blocks = article.current ? [...article.current.querySelectorAll<HTMLElement>("[data-ls]")] : [];
    const hits = first ? blocks.filter((el) => Number(el.dataset.le) >= first && Number(el.dataset.ls) <= last) : [];
    if (!box || !article.current || !hits.length) return setBand(null);
    const at = article.current.getBoundingClientRect().top;
    const top = hits[0].getBoundingClientRect().top - at;
    const bottom = Math.max(...hits.map((el) => el.getBoundingClientRect().bottom)) - at;
    setBand({ top: top - 6, height: bottom - top + 12 });
    box.scrollTop += hits[0].getBoundingClientRect().top - box.getBoundingClientRect().top - 28;
  }, [text, first, last]);
  return (
    <div ref={scroller} tabIndex={0} aria-label="Formatted Markdown" className="selectable min-h-0 flex-1 overflow-y-auto outline-none">
      <article ref={article} className="relative isolate mx-auto max-w-[760px] px-5 py-4 text-[13.5px] leading-[1.65] break-words text-fg [&>*:first-child]:mt-0">
        <Markdown remarkPlugins={PLUGINS} rehypePlugins={REHYPE} skipHtml components={components}>
          {text}
        </Markdown>
        {band && <div aria-hidden className="lines-band pointer-events-none absolute inset-x-2 -z-10 rounded-[6px]" style={band} />}
      </article>
    </div>
  );
});
