// One-off: rasterize assets/icon.svg -> assets/icon.png at 1024px for `cargo tauri icon`.
import { Resvg } from '@resvg/resvg-js'
import { readFileSync, writeFileSync } from 'node:fs'

const svg = readFileSync('assets/icon.svg')
const resvg = new Resvg(svg, { fitTo: { mode: 'width', value: 1024 } })
const png = resvg.render().asPng()
writeFileSync('assets/icon.png', png)
console.log('wrote assets/icon.png')
