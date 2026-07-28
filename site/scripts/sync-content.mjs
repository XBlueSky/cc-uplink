#!/usr/bin/env node
import { execFileSync } from 'node:child_process'
import { copyFileSync, mkdirSync, readdirSync, rmSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

/**
 * Directories under docs/ that must never reach the public site.
 * docs/superpowers/ holds internal specs and plans.
 */
export const EXCLUDED_DIRS = ['superpowers']

/**
 * Throw if any relative path lives under a directory that must never be
 * published. Returns its input unchanged so it can wrap a collection call.
 *
 * Only whole path segments count: `superpowers/specs/x.md` is rejected,
 * `superpowers-notes.md` is fine.
 *
 * collectDocs is single-level today, so nothing it produces can fail this
 * check — that is deliberate. This is the guard that makes a future change to
 * a recursive walk fail loudly instead of silently publishing internal specs,
 * and keeping it as its own exported function is what makes it testable.
 *
 * @param {string[]} relPaths
 * @returns {string[]}
 */
export function assertPublishable(relPaths) {
  for (const relPath of relPaths) {
    const dirSegments = relPath.split(/[\\/]/).slice(0, -1)
    const hit = dirSegments.find((segment) => EXCLUDED_DIRS.includes(segment))
    if (hit) {
      throw new Error(`refusing to publish ${relPath}: ${hit}/ is internal`)
    }
  }
  return relPaths
}

/**
 * Top-level markdown files in docsDir, sorted.
 *
 * Deliberately single-level: a recursive walk would sweep docs/superpowers/
 * onto the public site.
 *
 * @param {string} docsDir
 * @returns {string[]}
 */
export function collectDocs(docsDir) {
  const files = readdirSync(docsDir, { withFileTypes: true })
    .filter((entry) => entry.isFile() && entry.name.endsWith('.md'))
    .map((entry) => entry.name)
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

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main()
}
