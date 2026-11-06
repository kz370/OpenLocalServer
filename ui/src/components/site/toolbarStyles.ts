/**
 * Shared class names for the compact Sites toolbar. Theme tokens only.
 * Labels hide below the @2xl container breakpoint (the toolbar is an `@container`).
 */
export type ToolbarAppearance = 'group' | 'ghost'

const base = 'h-8 gap-1.5 px-2.5 text-[13px] font-medium text-foreground shadow-none active:scale-100 [&_svg]:size-4 [&_svg]:text-muted-foreground'

export const toolbarItemClass: Record<ToolbarAppearance, string> = {
  // No own border/radius; ring sits inside so the group's overflow-hidden never clips it.
  group: `${base} rounded-none bg-transparent hover:bg-accent hover:text-accent-foreground focus-visible:ring-inset`,
  ghost: `${base} rounded-md bg-transparent hover:bg-accent hover:text-accent-foreground`,
}

export const toolbarLabelClass = 'hidden @2xl:inline'
export const toolbarGroupClass =
  'inline-flex shrink-0 items-center overflow-hidden rounded-md border border-border bg-card [&>button:not(:first-child)]:border-l [&>button:not(:first-child)]:border-border'
