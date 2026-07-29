import { test } from 'node:test'
import assert from 'node:assert/strict'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import { loadManifest, parseManifest } from '../src/lib/manifest.mjs'

function valid() {
  return {
    plugins: [{
      id: 'cc-uplink',
      name: 'cc-uplink',
      version: '0.1.0',
      description: 'desc',
      tagline: 'tag',
      intro: 'intro',
      repository: 'https://github.com/XBlueSky/cc-uplink',
      mcp: [{
        name: 'cc-uplink',
        provides: ['channel_list', 'channel_describe', 'channel_send',
                   'channel_invoke', 'channel_recv', 'channel_doctor'],
        setup: [{ text: 'nothing to install' }, { text: 'log in', cmd: 'codex login' }],
      }],
      tips: [{ text: 'tip one' }],
      traps: [{ text: 'trap one' }],
      skills: [{ name: 'uplink', trigger: 'when messaging a peer', examples: ['ask codex'] }],
    }],
  }
}

test('normalises a valid manifest', () => {
  const m = parseManifest(valid())
  assert.equal(m.tagline, 'tag')
  assert.equal(m.version, '0.1.0')
  assert.deepEqual(m.tips, ['tip one'])
  assert.deepEqual(m.traps, ['trap one'])
  assert.equal(m.tools.length, 6)
  assert.equal(m.setup[1].cmd, 'codex login')
  assert.equal(m.skill.name, 'uplink')
})

test('accepts a JSON string', () => {
  assert.equal(parseManifest(JSON.stringify(valid())).tagline, 'tag')
})

test('throws when there are no plugins', () => {
  assert.throws(() => parseManifest({ plugins: [] }), /no plugins/)
})

for (const field of ['tagline', 'intro', 'version', 'description']) {
  test(`throws when ${field} is missing`, () => {
    const data = valid()
    delete data.plugins[0][field]
    assert.throws(() => parseManifest(data), new RegExp(field))
  })

  test(`throws when ${field} is empty`, () => {
    const data = valid()
    data.plugins[0][field] = '   '
    assert.throws(() => parseManifest(data), new RegExp(field))
  })
}

test('throws when the tool count is not exactly six', () => {
  const data = valid()
  data.plugins[0].mcp[0].provides.push('channel_extra')
  assert.throws(() => parseManifest(data), /six fixed tools/)
})

test('tolerates absent tips, traps, and skills', () => {
  const data = valid()
  delete data.plugins[0].tips
  delete data.plugins[0].traps
  delete data.plugins[0].skills
  const m = parseManifest(data)
  assert.deepEqual(m.tips, [])
  assert.deepEqual(m.traps, [])
  assert.equal(m.skill, null)
})

test('throws when a tool entry is not a string', () => {
  const data = valid()
  data.plugins[0].mcp[0].provides[2] = 42
  assert.throws(() => parseManifest(data), /tools\[2\]/)
})

test('throws when a tool entry is blank', () => {
  const data = valid()
  data.plugins[0].mcp[0].provides[5] = '   '
  assert.throws(() => parseManifest(data), /tools\[5\]/)
})

test('throws when a tip entry is blank, naming the array and index', () => {
  const data = valid()
  data.plugins[0].tips = [{ text: 'fine' }, { text: '   ' }]
  assert.throws(() => parseManifest(data), /tips\[1\]/)
})

test('throws when a trap entry is blank, naming the array and index', () => {
  const data = valid()
  data.plugins[0].traps = [{ text: '' }]
  assert.throws(() => parseManifest(data), /traps\[0\]/)
})

test('loadManifest requires an explicit path — no silent default under bundling', () => {
  assert.throws(() => loadManifest(), /requires an explicit path/)
  assert.throws(() => loadManifest(undefined), /requires an explicit path/)
})

test('loadManifest throws a helpful error when the manifest file is missing', () => {
  const missingPath = resolve(
    dirname(fileURLToPath(import.meta.url)),
    'fixtures-that-do-not-exist',
    'manifest.json',
  )
  assert.throws(
    () => loadManifest(missingPath),
    (err) => {
      assert.ok(err.message.includes(missingPath), 'error message should include the path')
      assert.match(err.message, /npm run sync/)
      assert.ok(err.cause, 'the original fs error should be preserved as cause')
      return true
    },
  )
})
