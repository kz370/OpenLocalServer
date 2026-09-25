import { defaultKeymap, history, historyKeymap, indentWithTab } from '@codemirror/commands'
import { StreamLanguage, syntaxHighlighting, defaultHighlightStyle, bracketMatching } from '@codemirror/language'
import { nginx } from '@codemirror/legacy-modes/mode/nginx'
import { properties } from '@codemirror/legacy-modes/mode/properties'
import { yaml } from '@codemirror/lang-yaml'
import { MergeView } from '@codemirror/merge'
import { highlightSelectionMatches, search, searchKeymap } from '@codemirror/search'
import { EditorState, type Extension } from '@codemirror/state'
import { oneDark } from '@codemirror/theme-one-dark'
import { EditorView, highlightActiveLine, keymap, lineNumbers } from '@codemirror/view'
import { useEffect, useRef } from 'react'

import { useTheme } from '@/lib/theme'

export type EditorLanguage = 'nginx' | 'apache' | 'yaml' | 'text'

function languageExtension(lang: EditorLanguage): Extension {
  switch (lang) {
    case 'nginx':
    case 'apache':
    case 'text':
      // Apache and Caddy configs read fine with the nginx/properties-style tokenizer:
      // directives, strings, comments. Nginx gets its own.
      return StreamLanguage.define(lang === 'nginx' ? nginx : properties)
    case 'yaml':
      return yaml()
  }
}

function baseExtensions(lang: EditorLanguage, dark: boolean, readOnly: boolean): Extension[] {
  return [
    lineNumbers(),
    history(),
    bracketMatching(),
    highlightActiveLine(),
    highlightSelectionMatches(),
    // §25: find / replace, with the usual Ctrl+F / Ctrl+H keys.
    search({ top: true }),
    keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap, indentWithTab]),
    languageExtension(lang),
    syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
    ...(dark ? [oneDark] : []),
    EditorState.readOnly.of(readOnly),
    EditorView.editable.of(!readOnly),
    EditorView.theme({
      '&': { height: '100%', fontSize: '13px' },
      '.cm-scroller': { fontFamily: 'ui-monospace, SFMono-Regular, Consolas, monospace' },
    }),
  ]
}

/** CodeMirror 6 editor (§25): syntax highlighting, search/replace, read-only mode. */
export function CodeEditor({
  value,
  onChange,
  language = 'text',
  readOnly = false,
  height = '420px',
}: {
  value: string
  onChange?: (v: string) => void
  language?: EditorLanguage
  readOnly?: boolean
  height?: string
}) {
  const host = useRef<HTMLDivElement>(null)
  const view = useRef<EditorView | null>(null)
  const onChangeRef = useRef(onChange)
  onChangeRef.current = onChange
  const { resolvedTheme } = useTheme()
  const dark = resolvedTheme === 'dark'

  // Rebuild only when the "shape" changes; content updates below are applied in place so
  // the cursor and undo history survive typing.
  useEffect(() => {
    if (!host.current) return
    const state = EditorState.create({
      doc: value,
      extensions: [
        ...baseExtensions(language, dark, readOnly),
        EditorView.updateListener.of((u) => {
          if (u.docChanged) onChangeRef.current?.(u.state.doc.toString())
        }),
      ],
    })
    const v = new EditorView({ state, parent: host.current })
    view.current = v
    return () => v.destroy()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [language, dark, readOnly])

  useEffect(() => {
    const v = view.current
    if (v && v.state.doc.toString() !== value) {
      v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: value } })
    }
  }, [value])

  return <div ref={host} style={{ height }} className="overflow-hidden rounded-lg border border-border" />
}

/** Side-by-side diff of two versions (§25 diff, §29 history compare). */
export function DiffView({
  original,
  modified,
  language = 'text',
  height = '420px',
}: {
  original: string
  modified: string
  language?: EditorLanguage
  height?: string
}) {
  const host = useRef<HTMLDivElement>(null)
  const { resolvedTheme } = useTheme()
  const dark = resolvedTheme === 'dark'

  useEffect(() => {
    if (!host.current) return
    const mv = new MergeView({
      parent: host.current,
      a: { doc: original, extensions: baseExtensions(language, dark, true) },
      b: { doc: modified, extensions: baseExtensions(language, dark, true) },
      highlightChanges: true,
      gutter: true,
    })
    return () => mv.destroy()
  }, [original, modified, language, dark])

  return <div ref={host} style={{ height }} className="overflow-auto rounded-lg border border-border" />
}
