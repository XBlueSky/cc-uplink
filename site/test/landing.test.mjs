import { test } from 'node:test'
import assert from 'node:assert/strict'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

import { loadManifest, MANIFEST_PATH } from '../src/lib/manifest.mjs'

const INDEX = new URL('../dist/index.html', import.meta.url).pathname
const DIST = new URL('../dist/', import.meta.url).pathname

/**
 * Decode the HTML entities Astro emits, so assertions can compare against the
 * manifest's raw text or against a literal wire string.
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

/**
 * Every `[data-line]` span also carries `data-text` mirroring its own
 * rendered content (see Journey.astro/InvokeDemo.astro: both render the same
 * JS string constant into the attribute and into the element's children, so
 * the two can never hand-drift). `data-text`'s value can legitimately contain
 * a literal `>` (from `<your answer>`), which is valid unescaped inside a
 * double-quoted HTML attribute but breaks any `[^>]*`-based tag-boundary
 * regex — so this walks the string by hand rather than pattern-matching the
 * whole tag.
 */
function findDataLineSpans(html) {
  const spans = []
  const lineMarker = 'data-line'
  const textMarker = 'data-text="'
  let pos = 0
  while (true) {
    const lineIdx = html.indexOf(lineMarker, pos)
    if (lineIdx === -1) break

    const textIdx = html.indexOf(textMarker, lineIdx)
    assert.ok(textIdx !== -1, `data-line at offset ${lineIdx} has no data-text attribute`)
    const valueStart = textIdx + textMarker.length
    const valueEnd = html.indexOf('"', valueStart)
    const dataText = html.slice(valueStart, valueEnd)

    const tagCloseIdx = html.indexOf('>', valueEnd)
    const spanCloseIdx = html.indexOf('</span>', tagCloseIdx)
    assert.ok(spanCloseIdx !== -1, `no closing </span> found for data-line at offset ${lineIdx}`)
    const content = html.slice(tagCloseIdx + 1, spanCloseIdx)

    spans.push({ dataText: decodeEntities(dataText), content: decodeEntities(content) })
    pos = spanCloseIdx + '</span>'.length
  }
  return spans
}

/**
 * Returns a copy of `html` with every `data-text="..."` attribute VALUE
 * blanked out (the surrounding markup, including the quotes, is preserved).
 * `data-line` spans deliberately mirror their own text into `data-text` (see
 * `findDataLineSpans`'s doc comment), so a wire string legitimately occurs
 * twice in the raw HTML source — once as that attribute value, once as the
 * element's visible text. Counting "how many times does this string actually
 * appear on the page" means counting the rendered/visible occurrence only.
 */
function withoutDataTextValues(html) {
  const marker = 'data-text="'
  let result = ''
  let pos = 0
  while (true) {
    const start = html.indexOf(marker, pos)
    if (start === -1) {
      result += html.slice(pos)
      break
    }
    const valueStart = start + marker.length
    const valueEnd = html.indexOf('"', valueStart)
    result += html.slice(pos, valueStart)
    pos = valueEnd
  }
  return result
}

function countOccurrences(haystack, needle) {
  return haystack.split(needle).length - 1
}

function jsFiles(dir) {
  if (!existsSync(dir)) return []
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = join(dir, entry.name)
    if (entry.isDirectory()) return jsFiles(full)
    return entry.name.endsWith('.js') ? [full] : []
  })
}

test('the entity decoder actually decodes, so the assertions below are not vacuous', () => {
  assert.equal(decodeEntities('Don&#39;t &lt;a&gt; &amp; &quot;x&quot;'), 'Don\'t <a> & "x"')
})

test('every [data-line] element\'s textContent equals its data-text attribute, exactly', (t) => {
  if (!existsSync(INDEX)) return t.skip('run `npm run build` first')
  const html = readFileSync(INDEX, 'utf8')

  const spans = findDataLineSpans(html)
  assert.ok(spans.length > 0, 'found no [data-line] spans — did the journey/demo markup get dropped?')

  for (const [i, span] of spans.entries()) {
    assert.ok(span.dataText.length > 0, `[data-line] #${i} has an empty data-text`)
    assert.equal(
      span.content,
      span.dataText,
      `[data-line] #${i}: rendered content must equal data-text exactly (a no-JS visitor and a future typewriter must agree)`,
    )
  }
})

test('the §2 envelope head and tail wire strings each render exactly once', (t) => {
  if (!existsSync(INDEX)) return t.skip('run `npm run build` first')
  const html = readFileSync(INDEX, 'utf8')

  // Byte-exact wire strings, mirroring `src/core/envelope.rs::format_outbound`'s
  // `ReplyHint::Full` shape for from:claude, pane:%1, id:6b3b20e6,
  // message:"review my diff".
  const ENV_HEAD = '[uplink from:claude pane:%1 id:6b3b20e6] review my diff'
  const ENV_TAIL = " (reply: run `tmux send-keys -t %1 -l '[reply id:6b3b20e6] <your answer>' \\; send-keys -t %1 Enter`)"

  const visible = decodeEntities(withoutDataTextValues(html))

  assert.equal(
    countOccurrences(visible, ENV_HEAD),
    1,
    'the envelope head string must render exactly once',
  )
  assert.equal(
    countOccurrences(visible, ENV_TAIL),
    1,
    'the envelope tail (reply-command) string must render exactly once',
  )
})

test('the install command block appears exactly twice (hero + footer)', (t) => {
  if (!existsSync(INDEX)) return t.skip('run `npm run build` first')
  const html = readFileSync(INDEX, 'utf8')
  const m = loadManifest(MANIFEST_PATH)

  assert.ok(m.repository, 'manifest declares no repository')
  const slug = m.repository
    .replace(/^https?:\/\/github\.com\//, '')
    .replace(/\.git$/, '')
    .replace(/\/$/, '')

  assert.ok(m.marketplace, 'manifest declares no marketplace name')
  const installCommand = `/plugin marketplace add ${slug}\n/plugin install ${m.name}@${m.marketplace}`

  const text = decodeEntities(html)
  assert.equal(
    countOccurrences(text, installCommand),
    2,
    'expected the install command block in exactly two places (hero and footer)',
  )
})

test('no dist JS file references gsap or lenis', (t) => {
  if (!existsSync(DIST)) return t.skip('run `npm run build` first')

  const offenders = []
  for (const file of jsFiles(DIST)) {
    const contents = readFileSync(file, 'utf8')
    if (/gsap|lenis/i.test(contents)) offenders.push(file)
  }
  assert.deepEqual(offenders, [], `dist JS still references the removed deck libraries: ${offenders.join(', ')}`)
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

test('landing renders the manifest tagline and all six tools', (t) => {
  if (!existsSync(INDEX)) return t.skip('run `npm run build` first')
  const text = decodeEntities(readFileSync(INDEX, 'utf8'))
  const m = loadManifest(MANIFEST_PATH)

  assert.ok(text.includes(m.tagline), 'tagline missing')
  for (const tool of m.tools) {
    assert.ok(text.includes(tool), `tool ${tool} missing from landing`)
  }
})

test('landing ships no ambience raster image', (t) => {
  if (!existsSync(INDEX)) return t.skip('run `npm run build` first')
  const html = readFileSync(INDEX, 'utf8')
  assert.ok(!html.includes('ambience-'), 'an ambience reference PNG was shipped')
})
