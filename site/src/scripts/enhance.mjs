import { clamp01 } from '../lib/scrub.mjs'
import { journeyState } from '../lib/journeyState.mjs'
import { demoState, demoProgress } from '../lib/demoState.mjs'

/**
 * Gate: flat visitors (reduced-motion or a viewport under 760px, same
 * breakpoint as Rail.astro's own media query) get the markup exactly as
 * shipped and nothing else — it's already the complete experience. This
 * module's job past the gate is every motion behaviour on the page: the §2
 * signal journey (the one pinned scene), the §3 invoke-demo scrub, the §4–§6
 * one-time reveals, the rail scrollspy, and the hero idle loop.
 *
 * `enhanced` is added to <html> synchronously, before first paint (same
 * CLS-0 technique as the deck's `.motion` gate) — every enhanced-only CSS
 * rule in the section components hangs off `.enhanced` for exactly this
 * reason: a flat visitor never gets the class, so the "lines/reveals start
 * hidden" rules never apply to them and there is no flash of hidden text.
 *
 * The copy buttons are the one exception: they're not motion, so they're
 * wired unconditionally, before the gate check below (see initCopyButtons).
 */
export function initEnhance() {
  initCopyButtons()

  // Gate is evaluated once, here, at load — not re-checked on resize or on
  // a live `matchMedia` listener. Deliberate: the storyboard's own gate is
  // load-time-only too, and a visitor who resizes across the 760px
  // breakpoint or flips reduced-motion mid-session while sitting on a
  // 460vh pinned track is a teardown problem (unpin the scroll, replay or
  // discard in-flight typed state, rebind listeners) this scene doesn't
  // warrant solving. Don't read this as an oversight.
  const flat = matchMedia('(prefers-reduced-motion: reduce)').matches || innerWidth < 760
  if (flat) return

  document.documentElement.classList.add('enhanced')

  // §2 and §3 are both scroll-position-driven scrubs; they share ONE
  // passive `scroll` listener with a single rAF-guarded tick rather than
  // each wiring their own (see the bottom of this function) — `initJourney`
  // and `initDemo` only measure the DOM and return a zero-argument "render
  // this frame" callback, they never touch `addEventListener` themselves.
  const ticks = [initJourney(), initDemo()].filter(Boolean)

  initReveals()
  initRailSpy()
  initHeroLoop()

  if (!ticks.length) return
  let ticking = false
  const onScroll = () => {
    if (ticking) return
    ticking = true
    requestAnimationFrame(() => {
      ticking = false
      for (const tick of ticks) tick()
    })
  }
  addEventListener('scroll', onScroll, { passive: true })
  addEventListener('resize', onScroll)
  onScroll()
}

/**
 * Wires the §2 pinned scroll scrub. All choreography math lives in the pure
 * `journeyState()` (site/src/lib/journeyState.mjs, unit tested without a
 * browser); this function only measures the DOM, reads the two typed
 * lines' full text from their own `data-text` attributes (the markup is
 * the single source of truth — never hardcode the copy here), and writes
 * `journeyState()`'s output back into the DOM, deduping every write
 * against the previous frame's value since this runs on every scroll frame.
 *
 * Returns a zero-argument tick function that measures the track's current
 * scroll position and renders one frame, or `null` if the markup contract
 * isn't met — `initEnhance` composes this (and `initDemo`'s own tick) into
 * the page's one shared scroll listener rather than this function owning
 * a listener itself.
 */
function initJourney() {
  const section = document.querySelector('[data-journey]')
  if (!section) return null

  const track = section.querySelector('[data-track]')
  const beatEl = section.querySelector('[data-beat]')
  const beamEl = section.querySelector('[data-beam]')
  const packetEl = section.querySelector('[data-packet]')

  const paneYouWrap = section.querySelector('[data-pane-you]')
  const panePeerWrap = section.querySelector('[data-pane-peer]')
  const paneYouEl = paneYouWrap?.querySelector('[data-pane]')
  const panePeerEl = panePeerWrap?.querySelector('[data-pane]')

  const youPre = paneYouWrap?.querySelector('pre')
  const peerPre = panePeerWrap?.querySelector('pre')

  const askEl = youPre?.querySelector('[data-typed]')
  const askCaretEl = youPre?.querySelector('[data-caret]')
  const [callEl, receiptEl, replyEl] = youPre
    ? youPre.querySelectorAll('[data-line]:not([data-typed])')
    : []

  const peerIdleEl = peerPre?.querySelector('.dim')
  const cmdEl = peerPre?.querySelector('[data-typed]')
  const cmdCaretEl = peerPre?.querySelector('[data-caret]')
  const [envHeadEl, envTailEl, work1El, work2El] = peerPre
    ? peerPre.querySelectorAll('[data-line]:not([data-typed])')
    : []

  const required = [
    track, beatEl, beamEl, packetEl, paneYouEl, panePeerEl,
    askEl, askCaretEl, callEl, receiptEl, replyEl,
    peerIdleEl, cmdEl, cmdCaretEl, envHeadEl, envTailEl, work1El, work2El,
  ]
  // The markup contract not being met (a future edit dropping a hook) should
  // silently no-op rather than throw mid-scroll and take the rest of the
  // page's JS down with it — the flat markup underneath is still correct.
  if (required.some((el) => !el)) return null

  // a11y: the pinned scene's letter-by-letter churn is described once, up
  // front, by the section's sr-only transcript (see Journey.astro) — hide
  // the replay from assistive tech, and drop the panes' own tabindex so a
  // hidden region can't still catch keyboard focus.
  youPre.setAttribute('aria-hidden', 'true')
  peerPre.setAttribute('aria-hidden', 'true')
  youPre.tabIndex = -1
  peerPre.tabIndex = -1

  const askText = askEl.dataset.text ?? ''
  const cmdText = cmdEl.dataset.text ?? ''
  askEl.textContent = ''
  cmdEl.textContent = ''

  const last = {
    askText: '', askCaret: null,
    call: null, receipt: null, envHead: null, envTail: null, work1: null, work2: null, reply: null,
    peerIdleOpacity: null,
    cmdText: '', cmdCaret: null,
    beamOn: null, packetText: null, packetLeft: null,
    peerActive: null, youFaded: null, youActive: null,
    beat: null,
  }

  const setOn = (el, key, on) => {
    if (last[key] === on) return
    last[key] = on
    el.classList.toggle('on', on)
  }

  function render(p) {
    const state = journeyState(p, { askText, cmdText })

    if (last.askText !== state.ask.text) {
      last.askText = state.ask.text
      askEl.textContent = state.ask.text
    }
    if (last.askCaret !== state.ask.caretOn) {
      last.askCaret = state.ask.caretOn
      askCaretEl.style.display = state.ask.caretOn ? 'inline-block' : 'none'
    }

    setOn(callEl, 'call', state.sendCallOn)
    setOn(receiptEl, 'receipt', state.receiptOn)
    setOn(envHeadEl, 'envHead', state.envHeadOn)
    setOn(envTailEl, 'envTail', state.envTailOn)
    setOn(work1El, 'work1', state.work1On)
    setOn(work2El, 'work2', state.work2On)
    setOn(replyEl, 'reply', state.replyOn)

    if (last.peerIdleOpacity !== state.peerIdleOpacity) {
      last.peerIdleOpacity = state.peerIdleOpacity
      peerIdleEl.style.opacity = String(state.peerIdleOpacity)
    }

    if (last.cmdText !== state.cmd.text) {
      last.cmdText = state.cmd.text
      cmdEl.textContent = state.cmd.text
    }
    if (last.cmdCaret !== state.cmd.caretOn) {
      last.cmdCaret = state.cmd.caretOn
      cmdCaretEl.style.display = state.cmd.caretOn ? 'inline-block' : 'none'
    }

    if (last.beamOn !== state.beamOn) {
      last.beamOn = state.beamOn
      const opacity = state.beamOn ? '1' : '0'
      beamEl.style.opacity = opacity
      packetEl.style.opacity = opacity
    }
    if (state.packet) {
      if (last.packetText !== state.packet.text) {
        last.packetText = state.packet.text
        packetEl.textContent = state.packet.text
      }
      const left = state.packet.t * (beamEl.clientWidth - packetEl.clientWidth)
      if (last.packetLeft !== left) {
        last.packetLeft = left
        packetEl.style.left = `${left}px`
      }
    } else {
      last.packetText = null
      last.packetLeft = null
    }

    if (last.peerActive !== state.peerActive) {
      last.peerActive = state.peerActive
      panePeerEl.dataset.active = state.peerActive ? 'true' : 'false'
    }
    if (last.youFaded !== state.youFaded) {
      last.youFaded = state.youFaded
      // Toggled on the wrapper THIS page owns ([data-pane-you]), not on
      // paneYouEl (Pane.astro's own [data-pane] root) — see Journey.astro's
      // comment above its enhanced-mode CSS block for why a Journey-scoped
      // selector can never match Pane's own element, :global() or not.
      paneYouWrap.classList.toggle('faded', state.youFaded)
    }
    if (last.youActive !== state.youActive) {
      last.youActive = state.youActive
      paneYouEl.dataset.active = state.youActive ? 'true' : 'false'
    }

    if (last.beat !== state.beat) {
      last.beat = state.beat
      beatEl.textContent = state.beat
    }
  }

  return () => {
    const total = track.offsetHeight - innerHeight
    render(clamp01(-track.getBoundingClientRect().top / total))
  }
}

/**
 * Wires the §3 invoke-demo view-position scrub. Same shape as `initJourney`
 * above — the pure math lives in `demoState()`/`demoProgress()`
 * (site/src/lib/demoState.mjs, unit tested without a browser), this
 * function only measures the DOM and writes deduped output back into it —
 * but this scene isn't pinned: `demoProgress()` reports playback progress
 * from the demo block's own position in the viewport (the storyboard's
 * "view-timeline mode"), so scrolling past it plays forward and scrolling
 * back rewinds it, same as any other frame of this tick.
 *
 * Returns a zero-argument tick function, or `null` if the markup contract
 * isn't met (see `initJourney`'s doc comment for why that's a silent no-op
 * rather than a throw).
 */
function initDemo() {
  const section = document.querySelector('[data-demo]')
  if (!section) return null

  // The block whose own viewport position drives playback — the pane +
  // artifact, NOT the whole section (which also includes the `.copy` intro
  // paragraph beside it). Matches the storyboard's own `#invoke-demo`,
  // which wrapped only the pane and the artifact figure. Measuring the
  // wider section here would make playback depend on the intro copy's
  // height too (and on whichever grid column happens to be taller when the
  // two-column layout collapses to one), which isn't what this scrub is
  // supposed to track.
  const demoBox = section.querySelector('.demo')

  const askEl = section.querySelector('[data-typed]')
  const caretEl = section.querySelector('[data-caret]')
  const [callEl, midEl, pathsEl] = section.querySelectorAll('[data-line]:not([data-typed])')
  const artEl = section.querySelector('[data-art]')

  const required = [demoBox, askEl, caretEl, callEl, midEl, pathsEl, artEl]
  if (required.some((el) => !el)) return null

  const askText = askEl.dataset.text ?? ''
  askEl.textContent = ''

  const last = { askText: '', caret: null, call: null, mid: null, paths: null, art: null }

  const setOn = (el, key, on) => {
    if (last[key] === on) return
    last[key] = on
    el.classList.toggle('on', on)
  }

  function render(t) {
    const state = demoState(t, { askText })

    if (last.askText !== state.ask.text) {
      last.askText = state.ask.text
      askEl.textContent = state.ask.text
    }
    if (last.caret !== state.ask.caretOn) {
      last.caret = state.ask.caretOn
      caretEl.style.display = state.ask.caretOn ? 'inline-block' : 'none'
    }

    setOn(callEl, 'call', state.callOn)
    setOn(midEl, 'mid', state.midOn)
    setOn(pathsEl, 'paths', state.pathsOn)
    setOn(artEl, 'art', state.artOn)
  }

  return () => {
    const r = demoBox.getBoundingClientRect()
    render(demoProgress({ top: r.top, height: r.height, viewportHeight: innerHeight }))
  }
}

/**
 * §3–§6 one-time reveals: fade/rise in once each `[data-reveal]` target
 * crosses 25% visible, then stop watching it — the storyboard's own
 * `.reveal` IntersectionObserver, verbatim (threshold + unobserve-after-fire).
 * The CSS this class toggle drives (`opacity`/`translate` → `.in`) lives in
 * each section component under its own `.enhanced` scope, not here.
 */
function initReveals() {
  const targets = document.querySelectorAll('[data-reveal]')
  if (!targets.length) return

  const io = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue
        entry.target.classList.add('in')
        io.unobserve(entry.target)
      }
    },
    { threshold: 0.25 },
  )
  targets.forEach((el) => io.observe(el))
}

/**
 * Rail scrollspy: toggles `.on` on the `a[data-spy]` tick whose section is
 * currently in the vertically-centred band of the viewport. `rootMargin`
 * shrinks the observer's root by 45% on both the top and bottom, so
 * "intersecting" only fires once a section occupies that centre band — the
 * storyboard's own logic, verbatim. The rail's `<a>`s are plain in-page
 * anchors already (see Rail.astro), so navigation keeps working with this
 * script absent entirely; this only adds the highlight.
 */
function initRailSpy() {
  const rail = document.querySelector('[data-rail]')
  if (!rail) return

  const links = [...rail.querySelectorAll('a[data-spy]')]
  const sections = links
    .map((a) => document.getElementById(a.dataset.spy))
    .filter(Boolean)
  if (!links.length || sections.length !== links.length) return

  const spy = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue
        for (const link of links) {
          link.classList.toggle('on', link.dataset.spy === entry.target.id)
        }
      }
    },
    { rootMargin: '-45% 0px -45% 0px' },
  )
  sections.forEach((sec) => spy.observe(sec))
}

// Hero idle loop copy, ported verbatim from the storyboard — three lines
// cycling with a hold between each, time-driven (not scroll-driven).
const HERO_LINES = ['> waiting for peers…', '> 2 channels live', '> ask codex to review my diff']
const HERO_TICK_MS = 90
const HERO_HOLD_TICKS = 14

/**
 * Hero idle loop: types each of `HERO_LINES` out one character per tick,
 * holds for `HERO_HOLD_TICKS` ticks once fully typed, then blanks and moves
 * to the next line. Time-driven (`setInterval`), not part of the shared
 * scroll tick above — the storyboard's own loop is scroll-independent too.
 * Only runs past the flat gate, so a reduced-motion/narrow visitor never
 * gets this `setInterval` at all (the flat markup shows nothing here by
 * design — see Hero.astro/task-2-brief: the span ships empty, its idle
 * loop is additive, not content).
 */
function initHeroLoop() {
  const lineEl = document.querySelector('[data-hero-line]')
  if (!lineEl) return

  // a11y: this line churns character-by-character on a timer, purely
  // decorative — hide it from assistive tech without hiding the pane's
  // static `$ claude` prompt above it. The caret span beside it is already
  // `aria-hidden` unconditionally in the markup (see Hero.astro); this is
  // the dynamic half of the same treatment, applied only once the loop is
  // actually about to start driving the line's text.
  lineEl.setAttribute('aria-hidden', 'true')

  let lineIndex = 0
  let charCount = 0
  setInterval(() => {
    const line = HERO_LINES[lineIndex]
    charCount++
    if (charCount > line.length + HERO_HOLD_TICKS) {
      charCount = 0
      lineIndex = (lineIndex + 1) % HERO_LINES.length
    }
    // Deliberately slices the OLD `line` (captured above, before a possible
    // wrap) rather than re-reading `HERO_LINES[lineIndex]` post-increment —
    // ported verbatim from the storyboard, where this looks like a bug but
    // isn't: on the wrap tick `charCount` is reset to 0, and `line.slice(0,
    // 0)` is `''` regardless of which string `line` holds, so the visible
    // behaviour (blank at the hold-to-next-line boundary, then type the new
    // line from scratch next tick) is identical either way.
    lineEl.textContent = line.slice(0, charCount)
  }, HERO_TICK_MS)
}

/**
 * Clipboard copy buttons. NOT gated behind the motion/flat check above —
 * copying text isn't motion, and a flat visitor deserves a working copy
 * button as much as anyone. Ships `hidden` in the markup (see Hero.astro/
 * FooterCta.astro) and stays hidden for any visitor without a Clipboard
 * API to back it. Selector is deliberately `button[data-copy]`, not just
 * `[data-copy]` — Journey.astro's four beat-copy paragraph blocks used to
 * collide with this exact attribute before Task 2's fix round renamed them
 * to `data-beat-copy`; scoping to `<button>` keeps that renamed-away
 * ambiguity from ever silently coming back.
 */
function initCopyButtons() {
  if (!navigator.clipboard) return

  document.querySelectorAll('button[data-copy]').forEach((button) => {
    button.hidden = false
    button.addEventListener('click', () => {
      const pre = button.previousElementSibling
      navigator.clipboard.writeText(pre ? pre.textContent : '')
      button.textContent = 'copied'
      setTimeout(() => {
        button.textContent = 'copy'
      }, 1200)
    })
  })
}

initEnhance()
