import assert from 'node:assert'
import { test } from 'node:test'

import { renderMarkdown, renderMarkdownInline, safeUrl } from '../safeMarkdown.ts'

test('raw HTML is shown as text, never as elements', () => {
  for (const attack of [
    '<script>alert(1)</script>',
    '<img src=x onerror="alert(1)">',
    '<meta http-equiv="refresh" content="0;url=https://evil.example">',
    '<form action="https://evil.example"><input name=p></form>',
    '<style>body{display:none}</style>',
    '<iframe src="https://evil.example"></iframe>',
  ]) {
    const html = renderMarkdown(`Intro\n\n${attack}\n\nmore`)
    assert.ok(!/<(script|img|meta|form|input|style|iframe)\b/i.test(html), `${attack} -> ${html}`)
    assert.ok(html.includes('&lt;'), `escaped: ${html}`)
  }
  const inline = renderMarkdownInline('hi <img src=x onerror=alert(1)> there')
  assert.ok(!/<img/i.test(inline), inline)
})

test('placeholders in angle brackets stay visible', () => {
  const html = renderMarkdownInline('*"The drive for <name> is disconnected"*')
  assert.ok(html.includes('&lt;name&gt;'), html)
})

test('dangerous link schemes are neutralised', () => {
  for (const href of [
    'javascript:alert(1)',
    'JaVaScRiPt:alert(1)',
    'java\nscript:alert(1)',
    ' javascript:alert(1)',
    'data:text/html,<script>alert(1)</script>',
    'vbscript:msgbox(1)',
    'file:///C:/Windows/win.ini',
  ]) {
    assert.strictEqual(safeUrl(href), '#', href)
  }
  const html = renderMarkdown('[click](javascript:alert(1)) ![x](data:image/svg+xml,abc)')
  assert.ok(!/javascript:|data:/i.test(html), html)
})

test('ordinary Markdown is unchanged', () => {
  assert.strictEqual(safeUrl('https://openlocalserver.com/docs'), 'https://openlocalserver.com/docs')
  assert.strictEqual(safeUrl('mailto:hi@example.com'), 'mailto:hi@example.com')
  assert.strictEqual(safeUrl('#install'), '#install')
  assert.strictEqual(safeUrl('./docs/MCP.md'), './docs/MCP.md')
  const html = renderMarkdown(
    '# Title\n\n- **bold** and `code <b>`\n- [site](https://example.com)\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n```js\nconst x = "<div>"\n```',
  )
  assert.ok(html.includes('<h1'), html)
  assert.ok(html.includes('<strong>bold</strong>'), html)
  assert.ok(html.includes('<code>code &lt;b&gt;</code>'), html)
  assert.ok(html.includes('<a href="https://example.com">site</a>'), html)
  assert.ok(html.includes('<table>'), html)
  assert.ok(html.includes('&lt;div&gt;'), html)
})
