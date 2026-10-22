import { Check, Copy } from 'lucide-react'
import { marked } from 'marked'
import { useEffect, useId, useMemo, useState } from 'react'

interface MarkdownProps {
  content: string
  className?: string
}

// Configure marked with sensible defaults
marked.setOptions({
  gfm: true,
  breaks: true,
})

export function Markdown({ content, className = '' }: MarkdownProps) {
  const containerId = useId()
  const [copiedCodeIndex, setCopiedCodeIndex] = useState<number | null>(null)

  // Parse markdown into HTML string
  const rawHtml = useMemo(() => {
    if (!content) return ''
    try {
      return marked.parse(content) as string
    } catch {
      return content
    }
  }, [content])

  // Enhance code blocks with copy buttons after render
  useEffect(() => {
    const el = document.getElementById(containerId)
    if (!el) return

    const preElements = el.querySelectorAll('pre')
    preElements.forEach((pre, index) => {
      if (pre.parentElement?.classList.contains('code-block-wrapper')) return

      const wrapper = document.createElement('div')
      wrapper.className = 'code-block-wrapper relative group my-3 rounded-lg overflow-hidden border border-border bg-muted/60 dark:bg-black/40 shadow-xs'

      // Check code language
      const codeEl = pre.querySelector('code')
      const langClass = Array.from(codeEl?.classList || []).find((c) => c.startsWith('language-'))
      const lang = langClass ? langClass.replace('language-', '') : ''

      // Header bar
      const header = document.createElement('div')
      header.className = 'flex items-center justify-between px-3 py-1.5 bg-muted/90 dark:bg-zinc-900 border-b border-border/80 text-[11px] font-mono text-muted-foreground'
      header.innerHTML = `<span>${lang || 'code'}</span>`

      // Copy button
      const copyBtn = document.createElement('button')
      copyBtn.className = 'flex items-center gap-1 text-[11px] text-muted-foreground hover:text-foreground transition-colors px-1.5 py-0.5 rounded hover:bg-muted'
      copyBtn.innerHTML = `<span>Copy</span>`
      copyBtn.onclick = () => {
        const textToCopy = codeEl?.innerText || pre.innerText
        void navigator.clipboard.writeText(textToCopy)
        setCopiedCodeIndex(index)
        copyBtn.innerHTML = `<span class="text-teal-500 font-medium">Copied!</span>`
        setTimeout(() => {
          copyBtn.innerHTML = `<span>Copy</span>`
          setCopiedCodeIndex(null)
        }, 2000)
      }
      header.appendChild(copyBtn)

      pre.parentNode?.insertBefore(wrapper, pre)
      wrapper.appendChild(header)
      wrapper.appendChild(pre)

      pre.className = 'p-3.5 overflow-x-auto text-xs font-mono leading-relaxed text-foreground/90 selection:bg-teal-500/20'
    })
  }, [rawHtml, containerId, copiedCodeIndex])

  return (
    <div
      id={containerId}
      className={`prose-ols max-w-none text-foreground ${className}`}
      dangerouslySetInnerHTML={{ __html: rawHtml }}
    />
  )
}

/** Standalone code block with syntax header and copy button */
export function CodeBlock({ code, language = 'bash' }: { code: string; language?: string }) {
  const [copied, setCopied] = useState(false)

  const handleCopy = () => {
    void navigator.clipboard.writeText(code)
    setCopied(true)
    setTimeout(() => setCopied(false), 2000)
  }

  return (
    <div className="relative my-3 rounded-lg border border-border bg-muted/60 dark:bg-black/40 overflow-hidden shadow-xs">
      <div className="flex items-center justify-between px-3.5 py-1.5 bg-muted/80 dark:bg-zinc-900/90 border-b border-border/80 text-[11px] font-mono text-muted-foreground">
        <span>{language}</span>
        <button
          onClick={handleCopy}
          className="flex items-center gap-1 hover:text-foreground transition-colors"
        >
          {copied ? <Check className="size-3 text-teal-500" /> : <Copy className="size-3" />}
          <span>{copied ? 'Copied' : 'Copy'}</span>
        </button>
      </div>
      <pre className="p-3.5 overflow-x-auto text-xs font-mono leading-relaxed text-foreground/90">
        <code>{code}</code>
      </pre>
    </div>
  )
}
