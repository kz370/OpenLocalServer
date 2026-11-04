// Regenerates ui/src/components/faIconsData.json, the offline icon set behind the
// site and project icon pickers, from the curated list in icons.json.
//
// The SVG path data is read from the @fortawesome/free-solid-svg-icons and
// @fortawesome/free-brands-svg-icons packages in ui/node_modules at build time.
// The app itself never imports those packages and never goes online for icons:
// it only reads the generated JSON, which Vite bundles into the app.
//
// To add an icon: add { "id", "name", "category", "color"? } to icons.json, where
// "id" is the Font Awesome icon name (for example "drupal" or "graduation-cap"),
// then run scripts\build-fa-icons.bat (or `npm run icons:fa` inside ui/).
//
// Font Awesome Free icons are licensed CC BY 4.0 (https://fontawesome.com/license/free).

import { readFileSync, writeFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const root = resolve(here, '..', '..')
const uiDir = join(root, 'ui')
const listPath = join(here, 'icons.json')
const outPath = join(uiDir, 'src', 'components', 'faIconsData.json')

/** Keep in sync with FaIconDef['category'] in ui/src/components/SiteLogo.tsx. */
const CATEGORIES = new Set(['brands', 'web', 'tech', 'commerce', 'media', 'lifestyle', 'general'])

function fail(problem, cause, fix) {
  console.error(`[x] Problem: ${problem}`)
  console.error(`    Cause: ${cause}`)
  console.error(`    Fix: ${fix}`)
  process.exit(1)
}

const require = createRequire(join(uiDir, 'package.json'))
let solid, brands
try {
  solid = require('@fortawesome/free-solid-svg-icons').fas
  brands = require('@fortawesome/free-brands-svg-icons').fab
} catch {
  fail(
    'the Font Awesome packages could not be loaded.',
    'ui/node_modules is missing @fortawesome/free-solid-svg-icons or @fortawesome/free-brands-svg-icons.',
    'run `npm install` inside ui/ and run this again.',
  )
}

function index(pack) {
  const map = new Map()
  for (const def of Object.values(pack)) {
    if (def && def.iconName && Array.isArray(def.icon)) map.set(def.iconName, def)
  }
  return map
}
const solidIndex = index(solid)
const brandIndex = index(brands)

let list
try {
  list = JSON.parse(readFileSync(listPath, 'utf8'))
} catch (err) {
  fail('scripts/build-fa-icons/icons.json could not be read.', String(err.message ?? err), 'fix the JSON syntax and run this again.')
}

const seen = new Set()
const problems = []
const out = []
for (const entry of list) {
  const { id, name, category, color } = entry
  if (!id || !name || !category) {
    problems.push(`${JSON.stringify(entry)}: needs "id", "name" and "category"`)
    continue
  }
  if (seen.has(id)) {
    problems.push(`${id}: listed twice`)
    continue
  }
  seen.add(id)
  if (!CATEGORIES.has(category)) {
    problems.push(`${id}: unknown category "${category}" (use one of ${[...CATEGORIES].join(', ')})`)
    continue
  }
  // Brand logos live only in the brands pack; everything else comes from solid.
  const def = category === 'brands' ? brandIndex.get(id) : solidIndex.get(id)
  if (!def) {
    const other = category === 'brands' ? solidIndex.has(id) : brandIndex.has(id)
    problems.push(
      other
        ? `${id}: exists in the ${category === 'brands' ? 'solid' : 'brands'} pack, so its category is wrong`
        : `${id}: not in Font Awesome Free ${category === 'brands' ? 'brands' : 'solid'}`,
    )
    continue
  }
  const [width, height, , , path] = def.icon
  if (typeof path !== 'string') {
    problems.push(`${id}: has a multi-path (duotone) SVG, which the renderer does not support`)
    continue
  }
  const icon = { id, name, category, viewBox: `0 0 ${width} ${height}` }
  if (color) icon.color = color
  icon.svgPath = path
  out.push(icon)
}

if (problems.length) {
  fail(
    `${problems.length} icon(s) in icons.json are invalid:\n      - ${problems.join('\n      - ')}`,
    'the list names an icon id, category or field that the Font Awesome packages do not provide.',
    'correct or remove those entries in scripts/build-fa-icons/icons.json and run this again.',
  )
}

writeFileSync(outPath, JSON.stringify(out, null, 2) + '\n')
const counts = {}
for (const icon of out) counts[icon.category] = (counts[icon.category] ?? 0) + 1
console.log(`[ok] Wrote ${out.length} icons to ui/src/components/faIconsData.json`)
console.log(`     ${Object.entries(counts).map(([c, n]) => `${c} ${n}`).join(', ')}`)
