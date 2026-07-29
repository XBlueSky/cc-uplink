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
 * Every path resolves calling `onProgress` exactly once at init: `0` on the
 * reduced-motion and no-sections paths, or the trigger's (possibly
 * browser-restored) scroll progress on the motion path — so a caller wiring
 * in a shader uniform (Task 5) always sees a consistent first frame no
 * matter which path returned, including after a mid-scroll reload.
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

  // The signal's travel distance in pixels, cached lazily and re-measured
  // once per ScrollTrigger refresh (see `onRefresh` below) rather than on
  // every frame — `paintAct` runs inside the scrub's rAF loop, so a layout
  // read there would thrash. `0` doubles as "not measured yet"; the signal
  // pane is never actually 2px wide, so this can't be mistaken for a real
  // measurement.
  let signalSpan = 0

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
        const next = typedSlice(full, clamp01(enter / 0.75))
        // Skip the write when the slice hasn't changed: an unconditional
        // `textContent` assignment tears down and recreates the text node
        // every tick, dirtying the <pre> subtree's layout 60x/s even while
        // the act is fully hidden (autoAlpha's visibility:hidden does not
        // elide layout work).
        if (typed.textContent !== next) typed.textContent = next
      }
      const signal = section.querySelector('[data-signal]')
      if (signal) {
        // `xPercent` resolves against the signal's own (2px) border box, so
        // it would only ever travel 2px regardless of the pane's width. A
        // pixel `x` transform against the pane's actual width is what makes
        // it cross the pane.
        if (signalSpan === 0 && signal.parentElement) {
          signalSpan = signal.parentElement.clientWidth - 2
        }
        gsap.set(signal, {
          x: enter * signalSpan,
          opacity: enter > 0.02 && enter < 0.9 ? 1 : 0,
        })
      }
    }

    if (id === 3) {
      const reply = section.querySelector('[data-reply]')
      if (reply) {
        const t = clamp01((enter - 0.25) / 0.35)
        gsap.set(reply, {
          opacity: t,
          y: (1 - t) * 8,
        })
      }
    }
  }

  // Stacking only makes sense once this script can also drive the
  // un-stacking, so the `.motion` class — and the pinning it implies — are
  // both added here, together, rather than the class living unconditionally
  // in CSS. The explicit `applyProgress` call right after
  // `ScrollTrigger.create` below paints the first frame in the same tick
  // the class lands, so acts 1-5 are never overprinted, not even for a
  // single frame.
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
    // Invalidate the cached signal span on every refresh (e.g. a resize),
    // not on every frame — the same once-per-refresh cadence
    // `invalidateOnRefresh` already uses for ScrollTrigger's own geometry.
    onRefresh: () => {
      signalSpan = 0
    },
  })

  // ScrollTrigger does not invoke onUpdate at create time (progress 0
  // equals its own initial prevProgress, so the internal guard skips it),
  // so the first paint has to be forced explicitly here. Using
  // `trigger.progress` rather than a hardcoded 0 matters because the first
  // paint has to honour whatever progress the trigger reports at init —
  // ScrollTrigger's own initial refresh computes it from wherever the page
  // actually is (normally 0, since page load starts there). This is not
  // compensating for a restored scroll position: scroll restoration itself
  // is disabled at the page level (see index.astro's script), precisely
  // because the pre-.motion document is a different height than the
  // pinned one, so a restored offset would map to the wrong act here.
  applyProgress(clamp01(trigger.progress))

  return {
    destroy() {
      trigger.kill()
      gsap.ticker.remove(tick)
      lenis.destroy()
      root.classList.remove('motion')
      gsap.set(sections, { clearProps: 'all' })
      // `clearProps` on `sections` only reaches the six act elements
      // themselves — it does not touch the descendants `paintAct` wrote to
      // directly, which would otherwise stay frozen wherever the scrub last
      // left them (the split pane at scaleX 0.2/opacity 0, the reply
      // hidden, the signal mid-flight).
      gsap.set(root.querySelectorAll('[data-split-target], [data-signal], [data-reply]'), {
        clearProps: 'all',
      })
      const typed = root.querySelector('[data-typewriter]')
      if (typed) typed.textContent = typed.dataset.text ?? ''
      gsap.ticker.lagSmoothing(500, 33)
    },
  }
}
