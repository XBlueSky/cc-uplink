import { test } from 'node:test'
import assert from 'node:assert/strict'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

import { EXCLUDED_DIRS } from '../scripts/sync-content.mjs'

const DIST = new URL('../dist/', import.meta.url).pathname

function walk(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) =>
    entry.isDirectory() ? walk(join(dir, entry.name)) : [join(dir, entry.name)],
  )
}

test('dist contains no route derived from an internal docs directory', (t) => {
  if (!existsSync(DIST)) return t.skip('run `npm run build` first')

  const paths = walk(DIST)
  for (const excluded of EXCLUDED_DIRS) {
    const leaked = paths.filter((p) => p.includes(`/${excluded}/`) || p.includes(`-${excluded}-`))
    assert.deepEqual(leaked, [], `internal docs leaked into dist: ${leaked.join(', ')}`)
  }
})

test('dist docs routes match exactly the expected public set', (t) => {
  if (!existsSync(DIST)) return t.skip('run `npm run build` first')

  const routes = readdirSync(join(DIST, 'docs'), { withFileTypes: true })
    .filter((e) => e.isDirectory())
    .map((e) => e.name)
    .sort()

  assert.deepEqual(routes, [
    'cli', 'configuration', 'downstream-contracts', 'security', 'wire-contract',
  ])
})

test('docs pages ship no JavaScript', (t) => {
  if (!existsSync(DIST)) return t.skip('run `npm run build` first')

  const html = readFileSync(join(DIST, 'docs', 'cli', 'index.html'), 'utf8')
  assert.ok(!/<script(?![^>]*type="application\/ld\+json")/.test(html),
    'a docs page shipped a script tag')
})
