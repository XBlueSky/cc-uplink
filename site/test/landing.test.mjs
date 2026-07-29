import { test } from 'node:test'
import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'

import { loadManifest, MANIFEST_PATH } from '../src/lib/manifest.mjs'

const INDEX = new URL('../dist/index.html', import.meta.url).pathname

/**
 * Decode the HTML entities Astro emits, so assertions can compare against the
 * manifest's raw text.
 *
 * This is not optional defensiveness: `traps[0]` begins with "Don't poll
 * channel_recv", which renders as `Don&#39;t`, so a substring check against the
 * raw manifest string fails on correct output. `&amp;` must be decoded last, or
 * an encoded `&amp;lt;` would be double-decoded.
 */
function decodeEntities(html) {
  return html
    .replaceAll('&lt;', '<')
    .replaceAll('&gt;', '>')
    .replaceAll('&quot;', '"')
    .replaceAll('&#39;', "'")
    .replaceAll('&#x27;', "'")
    .replaceAll('&amp;', '&')
}

test('landing renders all six acts', (t) => {
  if (!existsSync(INDEX)) return t.skip('run `npm run build` first')
  const html = readFileSync(INDEX, 'utf8')
  for (let act = 0; act <= 5; act += 1) {
    assert.ok(html.includes(`data-act="${act}"`), `act ${act} missing`)
  }
})

test('landing renders every fact from the manifest', (t) => {
  if (!existsSync(INDEX)) return t.skip('run `npm run build` first')
  const text = decodeEntities(readFileSync(INDEX, 'utf8'))
  const m = loadManifest(MANIFEST_PATH)

  for (const tool of m.tools) {
    assert.ok(text.includes(tool), `tool ${tool} missing from landing`)
  }
  assert.ok(text.includes(m.tagline), 'tagline missing')
  assert.ok(m.traps.length > 0, 'manifest declares no traps')
  assert.ok(text.includes(m.traps[0]), 'trap missing')
  assert.ok(m.tips.length > 1, 'manifest declares fewer than two tips')
  assert.ok(text.includes(m.tips[1]), 'tip missing')
})

test('landing derives the install command from the manifest repository, not a hardcoded slug', (t) => {
  if (!existsSync(INDEX)) return t.skip('run `npm run build` first')
  const text = decodeEntities(readFileSync(INDEX, 'utf8'))
  const m = loadManifest(MANIFEST_PATH)

  assert.ok(m.repository, 'manifest declares no repository')
  const slug = m.repository
    .replace(/^https?:\/\/github\.com\//, '')
    .replace(/\.git$/, '')
    .replace(/\/$/, '')
  assert.ok(
    text.includes(`/plugin marketplace add ${slug}`),
    'install command missing the manifest-derived repo slug — did the repo get renamed without updating index.astro?',
  )
})

test('landing derives the install command\'s marketplace half from the manifest, not the plugin name', (t) => {
  if (!existsSync(INDEX)) return t.skip('run `npm run build` first')
  const text = decodeEntities(readFileSync(INDEX, 'utf8'))
  const m = loadManifest(MANIFEST_PATH)

  assert.ok(m.marketplace, 'manifest declares no marketplace name')
  assert.ok(
    text.includes(`/plugin install ${m.name}@${m.marketplace}`),
    "install command missing the manifest-derived marketplace name — did parseManifest stop reading data.marketplace?",
  )
})

test('the entity decoder actually decodes, so the assertions above are not vacuous', () => {
  assert.equal(decodeEntities('Don&#39;t &lt;a&gt; &amp; &quot;x&quot;'), 'Don\'t <a> & "x"')
})

test('landing ships no ambience raster image', (t) => {
  if (!existsSync(INDEX)) return t.skip('run `npm run build` first')
  const html = readFileSync(INDEX, 'utf8')
  assert.ok(!html.includes('ambience-'), 'an ambience reference PNG was shipped')
})

test('act 2 typewriter data-text matches its rendered no-JS fallback text exactly', (t) => {
  if (!existsSync(INDEX)) return t.skip('run `npm run build` first')
  const html = readFileSync(INDEX, 'utf8')

  // Parsed by hand rather than with a `<code ...>` regex: the data-text
  // attribute value legitimately contains a literal `>` (from `<your
  // answer>`), which is valid unescaped inside a double-quoted HTML
  // attribute but breaks any `[^>]*`-based tag-boundary regex — it would
  // mistake that inner `>` for the end of the opening tag.
  const marker = 'data-text="'
  const markerStart = html.indexOf(marker)
  assert.ok(markerStart !== -1, 'data-text attribute not found — did the typewriter hook get dropped?')

  const valueStart = markerStart + marker.length
  const valueEnd = html.indexOf('"', valueStart)
  const dataTextRaw = html.slice(valueStart, valueEnd)

  const tagCloseIdx = html.indexOf('>', valueEnd)
  const codeCloseIdx = html.indexOf('</code>', tagCloseIdx)
  assert.ok(codeCloseIdx !== -1, 'closing </code> not found after data-text')
  const codeTextRaw = html.slice(tagCloseIdx + 1, codeCloseIdx)

  const dataText = decodeEntities(dataTextRaw)
  const codeText = decodeEntities(codeTextRaw)

  assert.ok(dataText.length > 0, 'data-text decoded to an empty string')
  assert.equal(
    dataText,
    codeText,
    'data-text must decode to exactly the rendered <code> fallback text — a no-JS visitor and the typewriter must agree',
  )
})

test('landing carries all four act-choreography hooks, each exactly once', (t) => {
  if (!existsSync(INDEX)) return t.skip('run `npm run build` first')
  const html = readFileSync(INDEX, 'utf8')

  for (const hook of ['data-split-target', 'data-typewriter', 'data-signal', 'data-reply']) {
    const count = html.split(hook).length - 1
    assert.equal(count, 1, `expected exactly one "${hook}", found ${count}`)
  }
})
