import { test } from 'node:test'
import assert from 'node:assert/strict'

/*
 * Inlined from the now-deleted src/lib/contrast.mjs (Task 1 of the
 * ink-and-light reskin deletes that file along with the retired ambience
 * shader's page-level use of it). This file keeps its own copy — same
 * math, same constants, same exported names as local consts below — so
 * its assertions stay byte-identical to what they verified before the
 * shared lib existed. FG intentionally still names the pre-reskin dark
 * palette's body colour (#e6e9f0, formerly --fg in tokens.css): it is
 * the historical fixed point this ceiling math was built and pinned
 * against, not a live reference into tokens.css, so it does not track
 * --fg's Task-1 remap to the ink-and-light palette.
 */

/** Body text colour; matched the pre-reskin --fg in tokens.css. */
const FG = '#e6e9f0'

/** sRGB channel (0..255) to linear light, per WCAG 2.x. */
function srgbToLinear(channel) {
  const s = channel / 255
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
}

function relativeLuminance(hex) {
  const n = Number.parseInt(hex.replace('#', ''), 16)
  const r = (n >> 16) & 0xff
  const g = (n >> 8) & 0xff
  const b = n & 0xff
  return 0.2126 * srgbToLinear(r) + 0.7152 * srgbToLinear(g) + 0.0722 * srgbToLinear(b)
}

function contrastRatio(l1, l2) {
  const hi = Math.max(l1, l2)
  const lo = Math.min(l1, l2)
  return (hi + 0.05) / (lo + 0.05)
}

/**
 * The highest background relative luminance that still holds `ratio`
 * against `fgHex`. The (now-retired) ambience shader clamped its output
 * to this.
 */
function maxBackgroundLuminance(fgHex = FG, ratio = 4.5) {
  return (relativeLuminance(fgHex) + 0.05) / ratio - 0.05
}

const LUM_CEILING = maxBackgroundLuminance()

test('relative luminance anchors are right', () => {
  assert.ok(Math.abs(relativeLuminance('#ffffff') - 1) < 1e-9)
  assert.equal(relativeLuminance('#000000'), 0)
})

test('white on black is 21:1', () => {
  const ratio = contrastRatio(relativeLuminance('#ffffff'), relativeLuminance('#000000'))
  assert.ok(Math.abs(ratio - 21) < 1e-6, `got ${ratio}`)
})

test('contrastRatio is order-independent', () => {
  const a = relativeLuminance('#e6e9f0')
  const b = relativeLuminance('#06070c')
  assert.equal(contrastRatio(a, b), contrastRatio(b, a))
})

test('the ceiling yields exactly the requested ratio', () => {
  const ceiling = maxBackgroundLuminance(FG, 4.5)
  const ratio = contrastRatio(relativeLuminance(FG), ceiling)
  assert.ok(Math.abs(ratio - 4.5) < 1e-9, `got ${ratio}`)
})

test('LUM_CEILING keeps body text at AA or better', () => {
  const ratio = contrastRatio(relativeLuminance(FG), LUM_CEILING)
  assert.ok(ratio >= 4.5, `AA violated: ${ratio}`)
})

test('LUM_CEILING is a plausible dark-background value', () => {
  // Sanity rails: a bug that returned 0 or 1 would still satisfy the ratio
  // assertions above in one direction, so pin the magnitude too.
  assert.ok(LUM_CEILING > 0.05 && LUM_CEILING < 0.25, `implausible: ${LUM_CEILING}`)
})

test('the base palette colour is comfortably under the ceiling', () => {
  assert.ok(relativeLuminance('#06070c') < LUM_CEILING)
})
