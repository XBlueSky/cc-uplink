import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import { assertPublishable, collectDocs, EXCLUDED_DIRS } from '../scripts/sync-content.mjs'

function fixture() {
  const dir = mkdtempSync(join(tmpdir(), 'cc-uplink-docs-'))
  writeFileSync(join(dir, 'wire-contract.md'), '# wire')
  writeFileSync(join(dir, 'cli.md'), '# cli')
  writeFileSync(join(dir, 'notes.txt'), 'not markdown')
  mkdirSync(join(dir, 'superpowers', 'specs'), { recursive: true })
  writeFileSync(join(dir, 'superpowers', 'specs', 'secret-design.md'), '# internal')
  return dir
}

test('superpowers is on the exclusion list', () => {
  assert.ok(EXCLUDED_DIRS.includes('superpowers'))
})

test('collects only top-level markdown, sorted', () => {
  assert.deepEqual(collectDocs(fixture()), ['cli.md', 'wire-contract.md'])
})

test('collects nothing out of an excluded subdirectory', () => {
  const collected = collectDocs(fixture())
  assert.ok(!collected.some((name) => name.includes('secret-design')))
  for (const excluded of EXCLUDED_DIRS) {
    assert.ok(!collected.some((name) => name.includes(excluded)))
  }
})

test('assertPublishable rejects a path under an excluded directory', () => {
  assert.throws(
    () => assertPublishable(['cli.md', 'superpowers/specs/secret-design.md']),
    /superpowers/,
  )
})

test('assertPublishable rejects an excluded segment nested deeper', () => {
  assert.throws(() => assertPublishable(['a/superpowers/b.md']), /superpowers/)
})

test('assertPublishable allows a file merely named like an excluded directory', () => {
  // The check is on path segments, not on substrings of a filename.
  assert.deepEqual(assertPublishable(['superpowers-notes.md']), ['superpowers-notes.md'])
})

test('assertPublishable returns its input unchanged when all paths are publishable', () => {
  const paths = ['cli.md', 'wire-contract.md']
  assert.deepEqual(assertPublishable(paths), paths)
})
