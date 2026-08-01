import { test } from 'node:test'
import assert from 'node:assert/strict'
import { journeyState, OUTBOUND_PACKET_TEXT, RETURN_PACKET_TEXT } from '../src/lib/journeyState.mjs'

// Generic, round-number-friendly stand-ins for the two typed lines. The
// function under test doesn't care what these say — using fixed-length
// digit/letter strings instead of the real ASK/CMD copy keeps the expected
// typedSlice() lengths easy to verify by hand and decouples this test from
// wording changes in Journey.astro.
const ASK = '0123456789' // 10 chars
const CMD = 'abcdefghijklmnopqrst' // 20 chars
const texts = { askText: ASK, cmdText: CMD }

test('p=0.1 — mid ask-typing, nothing else has started', () => {
  assert.deepEqual(journeyState(0.1, texts), {
    ask: { text: '0123456', caretOn: true },
    sendCallOn: false,
    receiptOn: false,
    envHeadOn: false,
    envTailOn: false,
    peerIdleOpacity: 1,
    work1On: false,
    work2On: false,
    cmd: { text: '', caretOn: false },
    replyOn: false,
    beamOn: false,
    packet: null,
    peerActive: false,
    youFaded: false,
    youActive: true,
    beat: '[1/4]',
  })
})

test('p=0.3 — ask fully typed, call+receipt shown, packet outbound', () => {
  const state = journeyState(0.3, texts)
  assert.deepEqual(state, {
    ask: { text: ASK, caretOn: false },
    sendCallOn: true,
    receiptOn: true,
    envHeadOn: false,
    envTailOn: false,
    peerIdleOpacity: 1,
    work1On: false,
    work2On: false,
    cmd: { text: '', caretOn: false },
    replyOn: false,
    beamOn: true,
    packet: state.packet, // shape asserted separately below (float t)
    peerActive: false,
    youFaded: false,
    youActive: true,
    beat: '[2/4]',
  })
  assert.equal(state.packet.text, OUTBOUND_PACKET_TEXT)
  assert.ok(Math.abs(state.packet.t - 3 / 7) < 1e-9, `expected t≈3/7, got ${state.packet.t}`)
})

test('p=0.5 — envelope + tail landed, codex reading, peer has focus', () => {
  assert.deepEqual(journeyState(0.5, texts), {
    ask: { text: ASK, caretOn: false },
    sendCallOn: true,
    receiptOn: true,
    envHeadOn: true,
    envTailOn: true,
    peerIdleOpacity: 0.35,
    work1On: true,
    work2On: false,
    cmd: { text: '', caretOn: false },
    replyOn: false,
    beamOn: false,
    packet: null,
    peerActive: true,
    youFaded: true,
    youActive: false,
    beat: '[3/4]',
  })
})

test('p=0.7 — reply command fully typed, its caret lingering before send, beam idle between legs', () => {
  assert.deepEqual(journeyState(0.7, texts), {
    ask: { text: ASK, caretOn: false },
    sendCallOn: true,
    receiptOn: true,
    envHeadOn: true,
    envTailOn: true,
    peerIdleOpacity: 0.35,
    work1On: true,
    work2On: true,
    cmd: { text: CMD, caretOn: true },
    replyOn: false,
    beamOn: false,
    packet: null,
    peerActive: true,
    youFaded: true,
    youActive: false,
    beat: '[3/4]',
  })
})

test('p=0.9 — reply landed back in your pane, session complete', () => {
  assert.deepEqual(journeyState(0.9, texts), {
    ask: { text: ASK, caretOn: false },
    sendCallOn: true,
    receiptOn: true,
    envHeadOn: true,
    envTailOn: true,
    peerIdleOpacity: 0.35,
    work1On: true,
    work2On: true,
    cmd: { text: CMD, caretOn: false },
    replyOn: true,
    beamOn: false,
    packet: null,
    peerActive: false,
    youFaded: false,
    youActive: true,
    beat: '[4/4]',
  })
})

test('p=0 — the very start of the scrub: only the ask caret blinks', () => {
  assert.deepEqual(journeyState(0, texts), {
    ask: { text: '', caretOn: true },
    sendCallOn: false,
    receiptOn: false,
    envHeadOn: false,
    envTailOn: false,
    peerIdleOpacity: 1,
    work1On: false,
    work2On: false,
    cmd: { text: '', caretOn: false },
    replyOn: false,
    beamOn: false,
    packet: null,
    peerActive: false,
    youFaded: false,
    youActive: false,
    beat: '[1/4]',
  })
})

test('p=1 — past the reply landing, peer focus has released back to you', () => {
  assert.deepEqual(journeyState(1, texts), {
    ask: { text: ASK, caretOn: false },
    sendCallOn: true,
    receiptOn: true,
    envHeadOn: true,
    envTailOn: true,
    peerIdleOpacity: 0.35,
    work1On: true,
    work2On: true,
    cmd: { text: CMD, caretOn: false },
    replyOn: true,
    beamOn: false,
    packet: null,
    peerActive: false,
    youFaded: false,
    youActive: true,
    beat: '[4/4]',
  })
})

test('the return leg mirrors its t and reports the return packet text', () => {
  const state = journeyState(0.78, texts)
  assert.equal(state.beamOn, true)
  assert.equal(state.packet.text, RETURN_PACKET_TEXT)
  assert.ok(Math.abs(state.packet.t - 0.5) < 1e-9, `expected t≈0.5, got ${state.packet.t}`)
  assert.equal(state.beat, '[4/4]')
})

test('journeyState is total: NaN/out-of-range progress clamps instead of throwing', () => {
  const zero = journeyState(0, texts)
  const one = journeyState(1, texts)
  assert.deepEqual(journeyState(Number.NaN, texts), zero)
  assert.deepEqual(journeyState(-5, texts), zero)
  assert.deepEqual(journeyState(5, texts), one)
})

test('the beam is never reported travelling on either endpoint of a leg (strictly between 0 and 1)', () => {
  // Outbound leg is seg(p, 0.24, 0.38); its own endpoints must not travel.
  assert.equal(journeyState(0.24, texts).beamOn, false)
  assert.equal(journeyState(0.38, texts).beamOn, false)
  // Return leg is seg(p, 0.72, 0.84); same guarantee.
  assert.equal(journeyState(0.72, texts).beamOn, false)
  assert.equal(journeyState(0.84, texts).beamOn, false)
})
