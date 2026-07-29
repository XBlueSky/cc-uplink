/** Body text colour; must match --fg in tokens.css. */
export const FG = '#e6e9f0'

/** sRGB channel (0..255) to linear light, per WCAG 2.x. */
export function srgbToLinear(channel) {
  const s = channel / 255
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
}

export function relativeLuminance(hex) {
  const n = Number.parseInt(hex.replace('#', ''), 16)
  const r = (n >> 16) & 0xff
  const g = (n >> 8) & 0xff
  const b = n & 0xff
  return 0.2126 * srgbToLinear(r) + 0.7152 * srgbToLinear(g) + 0.0722 * srgbToLinear(b)
}

export function contrastRatio(l1, l2) {
  const hi = Math.max(l1, l2)
  const lo = Math.min(l1, l2)
  return (hi + 0.05) / (lo + 0.05)
}

/**
 * The highest background relative luminance that still holds `ratio`
 * against `fgHex`. The ambience shader clamps its output to this.
 */
export function maxBackgroundLuminance(fgHex = FG, ratio = 4.5) {
  return (relativeLuminance(fgHex) + 0.05) / ratio - 0.05
}

export const LUM_CEILING = maxBackgroundLuminance()
