import {
  Activity,
  AlertTriangle,
  Bug,
  Compass,
  Cpu,
  Database,
  FileCode2,
  FileText,
  FolderGit2,
  Globe,
  HardDrive,
  Layers,
  LayoutGrid,
  Lock,
  Maximize2,
  MonitorDown,
  MousePointerClick,
  Palette,
  PlusCircle,
  Radio,
  Server,
  ShieldCheck,
  SlidersHorizontal,
  Sparkles,
  Terminal,
  Wrench,
  Zap,
  type LucideIcon,
} from 'lucide-react'
import { marked, type Tokens } from 'marked'

export interface ReleaseMetadata {
  title: string
  version: string
  subtitle?: string
  date?: string
  platform?: string
  license?: string
  summary?: string
}

export interface FeatureItem {
  id: string
  title: string
  description: string
  rawText: string
  tags?: string[]
}

export interface ReleaseSubsection {
  id: string
  title: string
  features: FeatureItem[]
  content: Tokens.Generic[]
}

export interface ReleaseSection {
  id: string
  title: string
  level: number
  badgeType: 'highlights' | 'added' | 'improved' | 'fixed' | 'security' | 'warning' | 'technical' | 'default'
  features: FeatureItem[]
  subsections: ReleaseSubsection[]
  content: Tokens.Generic[]
}

export interface ParsedReleaseNotes {
  metadata: ReleaseMetadata
  sections: ReleaseSection[]
  hasSpecialStructure: boolean
  rawMarkdown: string
}

export interface SectionBadgeConfig {
  label: string
  icon: LucideIcon
  className: string
}

/**
 * Returns an appropriate Lucide icon matching the feature topic.
 */
export function getFeatureIcon(title: string, sectionTitle?: string): LucideIcon {
  const t = title.toLowerCase()
  const s = (sectionTitle || '').toLowerCase()

  // Diagnostics, Folders & Projects
  if (t.includes('folder') || t.includes('directory') || t.includes('path')) return FolderGit2
  if (t.includes('drive') || t.includes('disk') || t.includes('volume') || t.includes('storage')) return HardDrive
  if (t.includes('diagnostic') || t.includes('doctor') || t.includes('repair')) return Activity

  // Sites, Domains & Networking
  if (t.includes('multi-domain') || t.includes('domain') || t.includes('host') || t.includes('dns')) return Globe
  if (t.includes('site') || t.includes('vhost')) return LayoutGrid
  if (t.includes('tunnel') || t.includes('broadcast') || t.includes('cloudflare') || t.includes('ngrok')) return Radio

  // Appearance, Themes & Navigation
  if (t.includes('appearance') || t.includes('theme') || t.includes('aurora') || t.includes('color') || t.includes('preset')) return Palette
  if (t.includes('menu') || t.includes('context') || t.includes('mouse') || t.includes('click')) return MousePointerClick
  if (t.includes('tray') || t.includes('tooltip') || t.includes('taskbar') || t.includes('notification')) return MonitorDown
  if (t.includes('scroll') || t.includes('viewport') || t.includes('clamping') || t.includes('window')) return Maximize2
  if (t.includes('navigation') || t.includes('sidebar') || t.includes('icon')) return Compass

  // Runtimes & Languages
  if (t.includes('php') || t.includes('opcache') || t.includes('fastcgi') || t.includes('composer')) return FileCode2
  if (t.includes('python') || t.includes('venv') || t.includes('pip')) return Terminal
  if (t.includes('node') || t.includes('npm') || t.includes('bun') || t.includes('corepack')) return Cpu

  // Systems & Security
  if (t.includes('elevation') || t.includes('uac') || t.includes('admin') || t.includes('security') || t.includes('helper')) return ShieldCheck
  if (t.includes('database') || t.includes('mysql') || t.includes('mariadb') || t.includes('postgres') || t.includes('sqlite')) return Database
  if (t.includes('web server') || t.includes('nginx') || t.includes('apache') || t.includes('caddy')) return Server
  if (t.includes('batch') || t.includes('grouping') || t.includes('deduplication')) return Layers
  if (t.includes('lock') || t.includes('keyring') || t.includes('secret') || t.includes('token')) return Lock

  // Bug Fixes & Reliability
  if (s.includes('fix') || t.includes('fix') || t.includes('bug') || t.includes('crash') || t.includes('error')) return Bug

  // Performance & Features
  if (t.includes('performance') || t.includes('fast') || t.includes('speed') || t.includes('aslr') || t.includes('cache')) return Zap

  return Sparkles
}

/**
 * Returns badge styling and icon for a release section.
 */
export function getSectionBadge(title: string): SectionBadgeConfig {
  const t = title.toLowerCase()

  if (t.includes('highlight') || t.includes('key feature')) {
    return {
      label: 'Highlights',
      icon: Sparkles,
      className: 'text-primary bg-primary/10 border-primary/20',
    }
  }
  if (t.includes('add') || t.includes('new')) {
    return {
      label: 'New Features',
      icon: PlusCircle,
      className: 'text-emerald-500 bg-emerald-500/10 border-emerald-500/20',
    }
  }
  if (t.includes('improv') || t.includes('change')) {
    return {
      label: 'Improvements',
      icon: SlidersHorizontal,
      className: 'text-sky-500 bg-sky-500/10 border-sky-500/20',
    }
  }
  if (t.includes('fix') || t.includes('resolved')) {
    return {
      label: 'Bug Fixes',
      icon: Wrench,
      className: 'text-indigo-500 bg-indigo-500/10 border-indigo-500/20',
    }
  }
  if (t.includes('security')) {
    return {
      label: 'Security',
      icon: ShieldCheck,
      className: 'text-violet-500 bg-violet-500/10 border-violet-500/20',
    }
  }
  if (t.includes('break') || t.includes('warning') || t.includes('limitation')) {
    return {
      label: 'Notice',
      icon: AlertTriangle,
      className: 'text-amber-500 bg-amber-500/10 border-amber-500/20',
    }
  }
  if (t.includes('technical') || t.includes('architecture')) {
    return {
      label: 'Technical',
      icon: Terminal,
      className: 'text-cyan-500 bg-cyan-500/10 border-cyan-500/20',
    }
  }

  return {
    label: title,
    icon: FileText,
    className: 'text-muted-foreground bg-muted/60 border-border/80',
  }
}

/**
 * Parses release notes Markdown into structured, accessible data models.
 */
export function parseReleaseNotes(markdown: string): ParsedReleaseNotes {
  const lines = markdown.split('\n')
  const metadata: ReleaseMetadata = {
    title: 'Open Local Server',
    version: '',
  }

  let lineIdx = 0

  // 1. Extract H1 Header
  while (lineIdx < lines.length && !lines[lineIdx].startsWith('# ')) {
    lineIdx++
  }
  if (lineIdx < lines.length) {
    const rawTitle = lines[lineIdx].replace(/^#\s+/, '').trim()
    lineIdx++

    const vMatch = rawTitle.match(/(v\d+\.\d+\.\d+(?:-[\w.]+)?)/i)
    if (vMatch) metadata.version = vMatch[1]

    const subMatch = rawTitle.match(/:\s*(.*)$/)
    if (subMatch) metadata.subtitle = subMatch[1].trim()
  }

  // 2. Extract Metadata Row (Release date, Platform, License)
  while (lineIdx < lines.length && !lines[lineIdx].trim()) lineIdx++
  if (lineIdx < lines.length && lines[lineIdx].includes('**Release date:**')) {
    const metaLine = lines[lineIdx]
    lineIdx++

    const dateMatch = metaLine.match(/\*\*Release date:\*\*\s*([^\s·]+)/i)
    if (dateMatch) metadata.date = dateMatch[1].trim()

    const platMatch = metaLine.match(/\*\*Platform:\*\*\s*([^·]+)/i)
    if (platMatch) metadata.platform = platMatch[1].trim()

    const licMatch = metaLine.match(/\*\*License:\*\*\s*(.*)$/i)
    if (licMatch) metadata.license = licMatch[1].trim()
  }

  // 3. Extract Summary Lead Paragraph
  while (lineIdx < lines.length && !lines[lineIdx].trim()) lineIdx++
  const summaryLines: string[] = []
  while (lineIdx < lines.length && lines[lineIdx].trim() && !lines[lineIdx].startsWith('#')) {
    summaryLines.push(lines[lineIdx].trim())
    lineIdx++
  }
  if (summaryLines.length > 0) {
    metadata.summary = summaryLines.join(' ')
  }

  // 4. Parse Sections & Features using Marked Tokenizer
  const tokens = marked.lexer(markdown)
  const sections: ReleaseSection[] = []
  let currentSection: ReleaseSection | null = null
  let currentSubsection: ReleaseSubsection | null = null
  let featureCounter = 0

  for (const token of tokens) {
    if (token.type === 'space') continue

    // Level 2 Heading: New major section
    if (token.type === 'heading' && token.depth === 2) {
      currentSubsection = null
      const badge = getSectionBadge(token.text)
      let badgeType: ReleaseSection['badgeType'] = 'default'
      if (badge.label === 'Highlights') badgeType = 'highlights'
      else if (badge.label === 'New Features') badgeType = 'added'
      else if (badge.label === 'Improvements') badgeType = 'improved'
      else if (badge.label === 'Bug Fixes') badgeType = 'fixed'
      else if (badge.label === 'Security') badgeType = 'security'
      else if (badge.label === 'Notice') badgeType = 'warning'
      else if (badge.label === 'Technical') badgeType = 'technical'

      currentSection = {
        id: `section-${sections.length}-${token.text.toLowerCase().replace(/[^\w]+/g, '-')}`,
        title: token.text,
        level: token.depth,
        badgeType,
        features: [],
        subsections: [],
        content: [],
      }
      sections.push(currentSection)
      continue
    }

    if (!currentSection) continue

    // Level 3 Heading: Subsection
    if (token.type === 'heading' && token.depth === 3) {
      currentSubsection = {
        id: `sub-${currentSection.id}-${token.text.toLowerCase().replace(/[^\w]+/g, '-')}`,
        title: token.text,
        features: [],
        content: [],
      }
      currentSection.subsections.push(currentSubsection)
      continue
    }

    const targetList = currentSubsection ? currentSubsection.features : currentSection.features
    const targetContent = currentSubsection ? currentSubsection.content : currentSection.content

    // List: Parse each bullet into a FeatureItem
    if (token.type === 'list') {
      for (const item of token.items) {
        const text = item.text.trim()
        featureCounter++
        // `**Title**: text` or `**Title:** text` (the notes use both). The title never
        // contains `*`/`_`, so it cannot run on into a later bold phrase.
        const boldMatch = text.match(/^\s*(?:\*\*|__)([^*_]+?)(?::\s*(?:\*\*|__)|(?:\*\*|__)\s*:)\s*(.*)$/s)
        if (boldMatch) {
          targetList.push({
            id: `feat-${featureCounter}`,
            title: boldMatch[1].trim(),
            description: boldMatch[2].trim(),
            rawText: text,
          })
        } else if (/^\s*(?:\*\*|__)[^*_]+?(?:\*\*|__)/.test(text)) {
          // `**Docker page** (Sites → Docker): search…`: the leading bold phrase is the title.
          const lead = text.match(/^\s*(?:\*\*|__)([^*_]+?)(?:\*\*|__)\s*(.*)$/s)!
          targetList.push({
            id: `feat-${featureCounter}`,
            title: lead[1].trim(),
            description: lead[2].trim(),
            rawText: text,
          })
        } else {
          // If no bold prefix, treat first sentence or entire text
          const firstDot = text.indexOf('.')
          let title = ''
          let description = text
          if (firstDot > 10 && firstDot < 60) {
            title = text.slice(0, firstDot).trim()
            description = text.slice(firstDot + 1).trim()
          }
          targetList.push({
            id: `feat-${featureCounter}`,
            title,
            description,
            rawText: text,
          })
        }
      }
    } else {
      // General markdown content (paragraph, code, table, blockquote)
      targetContent.push(token)
    }
  }

  return {
    metadata,
    sections,
    hasSpecialStructure: sections.length > 0,
    rawMarkdown: markdown,
  }
}

/**
 * Filters parsed release notes according to a search query.
 */
export function filterReleaseNotes(
  parsed: ParsedReleaseNotes,
  query: string,
): { filteredSections: ReleaseSection[]; matchCount: number } {
  const q = query.trim().toLowerCase()
  if (!q) {
    let total = 0
    for (const sec of parsed.sections) {
      total += sec.features.length
      for (const sub of sec.subsections) total += sub.features.length
    }
    return { filteredSections: parsed.sections, matchCount: total }
  }

  let matchCount = 0
  const filteredSections: ReleaseSection[] = []

  for (const sec of parsed.sections) {
    const secMatches = sec.title.toLowerCase().includes(q)
    const matchingFeatures = sec.features.filter(
      (f) =>
        secMatches ||
        f.title.toLowerCase().includes(q) ||
        f.description.toLowerCase().includes(q) ||
        f.rawText.toLowerCase().includes(q),
    )

    const matchingSubsections: ReleaseSubsection[] = []
    for (const sub of sec.subsections) {
      const subMatches = secMatches || sub.title.toLowerCase().includes(q)
      const subFeatures = sub.features.filter(
        (f) =>
          subMatches ||
          f.title.toLowerCase().includes(q) ||
          f.description.toLowerCase().includes(q) ||
          f.rawText.toLowerCase().includes(q),
      )
      if (subMatches || subFeatures.length > 0) {
        matchCount += subFeatures.length
        matchingSubsections.push({
          ...sub,
          features: subFeatures,
        })
      }
    }

    matchCount += matchingFeatures.length

    if (secMatches || matchingFeatures.length > 0 || matchingSubsections.length > 0) {
      filteredSections.push({
        ...sec,
        features: matchingFeatures,
        subsections: matchingSubsections,
      })
    }
  }

  return { filteredSections, matchCount }
}
