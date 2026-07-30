/** Clamp to [0,1]; any non-finite input (undefined, NaN, ±Infinity) → 0. */
export function clamp01(n) {
  if (!Number.isFinite(n)) return 0
  return Math.min(1, Math.max(0, n))
}

/** Progress of p across the sub-range [a,b], clamped to [0,1]. */
export function seg(p, a, b) {
  return clamp01((p - a) / (b - a))
}

/** The first round(t * text.length) characters — a scrub-reversible typewriter. */
export function typedSlice(text, t) {
  return text.slice(0, Math.round(clamp01(t) * text.length))
}
