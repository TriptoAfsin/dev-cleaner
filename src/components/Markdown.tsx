import { Fragment, type ReactNode } from "react"
import { Lexer, type Token, type Tokens } from "marked"
import { openUrl } from "@tauri-apps/plugin-opener"
import { cn } from "@/lib/format"

/** GitHub-flavoured Markdown (headings, lists, tables, code, links) drawn as
    React elements from marked's tokens. No HTML string is ever injected, so
    raw HTML in the source shows as text. Wide tables and code scroll sideways
    instead of breaking the layout. */
export function Markdown({ source, className }: { source: string; className?: string }) {
  const tokens = Lexer.lex(source, { gfm: true })
  return <div className={cn("flex min-w-0 flex-col gap-2 text-[12px] leading-[1.55] text-muted", className)}>{blocks(tokens)}</div>
}

function blocks(tokens: Token[]): ReactNode {
  return tokens.map((t, i) => <Fragment key={i}>{block(t)}</Fragment>)
}

function block(t: Token): ReactNode {
  switch (t.type) {
    case "heading": {
      const h = t as Tokens.Heading
      return <div className={cn("font-semibold text-text", h.depth <= 2 ? "mt-1 text-[13px]" : "text-[12.5px]")}>{inline(h.tokens)}</div>
    }
    case "paragraph":
      return <p className="m-0">{inline((t as Tokens.Paragraph).tokens)}</p>
    case "text": {
      const x = t as Tokens.Text
      return <p className="m-0">{x.tokens ? inline(x.tokens) : x.text}</p>
    }
    case "list": {
      const l = t as Tokens.List
      const Tag = l.ordered ? "ol" : "ul"
      return (
        <Tag className={cn("m-0 flex flex-col gap-1 pl-4", l.ordered ? "list-decimal" : "list-disc")}>
          {l.items.map((it, i) => (
            <li key={i} className="pl-0.5">
              {it.task && <input type="checkbox" checked={!!it.checked} readOnly disabled className="mr-1.5 align-middle" />}
              {listItem(it)}
            </li>
          ))}
        </Tag>
      )
    }
    case "code":
      return (
        <pre className="mono m-0 overflow-x-auto rounded-md border border-border bg-surface-2 px-2.5 py-2 text-[11.5px] leading-[1.5] text-text">
          <code>{(t as Tokens.Code).text}</code>
        </pre>
      )
    case "blockquote":
      return <div className="border-l-2 border-border-strong pl-2.5">{blocks((t as Tokens.Blockquote).tokens)}</div>
    case "table": {
      const tb = t as Tokens.Table
      const align = (a: "center" | "left" | "right" | null) => (a === "right" ? "text-right" : a === "center" ? "text-center" : "text-left")
      return (
        <div className="overflow-x-auto rounded-md border border-border" data-testid="md-table">
          <table className="w-max min-w-full border-collapse text-[11.5px]">
            <thead className="bg-surface-2 text-text">
              <tr>
                {tb.header.map((c, i) => (
                  <th key={i} className={cn("whitespace-nowrap border-b border-border px-2.5 py-1.5 font-semibold", align(tb.align[i]))}>
                    {inline(c.tokens)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {tb.rows.map((r, ri) => (
                <tr key={ri} className="border-t border-border first:border-t-0">
                  {r.map((c, i) => (
                    <td key={i} className={cn("px-2.5 py-1.5 align-top", align(tb.align[i]))}>
                      {inline(c.tokens)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )
    }
    case "hr":
      return <hr className="m-0 border-0 border-t border-border" />
    case "space":
      return null
    default:
      return "raw" in t ? <p className="m-0 whitespace-pre-wrap">{t.raw}</p> : null
  }
}

/** A list item's tokens: loose items wrap text in paragraphs, tight ones don't. */
function listItem(it: Tokens.ListItem): ReactNode {
  return it.tokens.map((t, i) => {
    if (t.type === "text") {
      const x = t as Tokens.Text
      return <Fragment key={i}>{x.tokens ? inline(x.tokens) : x.text}</Fragment>
    }
    return <Fragment key={i}>{block(t)}</Fragment>
  })
}

function inline(tokens: Token[] | undefined): ReactNode {
  return (tokens ?? []).map((t, i) => {
    switch (t.type) {
      case "strong":
        return <strong key={i} className="font-semibold text-text">{inline((t as Tokens.Strong).tokens)}</strong>
      case "em":
        return <em key={i}>{inline((t as Tokens.Em).tokens)}</em>
      case "del":
        return <del key={i}>{inline((t as Tokens.Del).tokens)}</del>
      case "codespan":
        return (
          <code key={i} className="mono rounded bg-surface-2 px-1 py-px text-[11.5px] text-text">
            {decode((t as Tokens.Codespan).text)}
          </code>
        )
      case "br":
        return <br key={i} />
      case "link": {
        const l = t as Tokens.Link
        const safe = /^https?:\/\//i.test(l.href)
        return safe ? (
          <a
            key={i}
            href={l.href}
            onClick={(e) => {
              e.preventDefault()
              void openUrl(l.href).catch(() => {})
            }}
            className="text-accent hover:underline"
          >
            {inline(l.tokens)}
          </a>
        ) : (
          <Fragment key={i}>{inline(l.tokens)}</Fragment>
        )
      }
      case "text": {
        const x = t as Tokens.Text
        return <Fragment key={i}>{x.tokens ? inline(x.tokens) : decode(x.text)}</Fragment>
      }
      case "escape":
        return <Fragment key={i}>{(t as Tokens.Escape).text}</Fragment>
      default:
        return <Fragment key={i}>{"text" in t ? decode(String(t.text)) : t.raw}</Fragment>
    }
  })
}

/** marked escapes a few characters in text; show them as written. */
function decode(s: string) {
  return s.replace(/&(amp|lt|gt|quot|#39);/g, (_, e) => ({ amp: "&", lt: "<", gt: ">", quot: '"', "#39": "'" })[e as string] ?? _)
}
