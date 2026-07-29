/**
 * Scroll ranges for the six acts, from the design spec. They must tile
 * [0, 1] exactly: the timeline maps global scroll progress through these.
 */
export const ACTS = [
  { id: 0, start: 0.0, end: 0.12 },
  { id: 1, start: 0.12, end: 0.28 },
  { id: 2, start: 0.28, end: 0.48 },
  { id: 3, start: 0.48, end: 0.62 },
  { id: 4, start: 0.62, end: 0.8 },
  { id: 5, start: 0.8, end: 1.0 },
]

export function clamp01(n) {
  if (Number.isNaN(n)) return 0
  return n < 0 ? 0 : n > 1 ? 1 : n
}

/** @returns {{id: number, start: number, end: number}} */
export function actAt(progress) {
  const p = clamp01(progress)
  for (const act of ACTS) {
    if (p < act.end) return act
  }
  return ACTS[ACTS.length - 1]
}

/** Progress within the current act, 0..1. */
export function localProgress(progress) {
  const p = clamp01(progress)
  const act = actAt(p)
  return clamp01((p - act.start) / (act.end - act.start))
}

/**
 * The visible prefix of `text` at `progress`.
 *
 * A pure function of progress rather than a timer, so scrubbing backwards
 * un-types instead of leaving stale characters on screen.
 */
export function typedSlice(text, progress) {
  return text.slice(0, Math.round(clamp01(progress) * text.length))
}
