import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'

/**
 * WCAG 2.1 relative luminance / contrast ratio, straight from the spec
 * (https://www.w3.org/TR/WCAG21/#dfn-relative-luminance). No shortcuts: the
 * design spec treats contrast as a hard constraint verified by measurement,
 * not by eye, and this is what closed the Shiki `github-dark` regression
 * (comments measured 3.05:1 against that theme's own background — under the
 * 4.5:1 AA floor this test enforces).
 */
function relativeLuminance(hex) {
  const n = hex.replace('#', '')
  const [r, g, b] = [0, 2, 4].map((i) => parseInt(n.slice(i, i + 2), 16) / 255)
  const linear = (c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4)
  const [R, G, B] = [r, g, b].map(linear)
  return 0.2126 * R + 0.7152 * G + 0.0722 * B
}

function contrastRatio(hexA, hexB) {
  const [l1, l2] = [relativeLuminance(hexA), relativeLuminance(hexB)].sort((a, b) => b - a)
  return (l1 + 0.05) / (l2 + 0.05)
}

const TOKENS_CSS = new URL('../src/styles/tokens.css', import.meta.url).pathname
const AA_FLOOR = 4.5

/**
 * A tiny, purpose-built resolver for this one file: reads every
 * `--name: value;` declaration inside the `:root { ... }` block and resolves
 * `var(--x)` references by substitution until every value is a literal hex
 * colour. This is not a general CSS parser — it doesn't need to be, since
 * tokens.css only ever assigns a hex literal or a single `var(--x)`
 * reference per custom property.
 */
function readCssVariables(css) {
  const root = css.match(/:root\s*{([^}]*)}/s)?.[1] ?? ''
  const raw = new Map()
  for (const match of root.matchAll(/--([\w-]+)\s*:\s*([^;]+);/g)) {
    raw.set(match[1], match[2].trim())
  }

  const resolved = new Map()
  function resolve(name, seen = new Set()) {
    if (resolved.has(name)) return resolved.get(name)
    if (seen.has(name)) throw new Error(`circular var() reference at --${name}`)
    seen.add(name)
    const value = raw.get(name)
    if (value === undefined) throw new Error(`--${name} is not defined in tokens.css`)
    const varRef = value.match(/^var\(--([\w-]+)\)$/)
    const out = varRef ? resolve(varRef[1], seen) : value
    resolved.set(name, out)
    return out
  }
  for (const name of raw.keys()) resolve(name)
  return resolved
}

const css = readFileSync(TOKENS_CSS, 'utf8')
const vars = readCssVariables(css)

test('tokens.css defines the astro-code CSS variables Shiki\'s css-variables theme needs', () => {
  // If any of these were missing, `var(--astro-code-*)` would be invalid at
  // computed-value time and Shiki's inline style would fall back to the
  // property's initial value instead of a palette colour — e.g. text
  // rendered in the browser's default black, invisible on this dark page.
  for (const name of [
    'astro-code-foreground',
    'astro-code-background',
    'astro-code-token-comment',
    'astro-code-token-keyword',
    'astro-code-token-string',
    'astro-code-token-string-expression',
    'astro-code-token-constant',
    'astro-code-token-function',
  ]) {
    assert.ok(vars.has(name), `tokens.css is missing --${name}`)
  }
})

test('every Shiki content colour clears the WCAG AA floor (4.5:1) against its actual background', () => {
  const background = vars.get('astro-code-background')
  assert.ok(background, 'astro-code-background is not resolved to a literal colour')

  // Every token role Shiki's css-variables theme can emit for a code span
  // (see `shiki.createCssVariablesTheme()`'s tokenColors) — not just the two
  // (bash, toml) that today's docs content happens to exercise, so a future
  // doc using a different language can't quietly reintroduce a low-contrast
  // token this test never looked at.
  const roles = [
    'astro-code-foreground',
    'astro-code-token-comment',
    'astro-code-token-keyword',
    'astro-code-token-string',
    'astro-code-token-string-expression',
    'astro-code-token-parameter',
    'astro-code-token-link',
    'astro-code-token-punctuation',
    'astro-code-token-constant',
    'astro-code-token-function',
    'astro-code-token-inserted',
    'astro-code-token-deleted',
    'astro-code-token-changed',
  ]

  const failures = []
  for (const role of roles) {
    const color = vars.get(role)
    if (!color) {
      failures.push(`--${role} is not defined`)
      continue
    }
    const ratio = contrastRatio(color, background)
    if (ratio < AA_FLOOR) {
      failures.push(`--${role} (${color}) on --astro-code-background (${background}): ${ratio.toFixed(2)}:1`)
    }
  }

  assert.deepEqual(failures, [], `colours under the ${AA_FLOOR}:1 AA floor:\n${failures.join('\n')}`)
})

test('regression pin: the comment token specifically clears 4.5:1 (this is the finding that started this test)', () => {
  const ratio = contrastRatio(vars.get('astro-code-token-comment'), vars.get('astro-code-background'))
  assert.ok(
    ratio >= AA_FLOOR,
    `comment colour is ${ratio.toFixed(2)}:1 against its background, under the ${AA_FLOOR}:1 AA floor`,
  )
})
