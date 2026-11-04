// Regenerates ui/src/components/faIconsData.json, the offline icon set behind the
// site and project icon pickers, from the curated list in icons.json.
//
// The SVG path data is read from the @fortawesome/free-solid-svg-icons and
// @fortawesome/free-brands-svg-icons packages in ui/node_modules at build time.
// The app itself never imports those packages and never goes online for icons:
// it only reads the generated JSON, which Vite bundles into the app.
//
// icons.json holds { "categories": [{ "id", "label" }], "icons": [...] }. The category
// order there is the order of the picker's filter chips and dropdown.
//
// To add an icon: add { "id", "name", "category", "color"? } to "icons", where "id" is
// the Font Awesome icon name (for example "drupal" or "graduation-cap") and "category"
// is one of the listed category ids, then run scripts\build-fa-icons.bat (or
// `npm run icons:fa` inside ui/). The pack (brands or solid) is found automatically;
// an id that exists in both packs needs "pack": "brands" or "pack": "solid".
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

const problems = []
const categories = Array.isArray(list?.categories) ? list.categories : []
const CATEGORIES = new Set()
for (const cat of categories) {
  if (!cat?.id || !cat?.label) problems.push(`category ${JSON.stringify(cat)}: needs "id" and "label"`)
  else if (cat.id === 'all') problems.push('category "all" is reserved for the picker\'s show-everything option')
  else if (CATEGORIES.has(cat.id)) problems.push(`category ${cat.id}: listed twice`)
  else CATEGORIES.add(cat.id)
}
if (!CATEGORIES.size || !Array.isArray(list?.icons)) {
  fail(
    'scripts/build-fa-icons/icons.json has the wrong shape.',
    'it must be { "categories": [{ "id", "label" }], "icons": [{ "id", "name", "category" }] } with at least one category.',
    'restore the file from git or fix its structure, then run this again.',
  )
}

const seen = new Set()
const out = []
for (const entry of list.icons) {
  const { id, name, category, color, pack } = entry
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
  const inBrands = brandIndex.has(id)
  const inSolid = solidIndex.has(id)
  let def
  if (pack === 'brands' || pack === 'solid') {
    def = (pack === 'brands' ? brandIndex : solidIndex).get(id)
    if (!def) {
      problems.push(`${id}: not in the Font Awesome Free ${pack} pack`)
      continue
    }
  } else if (pack !== undefined) {
    problems.push(`${id}: "pack" must be "brands" or "solid"`)
    continue
  } else if (inBrands && inSolid) {
    problems.push(`${id}: exists in both the brands and solid packs; add "pack": "brands" or "pack": "solid"`)
    continue
  } else {
    def = brandIndex.get(id) ?? solidIndex.get(id)
    if (!def) {
      problems.push(`${id}: not in Font Awesome Free (brands or solid)`)
      continue
    }
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

const empty = [...CATEGORIES].filter((c) => !out.some((icon) => icon.category === c))
if (empty.length) {
  fail(
    `category ${empty.join(', ')} has no icons.`,
    'an empty category would show as a filter chip that always lists nothing.',
    'add icons to it or remove it from "categories" in icons.json, then run this again.',
  )
}

const data = { categories: categories.map(({ id, label }) => ({ id, label })), icons: out }
writeFileSync(outPath, JSON.stringify(data, null, 2) + '\n')
console.log(`[ok] Wrote ${out.length} icons in ${CATEGORIES.size} categories to ui/src/components/faIconsData.json`)
console.log(`     ${categories.map(({ id }) => `${id} ${out.filter((icon) => icon.category === id).length}`).join(', ')}`)
