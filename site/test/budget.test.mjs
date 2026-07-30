import { test } from 'node:test'
import assert from 'node:assert/strict'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'
import { gzipSync } from 'node:zlib'

const DIST = new URL('../dist/', import.meta.url).pathname
const BUDGET_BYTES = 20 * 1024

function jsFiles(dir) {
  if (!existsSync(dir)) return []
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = join(dir, entry.name)
    if (entry.isDirectory()) return jsFiles(full)
    return entry.name.endsWith('.js') ? [full] : []
  })
}

test('landing ships the enhancement script bundle', (t) => {
  if (!existsSync(DIST)) return t.skip('run `npm run build` first')

  const files = jsFiles(join(DIST, '_astro'))
  assert.ok(
    files.length > 0,
    'the landing shipped no JS bundle — did the ambience bootstrap script get dropped?',
  )

  const html = readFileSync(join(DIST, 'index.html'), 'utf8')
  assert.ok(html.includes('data-page'), 'landing HTML is missing the data-page root')
  assert.ok(/<script type="module"/.test(html), 'landing HTML has no module script tag')
})

/**
 * Total gzipped bytes of every inline `<script type="module">…</script>`
 * body in `html`. Astro inlines small page scripts directly into the HTML
 * rather than emitting them as external chunks under `_astro/` — that's
 * where enhance.mjs's own bundle currently lands. Measuring only the
 * external `_astro/*.js` files (as this test did before) silently
 * under-counts exactly the JS this budget exists to cap: a page could ship
 * an arbitrarily large inline bundle and still read as "0 kB of JS" here.
 */
function inlineModuleScriptBytes(html) {
  const re = /<script type="module">([\s\S]*?)<\/script>/g
  let total = 0
  let match
  while ((match = re.exec(html))) {
    total += gzipSync(Buffer.from(match[1])).length
  }
  return total
}

test('landing JavaScript stays under 20 kB gzipped', (t) => {
  if (!existsSync(DIST)) return t.skip('run `npm run build` first')

  const files = jsFiles(join(DIST, '_astro'))
  const externalTotal = files.reduce(
    (sum, file) => sum + gzipSync(readFileSync(file)).length,
    0,
  )
  const html = readFileSync(join(DIST, 'index.html'), 'utf8')
  const inlineTotal = inlineModuleScriptBytes(html)

  // Non-vacuousness guard: enhance.mjs currently ships as an inline
  // `<script type="module">` body (see inlineModuleScriptBytes' doc
  // comment), so this count should never be 0. Without this assertion, if
  // Astro ever stopped inlining it — external chunking, or an attribute
  // landing between `type="module"` and `>` so the regex above no longer
  // matches — inlineModuleScriptBytes would silently return 0 and the
  // budget check below would keep passing while quietly measuring nothing,
  // resurrecting the exact undercounting bug this function exists to fix.
  assert.ok(
    inlineTotal > 0,
    'expected at least one inline module script body — did Astro stop inlining enhance.mjs?',
  )

  const total = externalTotal + inlineTotal
  const kb = (total / 1024).toFixed(1)
  assert.ok(
    total <= BUDGET_BYTES,
    `landing JS is ${kb} kB gzipped (${(externalTotal / 1024).toFixed(1)} kB external + ` +
      `${(inlineTotal / 1024).toFixed(1)} kB inline), budget is 20 kB`,
  )
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
