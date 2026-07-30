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
 * Walks every `<script>` tag in `html` and returns one `{kind, bytes}`
 * entry per tag, `bytes` being that tag's actual gzipped contribution to
 * page weight — regardless of whether Astro happened to inline it or chunk
 * it externally under `dist/_astro/`. Replaces an earlier version of this
 * budget test that only measured inline `<script type="module">` bodies:
 * that approach went vacuous (silently measured 0 bytes, budget check kept
 * "passing") the moment Astro's own inlining heuristic
 * (`config.build.assetsInlineLimit`, checked against the bundled script's
 * own byte size) decided a script had grown too large to inline and
 * started chunking it externally instead — which is exactly what happened
 * to enhance.mjs's bundle once Task 4 grew it past ~4 kB raw. This version
 * can't go vacuous in either mode: a `src` script asserts its target file
 * actually exists under `dist/` before gzipping it; a bodied script
 * asserts its body is non-empty before gzipping that. Either kind failing
 * its own assertion means an accounting bug, not a silent zero.
 *
 * `application/ld+json` tags are skipped — structured data, not executable
 * JS, and irrelevant to a JS budget.
 */
function pageScriptBytes(html, distRoot) {
  const scriptRe = /<script\b([^>]*)>([\s\S]*?)<\/script>/g
  const entries = []
  let match
  while ((match = scriptRe.exec(html))) {
    const [, attrsRaw, body] = match
    if (/type\s*=\s*["']application\/ld\+json["']/.test(attrsRaw)) continue

    const srcMatch = attrsRaw.match(/\bsrc\s*=\s*["']([^"']+)["']/)
    if (srcMatch) {
      const filePath = join(distRoot, srcMatch[1])
      assert.ok(existsSync(filePath), `script src "${srcMatch[1]}" does not resolve to a file under dist/`)
      entries.push({ kind: srcMatch[1], bytes: gzipSync(readFileSync(filePath)).length })
    } else {
      assert.ok(body.trim().length > 0, 'an inline <script> (no src) has an empty body')
      entries.push({ kind: '(inline)', bytes: gzipSync(Buffer.from(body)).length })
    }
  }
  return entries
}

test('landing JavaScript stays under 20 kB gzipped', (t) => {
  if (!existsSync(DIST)) return t.skip('run `npm run build` first')

  const html = readFileSync(join(DIST, 'index.html'), 'utf8')
  const scripts = pageScriptBytes(html, DIST)

  // Non-vacuousness: expect at least the pre-paint enhancement gate (see
  // index.astro) and enhance.mjs itself — two real contributions, in
  // whatever mix of inline/external Astro happens to choose for either of
  // them on a given build. Each must actually weigh something; a 0-byte
  // entry would mean the accounting above broke, not that the script is
  // free. Was ">= 3" (gate + a standalone ambience bootstrap + enhance)
  // before ambience's bootstrap moved into enhance.mjs itself (see that
  // file's `initPageAmbience`) — index.astro dropped to exactly these two
  // script tags, so the floor drops to match, not because the accounting
  // got looser.
  assert.ok(
    scripts.length >= 2,
    `expected at least 2 <script> contributions (gate + enhance, which now also bootstraps ambience), found ${scripts.length}: ${scripts.map((s) => s.kind).join(', ')}`,
  )
  for (const s of scripts) {
    assert.ok(s.bytes > 0, `script "${s.kind}" contributed 0 gzip bytes`)
  }

  const total = scripts.reduce((sum, s) => sum + s.bytes, 0)
  const kb = (total / 1024).toFixed(1)
  const breakdown = scripts.map((s) => `${s.kind}: ${(s.bytes / 1024).toFixed(2)} kB`).join(', ')
  assert.ok(
    total <= BUDGET_BYTES,
    `landing JS is ${kb} kB gzipped (${breakdown}), budget is 20 kB`,
  )
})

test('landing external chunk directory alone stays under 20 kB gzipped (conservative)', (t) => {
  if (!existsSync(DIST)) return t.skip('run `npm run build` first')

  // A second, cruder assertion kept alongside `pageScriptBytes` above:
  // sums every `_astro/*.js` file regardless of whether `<script>` tags in
  // `index.html` actually reference all of them. Catches the case the
  // tag-walker can't — a JS chunk emitted but never linked from the page
  // (dead weight still shipped in the build output) wouldn't show up in
  // `pageScriptBytes`'s per-tag accounting at all.
  const files = jsFiles(join(DIST, '_astro'))
  const total = files.reduce((sum, file) => sum + gzipSync(readFileSync(file)).length, 0)
  const kb = (total / 1024).toFixed(1)
  assert.ok(total <= BUDGET_BYTES, `external _astro/*.js alone is already ${kb} kB gzipped, budget is 20 kB`)
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
