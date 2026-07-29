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

  // Mirrors assertPublishable's semantics in sync-content.mjs: match whole
  // path segments, case-insensitively, rather than bare substrings. A
  // substring check (e.g. `-superpowers-`) would false-positive on an
  // innocent asset name like `chunk-superpowers-report.js`; segment
  // matching removes that whole class of false positive while still
  // catching a leak nested at any depth (`docs/superpowers/...`,
  // `docs/Superpowers/...`).
  const paths = walk(DIST)
  for (const excluded of EXCLUDED_DIRS) {
    const excludedLower = excluded.toLowerCase()
    const leaked = paths.filter((p) =>
      p.split(/[\\/]/).some((segment) => segment.toLowerCase() === excludedLower),
    )
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

test('dist ships a real 404 page', (t) => {
  if (!existsSync(DIST)) return t.skip('run `npm run build` first')

  // Without dist/404.html Cloudflare Pages assumes an SPA and rewrites every
  // unknown path to /index.html with HTTP 200 — which is exactly how the
  // "internal docs must 404 in production" check failed on 2026-07-29 (the
  // guards had kept the content out of dist, but misses answered 200 with the
  // landing page). This asserts the page that switches Pages back to real
  // 404 semantics never silently disappears.
  const html = readFileSync(join(DIST, '404.html'), 'utf8')
  assert.ok(html.includes('no such pane'), '404.html lost its content')
  assert.ok(
    !/<script(?![^>]*type="application\/ld\+json")/i.test(html),
    '404 page must ship no JavaScript',
  )
})

test('docs pages ship no JavaScript', (t) => {
  if (!existsSync(DIST)) return t.skip('run `npm run build` first')

  // Every page actually on disk, not a hardcoded sample of one — so a page
  // added later is covered automatically instead of silently slipping past
  // this guard the way a hardcoded list would.
  const docsDir = join(DIST, 'docs')
  const pages = readdirSync(docsDir, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)

  assert.ok(pages.length > 0, 'expected at least one built docs page')

  for (const page of pages) {
    const htmlPath = join(docsDir, page, 'index.html')
    const html = readFileSync(htmlPath, 'utf8')
    assert.ok(
      !/<script(?![^>]*type="application\/ld\+json")/i.test(html),
      `docs page shipped a script tag: ${htmlPath}`,
    )
  }
})
