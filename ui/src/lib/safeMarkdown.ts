import { Marked, type Tokens } from 'marked'

/**
 * Markdown to HTML for `dangerouslySetInnerHTML`, without letting the text carry HTML.
 *
 * - Raw HTML in the Markdown (`<script>`, `<img onerror>`, `<meta http-equiv>`, `<form>`,
 *   `<style>`) is shown as text instead of becoming elements. This also keeps
 *   placeholders such as "The drive for <name> is disconnected" visible.
 * - Link and image URLs keep only http, https and mailto schemes, plus relative and
 *   `#anchor` links. Anything else (`javascript:`, `data:`, `file:`, `vbscript:`) becomes `#`.
 *
 * Markdown formatting (headings, lists, tables, code, emphasis, links) is unchanged.
 */

const ALLOWED_SCHEMES = new Set(['http:', 'https:', 'mailto:'])

export function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

/** The URL when it is safe to put in `href` / `src`, else `#`. */
export function safeUrl(raw: string | null | undefined): string {
  if (!raw) return '#'
  // Browsers ignore control characters and whitespace inside a scheme (`java\nscript:`).
  // eslint-disable-next-line no-control-regex
  const compact = raw.replace(/[\u0000- \u007f]/g, '')
  const scheme = /^([a-z][a-z0-9+.-]*):/i.exec(compact)
  if (!scheme) return raw.trim()
  return ALLOWED_SCHEMES.has(`${scheme[1].toLowerCase()}:`) ? raw.trim() : '#'
}

const safe = new Marked({
  gfm: true,
  breaks: true,
  renderer: {
    html(token: Tokens.HTML | Tokens.Tag) {
      return escapeHtml(token.text)
    },
  },
  walkTokens(token) {
    if (token.type === 'link' || token.type === 'image') {
      token.href = safeUrl(token.href)
    }
  },
})

/** Block Markdown (paragraphs, lists, tables, code blocks). */
export function renderMarkdown(text: string): string {
  return safe.parse(text, { async: false })
}

/** One line of inline Markdown (bold, code, links), no surrounding `<p>`. */
export function renderMarkdownInline(text: string): string {
  return safe.parseInline(text, { async: false })
}
