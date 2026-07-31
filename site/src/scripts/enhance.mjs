import { clamp01 } from '../lib/scrub.mjs'
import { journeyState, RETURN_PACKET_TEXT } from '../lib/journeyState.mjs'
import { demoState, demoProgress } from '../lib/demoState.mjs'
import { initAmbience } from './ambience.mjs'

/**
 * Gate: flat visitors (reduced-motion or a viewport under 760px, same
 * breakpoint as Rail.astro's own media query) get the markup exactly as
 * shipped and nothing else — it's already the complete experience. This
 * module's job past the gate is every motion behaviour on the page: the §2
 * signal journey (the one pinned scene), the §3 invoke-demo scrub, the §4–§6
 * one-time reveals, the rail scrollspy, and the hero idle loop. It also owns
 * the WebGL ambience shader (see `initPageAmbience` below) — that one is
 * NOT gated, because the shader is the page's background for every visitor,
 * flat included (see that function's own comment for why).
 *
 * The flat/enhanced decision itself is NOT made here — it's made exactly
 * once, by a parser-blocking classic `<script is:inline>` at the top of
 * `<Base>` in index.astro, which stamps `enhanced` onto `<html>`
 * synchronously, before the parser reaches `[data-page]` (before first
 * paint). This module (a `<script type="module">`, deferred whether Astro
 * inlines it or chunks it externally — see budget.test.mjs) only ever runs
 * AFTER that, so it just reads the class rather than re-deriving it.
 *
 * Reading the class back is not the same as trusting it, though: the gate
 * script stamps `enhanced` (and, independently, `has-clipboard`)
 * optimistically, before any JS is known to have actually run past that
 * point. If this module's chunk 404s, is blocked, or fails to parse in an
 * old browser, those classes would otherwise sit on `<html>` forever with
 * nothing left alive to satisfy the CSS they gate — every enhanced-only
 * rule below keeps hiding its target, and the copy buttons stay visible
 * with no click handler ever wired, both with no script remaining to fix
 * either. The self-heal in index.astro's gate script closes that gap:
 * `initEnhance()` below sets `<html>`'s `enhanceReady` dataset flag as the
 * very first statement of BOTH branches of its flat/enhanced split — proof
 * this module got at least that far, regardless of which side of the split
 * a given visitor landed on. A `window.load` listener (armed by the same
 * gate script, unconditionally) strips `enhanced` and `has-clipboard` back
 * off if that flag was never set — see index.astro's gate comment for the
 * full mechanism and why a healthy run, flat or enhanced, never trips it.
 *
 * Every enhanced-only CSS rule in the section components hangs off
 * `.enhanced` for the same reason the gate had to move pre-paint: a flat
 * visitor never gets the class, so the "lines/reveals start hidden" rules
 * never apply to them and there is no flash of hidden text.
 *
 * The copy buttons (and the hero line's flat fallback) are the one
 * exception: neither is motion, so both are wired unconditionally, before
 * the gate check below (see initCopyButtons/initHeroLineFallback). The
 * ambience shader joins them here for the same reason.
 */
export function initEnhance() {
  initCopyButtons()
  initHeroLineFallback()

  // Constructed unconditionally, before the gate check — see
  // `initPageAmbience`'s own comment for why a flat visitor still gets a
  // live (or, under reduced motion, single-frame) shader rather than the
  // module skipping it entirely.
  const ambience = initPageAmbience()

  if (!document.documentElement.classList.contains('enhanced')) {
    // Proof-of-life for index.astro's load-time self-heal, flat side. A
    // flat visitor's run is just as "healthy" as an enhanced one — copy
    // buttons and the hero fallback line are already wired above by this
    // point, so the self-heal must not strip `has-clipboard` out from under
    // an already-working button just because this visitor never qualified
    // for `enhanced` in the first place. See the enhanced-path copy of this
    // same assignment below for the other half.
    document.documentElement.dataset.enhanceReady = 'true'
    return
  }

  // Proof-of-life for index.astro's load-time self-heal, enhanced side: the
  // FIRST statement of the enhanced path, not the last thing init does. A
  // module script always finishes running before `window.load` fires, so
  // on a healthy page this (or the flat-path copy above) is already set by
  // the time that listener checks it — the heal is a guaranteed no-op
  // either way a visitor's run lands. A module that never got as far as
  // EITHER copy of this assignment (chunk 404, blocked request, parse
  // error) never sets it, so the heal always fires for that case and the
  // page falls back to the flat experience. See index.astro's gate comment
  // for the other half.
  document.documentElement.dataset.enhanceReady = 'true'

  // §2 and §3 are both scroll-position-driven scrubs; they share ONE
  // passive `scroll` listener with a single rAF-guarded tick rather than
  // each wiring their own (see the bottom of this function) — `initJourney`
  // and `initDemo` only measure the DOM and return a zero-argument "render
  // this frame" callback, they never touch `addEventListener` themselves.
  // The ambience shader's progress uniform rides the same shared tick, once
  // more per frame than either scrub — see the pushed callback below.
  const ticks = [initJourney(), initDemo()].filter(Boolean)
  if (ambience) {
    ticks.push(() => {
      ambience.setProgress(clamp01(scrollY / (document.documentElement.scrollHeight - innerHeight)))
    })
  }

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
 * Boots the WebGL ambience shader (site/src/scripts/ambience.mjs) — the
 * page's fixed-position background, formerly bootstrapped by index.astro's
 * own dedicated `<script>` (see that file's diff history). Moved here so
 * enhance.mjs is the one module that owns every piece of page behaviour;
 * index.astro now imports only this file.
 *
 * Called unconditionally, before the enhanced/flat gate check above — the
 * shader IS the design (Ambience.astro's CSS gradient is only its no-WebGL
 * fallback), not a motion flourish layered on top of it, so a flat visitor
 * (reduced-motion or narrow viewport) still gets it rendered. `reducedMotion`
 * is read directly from `matchMedia` here — deliberately NOT derived from
 * the `enhanced` class, whose gate also folds in a `narrow viewport` check
 * (see index.astro's gate script): a visitor who is narrow but has NOT asked
 * for reduced motion should still get a live, animated shader, just without
 * the §2/§3 scroll scrub that only exists in enhanced mode. `initAmbience`'s
 * own contract (see that module) treats `reducedMotion` as "freeze to one
 * static frame" — folding the narrow check in here would wrongly freeze the
 * shader for every narrow-but-motion-OK visitor too.
 *
 * Returns the handle `initAmbience` hands back (with `setProgress`/
 * `destroy`), or `null` if the page has no `[data-ambience]` canvas at all.
 */
function initPageAmbience() {
  const canvas = document.querySelector('[data-ambience]')
  if (!canvas) return null
  const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)').matches
  return initAmbience({ canvas, reducedMotion })
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

  const peerIdleEl = peerPre?.querySelector('[data-peer-idle]')
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
  // The markup contract not being met (a future edit dropping a hook) is
  // handled by returning `null` here rather than throwing mid-scroll and
  // taking the rest of the page's JS down with it. That avoids a crash, but
  // it is NOT the same guarantee index.astro's load-time self-heal gives:
  // that heal only fires when `initEnhance()` never got as far as setting
  // `enhanceReady` (the whole module failing to run) — and this function is
  // called well after that flag is already set (see `initEnhance`), so a
  // narrower failure limited to THIS function (a future edit dropping just
  // one of Journey's own hooks, while the rest of the module still loads
  // and runs fine) doesn't trip it. Journey.astro's enhanced-mode CSS hides
  // `[data-line]` unconditionally off `.enhanced` (no per-instance "only
  // hide once this actually wired up" gate the way `initDemo` below has via
  // `.scrubbing`), so that narrower case would still render blank, not
  // flat. Giving Journey the same `.scrubbing`-style gate is tracked for a
  // future round, not done here.
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
        // Sanctioned §3 hook (design spec, Task 3): flips the trace SVG's
        // arrowhead via CSS alone — [data-beam] carries `.return` on the
        // reply leg, cleared on the outbound leg. See Journey.astro's
        // `.arrow-out`/`.arrow-back` rules for the CSS half of this.
        beamEl.classList.toggle('return', state.packet.text === RETURN_PACKET_TEXT)
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
  // supposed to track. Queried via its own `[data-demo-box]` hook rather
  // than the `.demo` class it also carries — a class is presentational and
  // can get renamed/restyled without anyone thinking of this script; the
  // data attribute is the explicit contract.
  const demoBox = section.querySelector('[data-demo-box]')
  const pre = demoBox?.querySelector('pre')

  const askEl = section.querySelector('[data-typed]')
  const caretEl = section.querySelector('[data-caret]')
  const [callEl, midEl, pathsEl] = section.querySelectorAll('[data-line]:not([data-typed])')
  const artEl = section.querySelector('[data-art]')

  const required = [demoBox, pre, askEl, caretEl, callEl, midEl, pathsEl, artEl]
  if (required.some((el) => !el)) return null

  // Unlike initJourney above, this section's CSS does NOT hide anything off
  // `.enhanced` alone — InvokeDemo.astro's opacity-0/display-none rules are
  // keyed off `.enhanced [data-demo].scrubbing`, and `.scrubbing` is only
  // ever added right here, after every required element above was
  // confirmed present. So if a future markup edit breaks this contract,
  // this function still returns `null` (no crash) AND the CSS never starts
  // hiding lines/the artifact in the first place — a broken contract falls
  // back to the fully-visible flat rendering instead of getting stuck
  // hidden forever with no JS left to turn it back on.
  section.classList.add('scrubbing')

  // a11y: same treatment as §2's initJourney above — this pane's typewriter
  // churn is described once, up front, by the section's adjacent intro
  // paragraph, so hide the raw replay from assistive tech and drop the
  // pane's own tabindex so a hidden region can't still catch keyboard focus.
  pre.setAttribute('aria-hidden', 'true')
  pre.tabIndex = -1

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
 * gets this `setInterval` at all — see `initHeroLineFallback` below for
 * what they get instead (the markup ships the span empty per Task 2/the
 * storyboard's own contract, but a flat visitor with nothing else touching
 * that span would otherwise see a permanently blank line next to a static
 * caret, which is what that function exists to prevent).
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
 * Flat fallback for the hero idle loop — the storyboard's own `else`
 * branch (`if (!reduced) { setInterval(...) } else { $('hero-line')
 * .textContent = heroLines[0] }`), restored here. `initHeroLoop`'s
 * `setInterval` only ever runs past the enhanced gate, so without this a
 * flat/reduced-motion/narrow visitor would see the hero pane's second line
 * sit permanently empty next to a static (CSS-animated or not) caret —
 * not the storyboard's intent, which was always to show at least the
 * first idle line even when nothing is animating it. Not gated on
 * `.enhanced` itself: called unconditionally, alongside `initCopyButtons`,
 * and checks the class directly so it does the opposite thing on each side
 * of the gate (flat → set the static line once; enhanced → do nothing,
 * `initHeroLoop` owns the span from here).
 */
function initHeroLineFallback() {
  if (document.documentElement.classList.contains('enhanced')) return
  const lineEl = document.querySelector('[data-hero-line]')
  if (!lineEl) return
  lineEl.textContent = HERO_LINES[0]
}

/**
 * Clipboard copy buttons. NOT gated behind the motion/flat check above —
 * copying text isn't motion, and a flat visitor deserves a working copy
 * button as much as anyone. Visibility isn't this function's job any more:
 * index.astro's pre-paint gate script already added `has-clipboard` to
 * `<html>` (independently of the `enhanced` check, same reason) whenever
 * `navigator.clipboard` exists, and Hero.astro/FooterCta.astro's own CSS
 * (`[data-copy] { display: none }` / `:global(.has-clipboard) [data-copy]`)
 * shows the button off that class alone — before this module has even
 * started executing, let alone reached this function. So the button is
 * already visible (or already correctly absent) by first paint; this
 * function only ever wires the click handler on top of it. Selector is
 * deliberately `button[data-copy]`, not just `[data-copy]` — Journey.astro's
 * four beat-copy paragraph blocks used to collide with this exact attribute
 * before Task 2's fix round renamed them to `data-beat-copy`; scoping to
 * `<button>` keeps that renamed-away ambiguity from ever silently coming
 * back.
 *
 * `writeText()` returns a promise that can reject (permission denied, no
 * secure context, etc.) — both outcomes are handled explicitly (`.then(on
 * success, on failure)`), so a rejection never becomes an uncaught
 * rejection and the label always reflects what actually happened rather
 * than optimistically claiming success before the write is even settled.
 * The reverting label is the button's OWN original text (captured once, at
 * wire-time) rather than a hardcoded `'copy'` — this button and the
 * footer's are wired by the same loop and could in principle ship
 * different label copy without this function needing to know. A pending
 * revert timer is cleared on every click so rapid re-clicking can't leave
 * two timers racing to stomp the label out from under each other.
 */
function initCopyButtons() {
  if (!navigator.clipboard) return

  document.querySelectorAll('button[data-copy]').forEach((button) => {
    const originalLabel = button.textContent
    let revertTimer = null

    const revertAfter = (ms) => {
      clearTimeout(revertTimer)
      revertTimer = setTimeout(() => {
        button.textContent = originalLabel
      }, ms)
    }

    button.addEventListener('click', () => {
      clearTimeout(revertTimer)
      const pre = button.previousElementSibling
      navigator.clipboard.writeText(pre ? pre.textContent : '').then(
        () => {
          button.textContent = 'copied'
          revertAfter(1200)
        },
        () => {
          button.textContent = 'failed'
          revertAfter(1200)
        },
      )
    })
  })
}

initEnhance()
