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
