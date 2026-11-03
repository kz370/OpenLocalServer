import {
  Calendar,
  ChevronDown,
  MonitorDown,
  Search,
  Shield,
  X,
} from 'lucide-react'
import { marked, type Tokens } from 'marked'
import { useMemo, useState } from 'react'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { CodeBlock } from '@/components/ui/Markdown'
import {
  filterReleaseNotes,
  getFeatureIcon,
  getSectionBadge,
  parseReleaseNotes,
  type FeatureItem,
  type ReleaseMetadata,
  type ReleaseSection,
  type ReleaseSubsection,
} from '@/lib/releaseNotes'
import { cn } from '@/lib/utils'

interface ReleaseNotesViewProps {
  markdown: string
  searchQuery?: string
  onClearSearch?: () => void
  currentVersion?: string
  className?: string
}

/**
 * Modern, polished Release Notes UI inspired by Linear, GitHub Desktop, and Raycast.
 */
export function ReleaseNotesView({
  markdown,
  searchQuery = '',
  onClearSearch,
  currentVersion = '2.5.0',
  className = '',
}: ReleaseNotesViewProps) {
  const [expandedFeatures, setExpandedFeatures] = useState<Set<string>>(new Set())

  // Parse markdown dynamically
  const parsed = useMemo(() => parseReleaseNotes(markdown), [markdown])

  // Apply search filter
  const { filteredSections, matchCount } = useMemo(
    () => filterReleaseNotes(parsed, searchQuery),
    [parsed, searchQuery],
  )

  const toggleExpand = (id: string) => {
    setExpandedFeatures((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  const isSearching = Boolean(searchQuery.trim())

  return (
    <div className={cn('flex flex-col gap-6 text-foreground', className)}>
      {/* 1. Dedicated Release Header */}
      <ReleaseHeader
        metadata={parsed.metadata}
        currentVersion={currentVersion}
      />

      {/* 2. Search Status Banner (if active) */}
      {isSearching && (
        <div className="flex flex-wrap items-center justify-between gap-2 rounded-xl border border-border/80 bg-muted/40 px-4 py-2.5 text-xs text-muted-foreground shadow-xs">
          <div className="flex items-center gap-2">
            <Search className="size-3.5 text-primary" />
            <span>
              Found <strong className="text-foreground">{matchCount}</strong> result
              {matchCount === 1 ? '' : 's'} matching &ldquo;{searchQuery}&rdquo;
            </span>
          </div>
          {onClearSearch && (
            <button
              onClick={onClearSearch}
              className="flex items-center gap-1 font-medium text-foreground hover:text-primary cursor-pointer transition-colors"
            >
              <X className="size-3" />
              <span>Clear search</span>
            </button>
          )}
        </div>
      )}

      {/* 3. Empty Search State */}
      {isSearching && filteredSections.length === 0 && (
        <div className="flex flex-col items-center justify-center rounded-2xl border border-dashed border-border/80 py-12 text-center">
          <div className="flex size-12 items-center justify-center rounded-full bg-muted/80 text-muted-foreground">
            <Search className="size-6" />
          </div>
          <h3 className="mt-3 text-sm font-semibold text-foreground">No release notes match</h3>
          <p className="mt-1 text-xs text-muted-foreground max-w-sm">
            We couldn&apos;t find any features, improvements, or fixes matching &ldquo;{searchQuery}&rdquo;.
          </p>
          {onClearSearch && (
            <Button
              variant="outline"
              size="sm"
              onClick={onClearSearch}
              className="mt-4 h-8 text-xs cursor-pointer"
            >
              Clear search query
            </Button>
          )}
        </div>
      )}

      {/* 4. Release Sections */}
      <div className="flex flex-col gap-8">
        {filteredSections.map((section) => (
          <SectionView
            key={section.id}
            section={section}
            expandedFeatures={expandedFeatures}
            onToggleExpand={toggleExpand}
          />
        ))}
      </div>
    </div>
  )
}

/**
 * Dedicated Header for Release Notes with app name, prominent version, metadata badges, and lead summary.
 */
function ReleaseHeader({
  metadata,
  currentVersion,
}: {
  metadata: ReleaseMetadata
  currentVersion: string
}) {
  const versionString = metadata.version || `v${currentVersion}`

  return (
    <div className="relative overflow-hidden rounded-2xl border border-border/80 bg-card/60 p-5 sm:p-6 backdrop-blur-md shadow-xs">
      {/* Ambient gradient lighting glow */}
      <div className="pointer-events-none absolute -top-20 -right-20 size-60 rounded-full bg-primary/10 blur-3xl" />

      <div className="relative flex flex-col gap-3.5">
        {/* Top bar: Brand title & subtle badge */}
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
              {metadata.title || 'Open Local Server'}
            </span>
            <span className="text-muted-foreground/30">•</span>
            <Badge
              variant="secondary"
              className="rounded-full px-2.5 py-0.5 font-mono text-[11px] font-medium tracking-tight bg-primary/10 text-primary border-primary/20"
            >
              {versionString}
            </Badge>
            {metadata.subtitle && (
              <span className="text-xs font-medium text-muted-foreground">
                {metadata.subtitle}
              </span>
            )}
          </div>

        </div>

        {/* Prominent Version Heading */}
        <div>
          <h1 className="text-2xl font-bold tracking-tight text-foreground sm:text-3xl">
            Version {versionString.replace(/^v/i, '')} Release Notes
          </h1>
        </div>

        {/* Metadata Badges: Date, Platform, License */}
        <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
          {metadata.date && (
            <div className="flex items-center gap-1.5 rounded-lg border border-border/70 bg-muted/40 px-2.5 py-1">
              <Calendar className="size-3.5 text-primary/80 shrink-0" />
              <span>Released {metadata.date}</span>
            </div>
          )}
          {metadata.platform && (
            <div className="flex items-center gap-1.5 rounded-lg border border-border/70 bg-muted/40 px-2.5 py-1">
              <MonitorDown className="size-3.5 text-primary/80 shrink-0" />
              <span>{metadata.platform}</span>
            </div>
          )}
          {metadata.license && (
            <div className="flex items-center gap-1.5 rounded-lg border border-border/70 bg-muted/40 px-2.5 py-1">
              <Shield className="size-3.5 text-primary/80 shrink-0" />
              <span>{metadata.license}</span>
            </div>
          )}
        </div>

        {/* Lead Release Summary */}
        {metadata.summary && (
          <div className="mt-1 rounded-xl border border-border/60 bg-muted/30 p-4 text-xs sm:text-sm leading-relaxed text-foreground/90 font-normal">
            {metadata.summary}
          </div>
        )}
      </div>
    </div>
  )
}

/**
 * Renders an organized release section (Highlights, Improvements, Bug Fixes, etc.).
 */
function SectionView({
  section,
  expandedFeatures,
  onToggleExpand,
}: {
  section: ReleaseSection
  expandedFeatures: Set<string>
  onToggleExpand: (id: string) => void
}) {
  const badge = getSectionBadge(section.title)
  const BadgeIcon = badge.icon
  const totalItems =
    section.features.length +
    section.subsections.reduce((acc, sub) => acc + sub.features.length, 0)

  return (
    <section className="flex flex-col gap-4">
      {/* Section Header */}
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border/70 pb-3">
        <div className="flex items-center gap-2.5">
          <span
            className={cn(
              'flex items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-xs font-medium tracking-tight shadow-2xs',
              badge.className,
            )}
          >
            <BadgeIcon className="size-3.5 shrink-0" />
            <span>{badge.label}</span>
          </span>
          <h2 className="text-base font-semibold text-foreground tracking-tight sm:text-lg">
            {section.title}
          </h2>
        </div>

        {totalItems > 0 && (
          <span className="text-xs text-muted-foreground font-mono">
            {totalItems} item{totalItems === 1 ? '' : 's'}
          </span>
        )}
      </div>

      {/* Direct Feature Cards Grid */}
      {section.features.length > 0 && (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-3.5">
          {section.features.map((feature) => (
            <FeatureCard
              key={feature.id}
              feature={feature}
              sectionTitle={section.title}
              isExpanded={expandedFeatures.has(feature.id)}
              onToggleExpand={() => onToggleExpand(feature.id)}
            />
          ))}
        </div>
      )}

      {/* Subsections (e.g. v1.0.0 toolchains, sites, etc.) */}
      {section.subsections.map((sub) => (
        <SubsectionView
          key={sub.id}
          subsection={sub}
          sectionTitle={section.title}
          expandedFeatures={expandedFeatures}
          onToggleExpand={onToggleExpand}
        />
      ))}

      {/* Standard Markdown Content Blocks (Paragraphs, Callouts, Tables, etc.) */}
      {section.content.length > 0 && (
        <div className="flex flex-col gap-3 rounded-xl border border-border/70 bg-card/40 p-4">
          {section.content.map((token, idx) => (
            <ContentBlock key={idx} token={token} />
          ))}
        </div>
      )}
    </section>
  )
}

/**
 * Subsection view with header and feature cards.
 */
function SubsectionView({
  subsection,
  sectionTitle,
  expandedFeatures,
  onToggleExpand,
}: {
  subsection: ReleaseSubsection
  sectionTitle: string
  expandedFeatures: Set<string>
  onToggleExpand: (id: string) => void
}) {
  return (
    <div className="flex flex-col gap-3 pt-2">
      <div className="flex items-center gap-2">
        <span className="size-1.5 rounded-full bg-primary/70 shrink-0" />
        <h3 className="text-sm font-semibold tracking-tight text-foreground">
          {subsection.title}
        </h3>
        {subsection.features.length > 0 && (
          <span className="text-[11px] text-muted-foreground font-mono">
            ({subsection.features.length})
          </span>
        )}
      </div>

      {subsection.features.length > 0 && (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-3.5">
          {subsection.features.map((feature) => (
            <FeatureCard
              key={feature.id}
              feature={feature}
              sectionTitle={sectionTitle}
              isExpanded={expandedFeatures.has(feature.id)}
              onToggleExpand={() => onToggleExpand(feature.id)}
            />
          ))}
        </div>
      )}

      {subsection.content.length > 0 && (
        <div className="flex flex-col gap-2 rounded-lg border border-border/60 bg-card/40 p-3.5">
          {subsection.content.map((token, idx) => (
            <ContentBlock key={idx} token={token} />
          ))}
        </div>
      )}
    </div>
  )
}

/**
 * Compact, polished Feature Card item with matching Lucide icon and readable typography.
 */
function FeatureCard({
  feature,
  sectionTitle,
  isExpanded,
  onToggleExpand,
}: {
  feature: FeatureItem
  sectionTitle: string
  isExpanded: boolean
  onToggleExpand: () => void
}) {
  const Icon = getFeatureIcon(feature.title || feature.description, sectionTitle)

  // Parse inline markdown safely (links, bold, code pills)
  const renderedDescription = useMemo(() => {
    try {
      return marked.parseInline(feature.description) as string
    } catch {
      return feature.description
    }
  }, [feature.description])

  const isLongDescription = feature.description.length > 220

  return (
    <div className="group relative flex flex-col justify-between rounded-xl border border-border/70 bg-card/50 p-4 transition-all duration-150 hover:border-border hover:bg-card/90 hover:shadow-xs">
      <div>
        <div className="flex items-start gap-3">
          {/* Feature Icon Badge */}
          <div className="flex size-8 shrink-0 items-center justify-center rounded-lg border border-border/80 bg-accent/60 text-primary shadow-xs transition-colors group-hover:border-primary/40 group-hover:bg-primary/10">
            <Icon className="size-4" />
          </div>

          {/* Feature Title & Content */}
          <div className="min-w-0 flex-1">
            {feature.title ? (
              <h4 className="text-[13px] font-semibold tracking-tight text-foreground leading-snug">
                {feature.title}
              </h4>
            ) : null}

            <div
              className={cn(
                'mt-1.5 text-xs leading-relaxed text-muted-foreground release-notes-prose',
                !isExpanded && isLongDescription && 'line-clamp-3',
              )}
              dangerouslySetInnerHTML={{ __html: renderedDescription }}
            />
          </div>
        </div>
      </div>

      {/* Expand / Collapse for long descriptions */}
      {isLongDescription && (
        <div className="mt-2.5 pt-2 border-t border-border/40 flex justify-end">
          <button
            type="button"
            onClick={onToggleExpand}
            className="flex items-center gap-1 text-[11px] font-medium text-muted-foreground hover:text-foreground cursor-pointer transition-colors"
          >
            <span>{isExpanded ? 'Less details' : 'More details'}</span>
            <ChevronDown className={cn('size-3 transition-transform', isExpanded && 'rotate-180')} />
          </button>
        </div>
      )}
    </div>
  )
}

/**
 * Renders standard markdown blocks (paragraphs, code blocks, tables, blockquotes, lists).
 */
function ContentBlock({ token }: { token: Tokens.Generic }) {
  if (token.type === 'paragraph') {
    const html = marked.parseInline(token.text) as string
    return (
      <p
        className="text-xs sm:text-sm leading-relaxed text-foreground/90 release-notes-prose"
        dangerouslySetInnerHTML={{ __html: html }}
      />
    )
  }

  if (token.type === 'code') {
    return <CodeBlock code={token.text} language={token.lang || 'bash'} />
  }

  if (token.type === 'blockquote') {
    const html = marked.parse(token.text) as string
    return (
      <blockquote
        className="border-l-2 border-primary/70 bg-primary/5 px-3 py-2 text-xs italic text-foreground rounded-r-md release-notes-prose"
        dangerouslySetInnerHTML={{ __html: html }}
      />
    )
  }

  if (token.type === 'table') {
    return (
      <div className="overflow-x-auto my-2 rounded-lg border border-border/80">
        <table className="w-full text-xs text-left">
          <thead className="bg-muted/70 text-foreground font-semibold border-b border-border/80">
            <tr>
              {token.header?.map((h: Tokens.TableCell, i: number) => (
                <th key={i} className="px-3 py-2">
                  {h.text}
                </th>
              ))}
            </tr>
          </thead>
          <tbody className="divide-y divide-border/60">
            {token.rows?.map((row: Tokens.TableCell[], ri: number) => (
              <tr key={ri} className="hover:bg-muted/20">
                {row.map((cell: Tokens.TableCell, ci: number) => (
                  <td key={ci} className="px-3 py-2 text-muted-foreground">
                    {cell.text}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    )
  }

  // Fallback to parsed html
  const raw = token.raw || token.text || ''
  if (!raw.trim()) return null
  const html = marked.parse(raw) as string
  return (
    <div
      className="text-xs leading-relaxed text-foreground/90 release-notes-prose"
      dangerouslySetInnerHTML={{ __html: html }}
    />
  )
}
