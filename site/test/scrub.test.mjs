import { test } from 'node:test'
import assert from 'node:assert/strict'
import { clamp01, seg, typedSlice } from '../src/lib/scrub.mjs'

test('clamp01 is total', () => {
  assert.equal(clamp01(0.5), 0.5)
  assert.equal(clamp01(-1), 0)
  assert.equal(clamp01(2), 1)
  for (const bad of [undefined, null, NaN, Infinity, -Infinity]) {
    assert.equal(clamp01(bad), 0)
  }
})

test('seg maps a sub-range to 0..1 and clamps outside it', () => {
  assert.equal(seg(0.0, 0.2, 0.6), 0)
  assert.equal(seg(0.2, 0.2, 0.6), 0)
  assert.ok(Math.abs(seg(0.4, 0.2, 0.6) - 0.5) < 1e-10)
  assert.equal(seg(0.6, 0.2, 0.6), 1)
  assert.equal(seg(0.9, 0.2, 0.6), 1)
  assert.equal(seg(NaN, 0.2, 0.6), 0)
})

test('typedSlice reveals characters monotonically and totally', () => {
  const s = 'abcdef'
  assert.equal(typedSlice(s, 0), '')
  assert.equal(typedSlice(s, 0.5), 'abc')
  assert.equal(typedSlice(s, 1), s)
  assert.equal(typedSlice(s, NaN), '')
  assert.equal(typedSlice('', 1), '')
})
