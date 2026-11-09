export interface ExportFinding {
  package: string
  severity: string
  title: string
  advisory?: string | null
  url?: string | null
  affected?: string | null
  fix?: string | null
}

export interface ExportItem {
  project: string
  tool: string
  f: ExportFinding
}

export interface ExportEol {
  runtime: string
  version: string
  usedBy: string[]
}

/** One finding as plain text, for the clipboard. */
export function formatFinding({ project, tool, f }: ExportItem): string {
  const lines = [
    `[${f.severity.toUpperCase()}] ${f.package} (${tool}) in ${project}`,
    f.title,
    f.advisory ? `Advisory: ${f.advisory}` : '',
    f.affected ? `Affected: ${f.affected}` : '',
    f.fix ? `Fix: ${f.fix}` : 'Fix: no fixed version listed',
    f.url ? `Link: ${f.url}` : '',
  ]
  return lines.filter(Boolean).join('\n')
}

/** Every problem as one Markdown report: ended versions first, then findings per project. */
export function formatReport(items: ExportItem[], ended: ExportEol[], when: Date): string {
  const out = [`# OLS security report`, '', `Created ${when.toISOString()}`, '']
  if (ended.length > 0) {
    out.push('## Versions that no longer get security fixes', '')
    for (const e of ended) {
      out.push(`- ${e.runtime} ${e.version}${e.usedBy.length ? ` (used by ${e.usedBy.join(', ')})` : ''}`)
    }
    out.push('')
  }
  const projects = [...new Set(items.map((i) => i.project))]
  for (const project of projects) {
    const mine = items.filter((i) => i.project === project)
    out.push(`## ${project} (${mine.length} finding${mine.length === 1 ? '' : 's'})`, '')
    for (const i of mine) {
      const [head, ...rest] = formatFinding(i).split('\n')
      out.push(`### ${head}`, '', ...rest.map((l) => `- ${l}`), '')
    }
  }
  if (items.length === 0 && ended.length === 0) out.push('Nothing to report.', '')
  return out.join('\n')
}
