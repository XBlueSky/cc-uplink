import { clamp01, seg, typedSlice } from './scrub.mjs'

/**
 * §3 invoke-demo thresholds, ported verbatim from the storyboard's
 * renderDemo()/demoProgress() (docs/superpowers/design/2026-07-30-storyboard.html
 * §3 — the canonical behaviour reference). Same "named constants, not bare
 * numbers" convention as journeyState.mjs.
 *
 *   0.02–0.40 ask line types out   0.44 its caret stops blinking
 *   0.48      tool call line lands 0.62 the "codex exec" line lands
 *   0.74      the paths line lands 0.86 the generated artifact fades in
 */
const ASK_TYPE_START = 0.02
const ASK_TYPE_END = 0.4
const ASK_CARET_END = 0.44
const CALL_ON = 0.48
const MID_ON = 0.62
const PATHS_ON = 0.74
const ART_ON = 0.86

// The demo section isn't pinned — it's a normal-flow block whose own
// position in the viewport stands in for a scrub timeline (the storyboard's
// "view-timeline mode": scroll it into view, the block plays; scroll back
// past it, the block rewinds). These two thresholds carve the *middle*
// slice of that traversal out as the actual playback window, so the demo
// finishes typing well before the block scrolls out of view entirely.
const PROGRESS_START = 0.12
const PROGRESS_END = 0.48

/**
 * Pure progress -> state mapping for the §3 invoke-demo scrub.
 *
 * @param {number} t - demo playback progress, 0..1 (anything else clamps via
 *   `seg`/`typedSlice`'s own totality — this function is total, never throws).
 * @param {{askText: string}} texts - the demo's one typed line's full text,
 *   read by the caller from its own `data-text` attribute at runtime — same
 *   "markup is the single source of truth" rule as journeyState.
 *
 * No DOM access happens in this function — see journeyState.mjs's doc
 * comment for why that's what makes it unit testable without a browser
 * (site/test/demo-state.test.mjs) and keeps element measurement (this
 * module reports a fraction, not a pixel) in the DOM-driving caller,
 * site/src/scripts/enhance.mjs.
 */
export function demoState(t, { askText }) {
  // Clamped once, up front, exactly like journeyState's own `progress` —
  // `seg()`/`typedSlice()` already clamp internally, but the direct `<`/`>=`
  // comparisons below don't go through either helper, so an unclamped NaN
  // would otherwise compare `false` against every threshold (`NaN < x` and
  // `NaN >= x` are both always `false`), silently breaking totality at t=0
  // instead of degrading to the t=0 state like every other out-of-range input.
  const progress = clamp01(t)
  const askT = seg(progress, ASK_TYPE_START, ASK_TYPE_END)

  return {
    ask: {
      text: typedSlice(askText, askT),
      caretOn: progress < ASK_CARET_END,
    },
    callOn: progress >= CALL_ON,
    midOn: progress >= MID_ON,
    pathsOn: progress >= PATHS_ON,
    artOn: progress >= ART_ON,
  }
}

/**
 * Maps a demo block's own viewport geometry to a 0..1 playback progress:
 * 0 while its top edge is still below the viewport (not yet visible), 1
 * once its bottom edge has scrolled fully past the top of the viewport
 * (long gone) — then re-scaled so the actual typing/reveal only plays out
 * across the `PROGRESS_START..PROGRESS_END` middle slice of that span (see
 * above). Pure geometry in, fraction out — same DOM-free contract as
 * `demoState`; the caller measures the real element with
 * `getBoundingClientRect()` and passes plain numbers.
 *
 * @param {{top: number, height: number, viewportHeight: number}} rect
 */
export function demoProgress({ top, height, viewportHeight }) {
  return seg((viewportHeight - top) / (viewportHeight + height), PROGRESS_START, PROGRESS_END)
}
