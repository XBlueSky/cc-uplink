#!/usr/bin/env node
import { execFileSync } from 'node:child_process'
import { copyFileSync, mkdirSync, readdirSync, realpathSync, rmSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

/**
 * Directories under docs/ that must never reach the public site.
 * docs/superpowers/ holds internal specs and plans.
 *
 * Frozen: this is a second consumer's safety contract (Task 7's build-output
 * guard imports it too), so it must not be mutable from an importer.
 */
export const EXCLUDED_DIRS = Object.freeze(['superpowers'])

/**
 * Throw if any relative path lives under a directory that must never be
 * published. Returns its input unchanged so it can wrap a collection call.
 *
 * Every path segment is checked, not just the directory segments before the
 * final basename — a bare directory path like `superpowers` (no filename)
 * must be rejected too, so a future caller that hands this paths instead of
 * files still gets protection. The comparison is case-insensitive because a
 * case-renamed `docs/Superpowers/` is, on a case-insensitive filesystem, the
 * same directory still holding the internal specs.
 *
 * Only whole path segments count: `superpowers/specs/x.md` is rejected,
 * `superpowers-notes.md` is fine (that's a filename, not a segment).
 *
 * `collectDocs` passes this function paths relative to `docsDir`, so a future
 * change to a recursive walk produces `superpowers/specs/x.md` here instead
 * of the bare basename `x.md` — that is the whole point of routing collected
 * paths (not raw basenames) through this guard: it is what makes a future
 * regression fail loudly instead of silently publishing internal specs.
 * Keeping the guard as its own exported function is what makes it testable.
 *
 * @param {string[]} relPaths
 * @returns {string[]}
 */
export function assertPublishable(relPaths) {
  if (!Array.isArray(relPaths)) {
    throw new Error('assertPublishable expects an array of relative paths')
  }
  for (const relPath of relPaths) {
    const segments = relPath.split(/[\\/]/)
    const hit = segments.find((segment) => EXCLUDED_DIRS.includes(segment.toLowerCase()))
    if (hit) {
      throw new Error(`refusing to publish ${relPath}: ${hit}/ is internal`)
    }
  }
  return relPaths
}

/**
 * Top-level markdown files in docsDir, sorted, expressed as paths relative to
 * docsDir (today identical to bare basenames, since this walk is single-level).
 *
 * Deliberately single-level: a recursive walk would sweep docs/superpowers/
 * onto the public site. Paths are computed relative to docsDir — rather than
 * taken as bare `entry.name` — specifically so that if this ever grows a
 * `recursive: true` option, assertPublishable receives `superpowers/specs/x.md`
 * instead of just `x.md` and can still catch the regression.
 *
 * @param {string} docsDir
 * @returns {string[]}
 */
export function collectDocs(docsDir) {
  const base = resolve(docsDir)
  const files = readdirSync(docsDir, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith('.md'))
    .map((entry) => relative(base, resolve(entry.parentPath ?? base, entry.name)))
    .sort()

  return assertPublishable(files)
}

function main() {
  const siteDir = resolve(dirname(fileURLToPath(import.meta.url)), '..')
  const repoRoot = resolve(siteDir, '..')
  const docsSrc = join(repoRoot, 'docs')
  const docsDest = join(siteDir, 'src', 'content', 'docs')

  execFileSync('npx', ['@xbluesky/cc-marketspec@latest'], {
    cwd: repoRoot,
    stdio: 'inherit',
  })

  rmSync(docsDest, { recursive: true, force: true })
  mkdirSync(docsDest, { recursive: true })

  const files = collectDocs(docsSrc)
  for (const name of files) {
    copyFileSync(join(docsSrc, name), join(docsDest, name))
  }
  console.log(`sync-content: manifest generated, ${files.length} docs copied: ${files.join(', ')}`)
}

/**
 * True when this file was invoked directly (e.g. `node scripts/sync-content.mjs`
 * or `npm run sync`), false when it is only being imported (e.g. by the test
 * suite). Compares resolved real paths rather than the raw strings: if the
 * repo is reached through a symlink, `process.argv[1]` and
 * `fileURLToPath(import.meta.url)` can disagree even though they name the
 * same file, and a plain `===` would silently skip `main()` — `npm run sync`
 * would then print nothing and exit 0 while the site builds from a stale or
 * empty content directory. Guarded so a missing argv[1] cannot throw.
 *
 * Not using `import.meta.main`: it is unavailable on Node 22, and
 * site/package.json requires only `engines.node >= 22.12.0`.
 *
 * @returns {boolean}
 */
function isMainModule() {
  if (!process.argv[1]) {
    return false
  }
  try {
    return realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))
  } catch {
    return false
  }
}

if (isMainModule()) {
  main()
}
