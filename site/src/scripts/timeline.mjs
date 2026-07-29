import gsap from 'gsap'
import { ScrollTrigger } from 'gsap/ScrollTrigger'
import Lenis from 'lenis'

import { actAt, clamp01, localProgress } from '../lib/actState.mjs'

gsap.registerPlugin(ScrollTrigger)

export function prefersReducedMotion(mq = window.matchMedia('(prefers-reduced-motion: reduce)')) {
  return mq.matches
}

/**
 * Pin the act stack and scrub one master timeline across it.
 *
 * Returns a no-op handle under reduced motion: the static layout the
 * foundation plan built is already the correct experience, so nothing is
 * initialised at all — no pin, no Lenis, no rAF loop.
 *
 * @param {{root: HTMLElement, onProgress?: (p: number) => void}} options
 */
export function initTimeline({ root, onProgress = () => {} }) {
  if (prefersReducedMotion()) {
    onProgress(1)
    return { destroy() {} }
  }

  const sections = [...root.querySelectorAll('[data-act]')]
  if (sections.length === 0) return { destroy() {} }

  const lenis = new Lenis({ autoRaf: false })

  // One rAF loop drives Lenis, and Lenis drives ScrollTrigger. Two
  // independent loops would fight over the same scroll position.
  lenis.on('scroll', ScrollTrigger.update)
  const tick = (time) => lenis.raf(time * 1000)
  gsap.ticker.add(tick)
  gsap.ticker.lagSmoothing(0)

  // Only transform and opacity are animated: nothing here can trigger
  // layout, so the pin contributes no CLS.
  const trigger = ScrollTrigger.create({
    trigger: root,
    start: 'top top',
    end: () => `+=${sections.length * 100}%`,
    pin: true,
    pinSpacing: true,
    scrub: true,
    anticipatePin: 1,
    invalidateOnRefresh: true,
    onUpdate: (self) => {
      const progress = clamp01(self.progress)
      const act = actAt(progress)
      const local = localProgress(progress)

      root.dataset.currentAct = String(act.id)

      for (const section of sections) {
        const id = Number(section.dataset.act)
        const distance = id - act.id
        const enter = id === act.id ? local : 0

        gsap.set(section, {
          opacity: distance === 0 ? 1 : 0,
          yPercent: distance === 0 ? 0 : distance > 0 ? 6 : -6,
          pointerEvents: distance === 0 ? 'auto' : 'none',
        })

        if (distance === 0) section.dataset.enter = enter.toFixed(3)
      }

      onProgress(progress)
    },
  })

  return {
    destroy() {
      trigger.kill()
      gsap.ticker.remove(tick)
      lenis.destroy()
    },
  }
}
