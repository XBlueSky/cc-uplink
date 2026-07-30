import { clamp01 } from '../lib/scrub.mjs'
import { journeyState } from '../lib/journeyState.mjs'

/**
 * Gate: flat visitors (reduced-motion or a viewport under 760px, same
 * breakpoint as Rail.astro's own media query) get the markup exactly as
 * shipped and nothing else — it's already the complete experience. This
 * module's only job past the gate is the §2 signal journey, the page's one
 * pinned scene.
 *
 * `enhanced` is added to <html> synchronously, before first paint (same
 * CLS-0 technique as the deck's `.motion` gate) — every enhanced-only CSS
 * rule in Journey.astro hangs off `.enhanced` for exactly this reason: a
 * flat visitor never gets the class, so the "lines start hidden" rules
 * never apply to them and there is no flash of hidden text.
 */
export function initEnhance() {
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
  initJourney()
}

/**
 * Wires the §2 pinned scroll scrub. All choreography math lives in the pure
 * `journeyState()` (site/src/lib/journeyState.mjs, unit tested without a
 * browser); this function only measures the DOM, reads the two typed
 * lines' full text from their own `data-text` attributes (the markup is
 * the single source of truth — never hardcode the copy here), and writes
 * `journeyState()`'s output back into the DOM, deduping every write
 * against the previous frame's value since this runs on every scroll frame.
 */
function initJourney() {
  const section = document.querySelector('[data-journey]')
  if (!section) return

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
  if (required.some((el) => !el)) return

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

  let ticking = false
  const onScroll = () => {
    if (ticking) return
    ticking = true
    requestAnimationFrame(() => {
      ticking = false
      const total = track.offsetHeight - innerHeight
      render(clamp01(-track.getBoundingClientRect().top / total))
    })
  }

  addEventListener('scroll', onScroll, { passive: true })
  addEventListener('resize', onScroll)
  onScroll()
}

initEnhance()
