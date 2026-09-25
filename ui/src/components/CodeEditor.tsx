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
    // Long lines wrap: nothing in the app scrolls sideways.
    EditorView.lineWrapping,
    syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
    ...(dark ? [oneDark] : []),
    EditorState.readOnly.of(readOnly),
    EditorView.editable.of(!readOnly),
    EditorView.theme({
      '&': { height: '100%', fontSize: '13px' },
      '.cm-scroller': { fontFamily: 'ui-monospace, SFMono-Regular, Consolas, monospace' },
    }),
    appSurface,
  ]
}

/** Sit on the app's own card colour instead of One Dark's grey, in both themes. */
const appSurface = EditorView.theme({
  '&': { backgroundColor: 'var(--card)' },
  '.cm-gutters': { backgroundColor: 'var(--card)', borderRight: '1px solid var(--border)', color: 'var(--muted-foreground)' },
  '.cm-activeLineGutter': { backgroundColor: 'transparent' },
})

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

/** Side-by-side diff of two versions (§25 diff, §29 history compare). Long lines wrap
 * so both panes always fit; `onChanges` reports how many changed blocks there are. */
export function DiffView({
  original,
  modified,
  language = 'text',
  height = '420px',
  originalLabel = 'Before',
  modifiedLabel = 'After',
  onChanges,
}: {
  original: string
  modified: string
  language?: EditorLanguage
  height?: string
  originalLabel?: string
  modifiedLabel?: string
  onChanges?: (count: number) => void
}) {
  const host = useRef<HTMLDivElement>(null)
  const onChangesRef = useRef(onChanges)
  useEffect(() => {
    onChangesRef.current = onChanges
  })
  const { resolvedTheme } = useTheme()
  const dark = resolvedTheme === 'dark'

  useEffect(() => {
    if (!host.current) return
    const pane = [...diffExtensions(language, dark)]
    const mv = new MergeView({
      parent: host.current,
      a: { doc: original, extensions: pane },
      b: { doc: modified, extensions: pane },
      highlightChanges: true,
      gutter: true,
      // Long unchanged runs fold away so the changes are what you see.
      collapseUnchanged: { margin: 3, minSize: 8 },
    })
    onChangesRef.current?.(mv.chunks.length)
    return () => mv.destroy()
  }, [original, modified, language, dark])

  return (
    <div className="flex flex-col overflow-hidden rounded-lg border border-border bg-card">
      <div className="grid grid-cols-2 border-b border-border text-xs font-medium text-muted-foreground">
        <div className="truncate px-3 py-1.5">{originalLabel}</div>
        <div className="truncate border-l border-border px-3 py-1.5">{modifiedLabel}</div>
      </div>
      <div ref={host} style={{ maxHeight: height }} className="diff-host overflow-y-auto overflow-x-hidden" />
    </div>
  )
}

function diffExtensions(lang: EditorLanguage, dark: boolean): Extension[] {
  return [
    lineNumbers(),
    EditorView.lineWrapping,
    languageExtension(lang),
    syntaxHighlighting(defaultHighlightStyle, { fallback: true }),
    ...(dark ? [oneDark] : []),
    EditorState.readOnly.of(true),
    EditorView.editable.of(false),
    EditorView.theme({
      '&': { fontSize: '12.5px' },
      '.cm-scroller': { fontFamily: 'ui-monospace, SFMono-Regular, Consolas, monospace', lineHeight: '1.55' },
    }),
    appSurface,
  ]
}
