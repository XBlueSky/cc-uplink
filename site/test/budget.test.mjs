import { test } from 'node:test'
import assert from 'node:assert/strict'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { gzipSync } from 'node:zlib'

const DIST = new URL('../dist/', import.meta.url).pathname
const BUDGET_BYTES = 60 * 1024

function jsFiles(dir) {
  if (!existsSync(dir)) return []
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = join(dir, entry.name)
    if (entry.isDirectory()) return jsFiles(full)
    return entry.name.endsWith('.js') ? [full] : []
  })
}

test('landing ships the timeline script bundle', (t) => {
  if (!existsSync(DIST)) return t.skip('run `npm run build` first')

  const files = jsFiles(join(DIST, '_astro'))
  assert.ok(
    files.length > 0,
    'the landing shipped no JS bundle — did the timeline script get dropped?',
  )

  const html = readFileSync(join(DIST, 'index.html'), 'utf8')
  assert.ok(html.includes('data-acts'), 'landing HTML is missing the data-acts root')
  assert.ok(/<script type="module"/.test(html), 'landing HTML has no module script tag')
})

test('landing JavaScript stays under 60 kB gzipped', (t) => {
  if (!existsSync(DIST)) return t.skip('run `npm run build` first')

  const files = jsFiles(join(DIST, '_astro'))
  const total = files.reduce(
    (sum, file) => sum + gzipSync(readFileSync(file)).length,
    0,
  )
  const kb = (total / 1024).toFixed(1)
  assert.ok(total <= BUDGET_BYTES, `landing JS is ${kb} kB gzipped, budget is 60 kB`)
})

test('docs pages reference no script bundle', (t) => {
  const page = join(DIST, 'docs', 'cli', 'index.html')
  if (!existsSync(page)) return t.skip('run `npm run build` first')

  const html = readFileSync(page, 'utf8')
  assert.ok(!/<script(?![^>]*type="application\/ld\+json")/.test(html),
    'a docs page shipped a script tag')
})

test('no ambience reference PNG was copied into the build', (t) => {
  if (!existsSync(DIST)) return t.skip('run `npm run build` first')

  const leaked = readdirSync(DIST, { recursive: true })
    .filter((name) => String(name).includes('ambience-'))
  assert.deepEqual(leaked, [], `art-direction references shipped: ${leaked.join(', ')}`)
})
