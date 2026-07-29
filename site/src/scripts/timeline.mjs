import { actAt, clamp01, localProgress } from '../lib/actState.mjs'

export function prefersReducedMotion(mq = window.matchMedia('(prefers-reduced-motion: reduce)')) {
  return mq.matches
}

/**
 * Pin the act stack and scrub one master timeline across it.
 *
 * Resolves to a no-op handle under reduced motion, or when there is nothing
 * to pin: the static layout the foundation plan built is already the
 * correct experience, so nothing is initialised at all — no pin, no Lenis,
 * no rAF loop. `gsap`, `gsap/ScrollTrigger` and `lenis` are only ever
 * imported dynamically, and only past the reduced-motion check: importing
 * `gsap/ScrollTrigger` self-registers a permanent rAF loop, a 250ms
 * interval, and global wheel/scroll/pointer listeners the instant the
 * module evaluates — a static import at the top of this file would run
 * those side effects for reduced-motion users too, defeating the whole
 * point of the early return.
 *
 * Every path resolves calling `onProgress` exactly once with `0`, so a
 * caller wiring in a shader uniform (Task 5) always sees a consistent first
 * frame no matter which path returned.
 *
 * @param {{root: HTMLElement, onProgress?: (p: number) => void, mq?: MediaQueryList}} options
 * @returns {Promise<{destroy(): void}>}
 */
export async function initTimeline({ root, onProgress = () => {}, mq } = {}) {
  if (prefersReducedMotion(mq)) {
    onProgress(0)
    return { destroy() {} }
  }

  const sections = [...root.querySelectorAll('[data-act]')]
  if (sections.length === 0) {
    onProgress(0)
    return { destroy() {} }
  }

  const [{ default: gsap }, { ScrollTrigger }, { default: Lenis }] = await Promise.all([
    import('gsap'),
    import('gsap/ScrollTrigger'),
    import('lenis'),
  ])

  gsap.registerPlugin(ScrollTrigger)

  // Stacking only makes sense once this script can also drive the
  // un-stacking, so the `.motion` class — and the pinning it implies — are
  // both added here, together, rather than the class living unconditionally
  // in CSS. `applyProgress(0)` right after `ScrollTrigger.create` below
  // paints the first frame in the same tick the class lands, so acts 1-5
  // are never overprinted, not even for a single frame.
  root.classList.add('motion')

  const lenis = new Lenis({ autoRaf: false })

  // One rAF loop drives Lenis, and Lenis drives ScrollTrigger. Two
  // independent loops would fight over the same scroll position.
  lenis.on('scroll', ScrollTrigger.update)
  const tick = (time) => lenis.raf(time * 1000)
  gsap.ticker.add(tick)
  gsap.ticker.lagSmoothing(0)

  function applyProgress(rawProgress) {
    const progress = clamp01(rawProgress)
    const act = actAt(progress)

    root.dataset.currentAct = String(act.id)

    for (const section of sections) {
      const id = Number(section.dataset.act)
      const distance = id - act.id

      // autoAlpha (not opacity) so gsap also toggles visibility: hidden
      // acts drop out of tab order and the accessibility tree instead of
      // sitting invisible-but-focusable. Still transform/opacity only —
      // nothing here can trigger layout, so the pin contributes no CLS.
      gsap.set(section, {
        autoAlpha: distance === 0 ? 1 : 0,
        yPercent: distance === 0 ? 0 : distance > 0 ? 6 : -6,
        pointerEvents: distance === 0 ? 'auto' : 'none',
      })

      // Every section gets a live value: the current act's local progress,
      // '1.000' for acts already scrolled past, '0.000' for acts not yet
      // reached — so nothing keeps a stale reading from an earlier scrub.
      if (distance === 0) section.dataset.enter = localProgress(progress).toFixed(3)
      else if (distance < 0) section.dataset.enter = '1.000'
      else section.dataset.enter = '0.000'
    }

    onProgress(progress)
  }

  const trigger = ScrollTrigger.create({
    trigger: root,
    start: 'top top',
    end: () => `+=${sections.length * 100}%`,
    pin: true,
    pinSpacing: true,
    scrub: true,
    anticipatePin: 1,
    invalidateOnRefresh: true,
    onUpdate: (self) => applyProgress(self.progress),
  })

  // ScrollTrigger does not invoke onUpdate at create time (progress 0
  // equals its own initial prevProgress, so the internal guard skips it),
  // so the first paint has to be forced explicitly here — otherwise all six
  // acts stay overprinted at full opacity until the user's first scroll.
  applyProgress(0)

  return {
    destroy() {
      trigger.kill()
      gsap.ticker.remove(tick)
      lenis.destroy()
      root.classList.remove('motion')
      gsap.set(sections, { clearProps: 'all' })
      gsap.ticker.lagSmoothing(500, 33)
    },
  }
}
