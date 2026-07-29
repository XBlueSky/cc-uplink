import { test } from 'node:test'
import assert from 'node:assert/strict'

import { initTimeline, prefersReducedMotion } from '../src/scripts/timeline.mjs'

test('prefersReducedMotion reflects the media query it is given', () => {
  assert.equal(prefersReducedMotion({ matches: true }), true)
  assert.equal(prefersReducedMotion({ matches: false }), false)
})

test('initTimeline under reduced motion resolves a no-op handle, touches nothing on root, and paints progress 0 exactly once', async (t) => {
  const calls = []
  const onProgress = (p) => calls.push(p)
  const root = {}

  const handle = await initTimeline({ root, onProgress, mq: { matches: true } })

  assert.equal(typeof handle.destroy, 'function')
  assert.deepEqual(calls, [0], 'onProgress must be called exactly once, with 0')
  assert.deepEqual(root, {}, 'reduced motion must not touch root at all — no gsap/lenis import')

  // Must be safe to call, and inert — no trigger/lenis was ever created.
  assert.doesNotThrow(() => handle.destroy())
})
