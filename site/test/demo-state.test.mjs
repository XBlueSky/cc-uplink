import { test } from 'node:test'
import assert from 'node:assert/strict'
import { demoState, demoProgress } from '../src/lib/demoState.mjs'

// Generic, round-number-friendly stand-in for the demo's one typed line —
// same rationale as journey-state.test.mjs's ASK/CMD fixtures: decouples
// the math assertions from wording changes in InvokeDemo.astro.
const ASK = '0123456789' // 10 chars
const texts = { askText: ASK }

test('t=0 — nothing typed yet, caret blinking', () => {
  assert.deepEqual(demoState(0, texts), {
    ask: { text: '', caretOn: true },
    callOn: false,
    midOn: false,
    pathsOn: false,
    artOn: false,
  })
})

test('t=0.2 — mid-typing (seg(0.2,.02,.40) = 18/38), nothing else has fired', () => {
  const state = demoState(0.2, texts)
  assert.equal(state.ask.caretOn, true)
  assert.equal(state.callOn, false)
  assert.equal(state.midOn, false)
  assert.equal(state.pathsOn, false)
  assert.equal(state.artOn, false)
  // seg(0.2, 0.02, 0.40) = (0.18)/(0.38) ≈ 0.4737 -> round(0.4737*10) = 5
  assert.equal(state.ask.text, ASK.slice(0, 5))
})

test('t=0.4 — ask fully typed, caret still on (below .44)', () => {
  const state = demoState(0.4, texts)
  assert.equal(state.ask.text, ASK)
  assert.equal(state.ask.caretOn, true)
  assert.equal(state.callOn, false)
})

test('t=0.44 — caret window closes exactly here (caretOn is a < check)', () => {
  assert.equal(demoState(0.44, texts).ask.caretOn, false)
  assert.equal(demoState(0.43999, texts).ask.caretOn, true)
})

test('t=0.48 — the tool-call line lands', () => {
  const state = demoState(0.48, texts)
  assert.equal(state.callOn, true)
  assert.equal(state.midOn, false)
  assert.equal(state.pathsOn, false)
  assert.equal(state.artOn, false)
})

test('t=0.62 — the "codex exec" line lands', () => {
  const state = demoState(0.62, texts)
  assert.equal(state.callOn, true)
  assert.equal(state.midOn, true)
  assert.equal(state.pathsOn, false)
})

test('t=0.74 — the paths line lands', () => {
  const state = demoState(0.74, texts)
  assert.equal(state.midOn, true)
  assert.equal(state.pathsOn, true)
  assert.equal(state.artOn, false)
})

test('t=0.86 — the generated artifact fades in, everything else already settled', () => {
  assert.deepEqual(demoState(0.86, texts), {
    ask: { text: ASK, caretOn: false },
    callOn: true,
    midOn: true,
    pathsOn: true,
    artOn: true,
  })
})

test('t=1 — fully played out', () => {
  assert.deepEqual(demoState(1, texts), {
    ask: { text: ASK, caretOn: false },
    callOn: true,
    midOn: true,
    pathsOn: true,
    artOn: true,
  })
})

test('demoState is total: NaN/out-of-range progress clamps instead of throwing', () => {
  const zero = demoState(0, texts)
  const one = demoState(1, texts)
  assert.deepEqual(demoState(Number.NaN, texts), zero)
  assert.deepEqual(demoState(-5, texts), zero)
  assert.deepEqual(demoState(5, texts), one)
})

// ── demoProgress ────────────────────────────────────────────────────────

test('demoProgress: block below the viewport entirely reports 0', () => {
  // top == viewportHeight -> (vh - top) = 0 -> raw ratio 0 -> seg(...) = 0
  assert.equal(demoProgress({ top: 900, height: 300, viewportHeight: 900 }), 0)
})

test('demoProgress: block has fully scrolled past (bottom at/above 0) reports 1', () => {
  // top == -height -> (vh - top) = vh + height -> raw ratio 1 -> seg(...) = 1
  assert.equal(demoProgress({ top: -300, height: 300, viewportHeight: 900 }), 1)
})

test('demoProgress: raw ratio 0.5 with a square viewport/height maps through seg(.12,.48)', () => {
  // top=0 (block's top pinned exactly at viewport top), height == viewportHeight:
  // raw ratio = (900 - 0) / (900 + 900) = 0.5 -> seg(0.5, 0.12, 0.48) = 0.38/0.36 clamped to 1
  assert.equal(demoProgress({ top: 0, height: 900, viewportHeight: 900 }), 1)
})

test('demoProgress: totality under non-finite input', () => {
  assert.equal(demoProgress({ top: NaN, height: 300, viewportHeight: 900 }), 0)
})
