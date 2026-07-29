import { actAt, clamp01, localProgress, typedSlice } from '../lib/actState.mjs'

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

  /**
   * Per-act internal motion. Every value is derived from `enter` (0..1
   * within the act), never from elapsed time, so scrubbing backwards is
   * exact. Defined here, inside `initTimeline`, so it can close over the
   * `gsap` binding that only exists after the dynamic import above resolves.
   */
  function paintAct(section, id, enter) {
    if (id === 1) {
      const target = section.querySelector('[data-split-target]')
      if (target) {
        gsap.set(target, {
          scaleX: 0.2 + 0.8 * enter,
          opacity: enter,
        })
      }
    }

    if (id === 2) {
      const typed = section.querySelector('[data-typewriter]')
      if (typed) {
        const full = typed.dataset.text ?? ''
        typed.textContent = typedSlice(full, clamp01(enter / 0.75))
      }
      const signal = section.querySelector('[data-signal]')
      if (signal) {
        gsap.set(signal, {
          xPercent: enter * 100,
          opacity: enter > 0.02 && enter < 0.9 ? 1 : 0,
        })
      }
    }

    if (id === 3) {
      const reply = section.querySelector('[data-reply]')
      if (reply) {
        gsap.set(reply, {
          opacity: clamp01((enter - 0.25) / 0.35),
          y: (1 - clamp01((enter - 0.25) / 0.35)) * 8,
        })
      }
    }
  }

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
      // 1 for acts already scrolled past, 0 for acts not yet reached — so
      // nothing keeps a stale reading from an earlier scrub. `paintAct`
      // receives exactly this same effective value for every section (not
      // just the current act), so a passed act's typewriter holds the full
      // text and a future act's holds empty — no boundary crossing ever
      // shows a stale state.
      const effectiveEnter = distance === 0 ? localProgress(progress) : distance < 0 ? 1 : 0
      section.dataset.enter = effectiveEnter.toFixed(3)
      paintAct(section, id, effectiveEnter)
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
