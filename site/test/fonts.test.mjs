import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  existsSync, readdirSync, readFileSync, statSync,
} from 'node:fs'
import { join } from 'node:path'

const DIST = new URL('../dist/', import.meta.url).pathname
const FONT_NAMES = ['instrument-serif-regular.woff2', 'instrument-serif-italic.woff2']
const FONTS_BUDGET_BYTES = 60 * 1024

function cssFiles(dir) {
  if (!existsSync(dir)) return []
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = join(dir, entry.name)
    if (entry.isDirectory()) return cssFiles(full)
    return entry.name.endsWith('.css') ? [full] : []
  })
}

/**
 * Astro may inline small stylesheets straight into the page's <style> tag
 * (build.inlineStylesheets defaults to 'auto') instead of always emitting
 * a separate dist/_astro/*.css chunk. Search both so this test doesn't
 * depend on which way Astro's size heuristic happens to land.
 */
function builtCss() {
  const files = cssFiles(join(DIST, '_astro'))
  const external = files.map((f) => readFileSync(f, 'utf8')).join('\n')
  const indexHtml = join(DIST, 'index.html')
  const inline = existsSync(indexHtml) ? readFileSync(indexHtml, 'utf8') : ''
  return { combined: external + '\n' + inline, fileCount: files.length }
}

test('Instrument Serif woff2 files and the OFL licence ship in the build', (t) => {
  if (!existsSync(DIST)) return t.skip('run `npm run build` first')

  for (const name of [...FONT_NAMES, 'OFL.txt']) {
    const path = join(DIST, 'fonts', name)
    assert.ok(existsSync(path), `dist/fonts/${name} is missing`)
  }
})

test('the two Instrument Serif woff2 files combined stay under 60 kB', (t) => {
  if (!existsSync(DIST)) return t.skip('run `npm run build` first')

  const sizes = FONT_NAMES.map((name) => statSync(join(DIST, 'fonts', name)).size)
  const total = sizes.reduce((sum, size) => sum + size, 0)
  const kb = (total / 1024).toFixed(1)
  assert.ok(
    total <= FONTS_BUDGET_BYTES,
    `Instrument Serif woff2 files are ${kb} kB combined, budget is 60 kB`,
  )
})

test('built CSS declares Instrument Serif with font-display: swap', (t) => {
  if (!existsSync(DIST)) return t.skip('run `npm run build` first')

  const { combined, fileCount } = builtCss()
  assert.ok(
    fileCount > 0 || combined.includes('<style'),
    'no built CSS found (neither dist/_astro/*.css nor an inline <style> tag)',
  )
  assert.ok(combined.includes('Instrument Serif'), 'built CSS never mentions Instrument Serif')
  assert.ok(
    combined.includes('font-display:swap') || combined.includes('font-display: swap'),
    'built CSS is missing font-display: swap',
  )
})
