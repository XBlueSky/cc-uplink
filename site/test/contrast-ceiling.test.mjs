import { test } from 'node:test'
import assert from 'node:assert/strict'

import {
  FG, LUM_CEILING, contrastRatio, maxBackgroundLuminance, relativeLuminance,
} from '../src/lib/contrast.mjs'

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
