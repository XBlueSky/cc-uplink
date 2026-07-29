import { test } from 'node:test'
import assert from 'node:assert/strict'

import { ACTS, actAt, clamp01, localProgress, typedSlice } from '../src/lib/actState.mjs'

test('the six acts tile 0..1 with no gaps or overlaps', () => {
  assert.equal(ACTS.length, 6)
  assert.equal(ACTS[0].start, 0)
  assert.equal(ACTS[5].end, 1)
  for (let i = 1; i < ACTS.length; i += 1) {
    assert.equal(ACTS[i].start, ACTS[i - 1].end, `gap before act ${i}`)
    assert.ok(ACTS[i].end > ACTS[i].start, `act ${i} has no width`)
  }
})

test('clamp01 clamps both ends', () => {
  assert.equal(clamp01(-3), 0)
  assert.equal(clamp01(0.4), 0.4)
  assert.equal(clamp01(7), 1)
})

test('clamp01 is total: non-finite input maps to 0', () => {
  assert.equal(clamp01(undefined), 0)
  assert.equal(clamp01(null), 0)
  assert.equal(clamp01(NaN), 0)
  assert.equal(clamp01(Infinity), 0)
})

test('actAt maps progress onto the right act', () => {
  assert.equal(actAt(0).id, 0)
  assert.equal(actAt(0.11).id, 0)
  assert.equal(actAt(0.12).id, 1)
  assert.equal(actAt(0.47).id, 2)
  assert.equal(actAt(0.48).id, 3)
  assert.equal(actAt(0.79).id, 4)
  assert.equal(actAt(0.8).id, 5)
  assert.equal(actAt(1).id, 5)
})

test('actAt clamps out-of-range input instead of returning undefined', () => {
  assert.equal(actAt(-1).id, 0)
  assert.equal(actAt(2).id, 5)
})

test('actAt treats non-finite input as 0 via clamp01', () => {
  assert.equal(actAt(undefined).id, 0)
})

test('localProgress runs 0..1 within each act', () => {
  assert.equal(localProgress(0), 0)
  assert.ok(Math.abs(localProgress(0.06) - 0.5) < 0.01)
  assert.equal(localProgress(1), 1)
  // At an act boundary the new act starts at 0, not at 1.
  assert.equal(localProgress(0.12), 0)
})

test('typedSlice reveals text proportionally', () => {
  assert.equal(typedSlice('hello', 0), '')
  assert.equal(typedSlice('hello', 1), 'hello')
  assert.equal(typedSlice('hello', 0.4), 'he')
})

test('typedSlice is deterministic and reversible — scrubbing back un-types', () => {
  const text = 'tmux send-keys -t %1 reply Enter'
  const forward = [0, 0.25, 0.5, 0.75, 1].map((p) => typedSlice(text, p))
  const backward = [1, 0.75, 0.5, 0.25, 0].map((p) => typedSlice(text, p))
  assert.deepEqual(forward, backward.reverse())
  for (let i = 1; i < forward.length; i += 1) {
    assert.ok(forward[i].startsWith(forward[i - 1]), 'typing must only ever extend')
  }
})

test('typedSlice tolerates empty text', () => {
  assert.equal(typedSlice('', 0.5), '')
})
