import { test } from 'node:test'
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdtempSync,
  mkdirSync,
  readdirSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import { assertPublishable, collectDocs, EXCLUDED_DIRS } from '../scripts/sync-content.mjs'

function fixture(t) {
  const dir = mkdtempSync(join(tmpdir(), 'cc-uplink-docs-'))
  t.after(() => rmSync(dir, { recursive: true, force: true }))
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

test('collects only top-level markdown, sorted', (t) => {
  assert.deepEqual(collectDocs(fixture(t)), ['cli.md', 'wire-contract.md'])
})

test('collects nothing out of an excluded subdirectory', (t) => {
  const collected = collectDocs(fixture(t))
  // Exact match, not substring: a legitimate file named e.g. `superpowers-notes.md`
  // must never make this test fail.
  assert.ok(!collected.includes('secret-design.md'))
  for (const excluded of EXCLUDED_DIRS) {
    assert.ok(!collected.includes(excluded))
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

test('assertPublishable rejects a bare excluded directory path with no filename', () => {
  // Regression: the old implementation dropped the final path segment before
  // checking (on the assumption it was always a filename), so a bare
  // directory path with nothing after it — or a directory that is itself the
  // last segment — slipped through unchecked.
  assert.throws(() => assertPublishable(['superpowers']), /superpowers/)
  assert.throws(() => assertPublishable(['a/superpowers']), /superpowers/)
})

test('assertPublishable is case-insensitive for excluded segments', () => {
  // On a case-insensitive filesystem (e.g. default macOS), docs/Superpowers/
  // and docs/superpowers/ are the same directory.
  assert.throws(() => assertPublishable(['Superpowers/x.md']), /Superpowers/)
  assert.throws(() => assertPublishable(['a/SUPERPOWERS/b.md']), /SUPERPOWERS/)
})

test('assertPublishable rejects non-array input instead of silently passing it through', () => {
  // A caller that passes a bare string instead of an array used to iterate
  // characters, match nothing, and return the string unchanged — a silent
  // no-op that reports success.
  assert.throws(
    () => assertPublishable('superpowers/specs/x.md'),
    /expects an array/,
  )
})

test('EXCLUDED_DIRS is frozen against mutation', () => {
  // Task 7 imports this array too. A second consumer means the exported
  // reference must not be mutable out from under either caller.
  assert.ok(Object.isFrozen(EXCLUDED_DIRS))
  assert.throws(() => EXCLUDED_DIRS.push('mutated'))
})

test('assertPublishable throws if collectDocs were ever made recursive', (t) => {
  // Pins the drift protection end to end: read the fixture the way collectDocs
  // would if `recursive: true` were ever added to its readdirSync call, using
  // the exact same relative-path mapping collectDocs uses, and confirm the
  // guard still throws on the nested internal file. This is the scenario the
  // whole guard exists for — without this test we are trusting a comment.
  const dir = fixture(t)
  const base = resolve(dir)
  const files = readdirSync(dir, { recursive: true, withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith('.md'))
    .map((entry) => relative(base, resolve(entry.parentPath ?? base, entry.name)))
    .sort()
  assert.throws(() => assertPublishable(files), /superpowers/)
})

test('running the script via a symlink still executes main() (entry-point regression)', (t) => {
  // Sandbox that mirrors the real docs/ + site/scripts/ layout so main()'s
  // import.meta.url-derived paths stay entirely inside the sandbox and never
  // touch the real repo or the network.
  const root = mkdtempSync(join(tmpdir(), 'cc-uplink-entrypoint-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))

  mkdirSync(join(root, 'docs'), { recursive: true })
  writeFileSync(join(root, 'docs', 'only.md'), '# only')

  mkdirSync(join(root, 'site', 'scripts'), { recursive: true })
  const scriptPath = join(root, 'site', 'scripts', 'sync-content.mjs')
  copyFileSync(fileURLToPath(new URL('../scripts/sync-content.mjs', import.meta.url)), scriptPath)

  // Stub npx as a no-op so this test never touches the network. The real
  // `npx @xbluesky/cc-marketspec@latest` (invoked with `cwd: repoRoot`)
  // writes `.cc-marketspec/dist/manifest.json` under the repo root as a
  // side effect; main() now copies that file into `site/src/data/`, so the
  // stub must produce it too, or that copy fails with ENOENT.
  const binDir = join(root, 'bin')
  mkdirSync(binDir, { recursive: true })
  const npxStub = join(binDir, 'npx')
  writeFileSync(
    npxStub,
    "#!/bin/sh\nmkdir -p .cc-marketspec/dist\necho '{}' > .cc-marketspec/dist/manifest.json\nexit 0\n",
  )
  chmodSync(npxStub, 0o755)

  // The mismatch that broke the old `process.argv[1] === fileURLToPath(...)`
  // check: argv[1] is this symlink's own path, but import.meta.url resolves
  // through to scriptPath.
  const symlinkPath = join(root, 'site', 'scripts', 'entry-alias.mjs')
  symlinkSync(scriptPath, symlinkPath)

  const result = spawnSync(process.execPath, [symlinkPath], {
    env: { ...process.env, PATH: `${binDir}:${process.env.PATH}` },
    encoding: 'utf8',
  })

  assert.equal(result.status, 0, result.stderr)
  assert.match(result.stdout, /sync-content: manifest generated/)
  assert.deepEqual(readdirSync(join(root, 'site', 'src', 'content', 'docs')), ['only.md'])
  assert.ok(
    existsSync(join(root, 'site', 'src', 'data', 'manifest.json')),
    'manifest should be copied into site/src/data/',
  )
})
