import { clamp01, seg, typedSlice } from './scrub.mjs'

/**
 * §2 signal-journey beat thresholds, ported verbatim from the storyboard's
 * render() (docs/superpowers/design/2026-07-30-storyboard.html §2 — the
 * canonical behaviour reference). Do not retime these — they're the
 * reviewed choreography, not a first draft.
 *
 *   0.03–0.13 you type        0.15/0.19 tool call + delivery receipt
 *   0.24–0.38 packet outbound 0.40/0.44 envelope + reply hint land on peer
 *   0.48/0.53 codex works     0.58–0.68 codex types the reply command
 *   0.72–0.84 packet return   0.86      reply lands back in your pane
 */
const ASK_TYPE_START = 0.03
const ASK_TYPE_END = 0.13
const ASK_CARET_END = 0.15
const SEND_CALL = 0.15
const RECEIPT = 0.19
const ENV_HEAD = 0.4
const ENV_TAIL = 0.44
const WORK1 = 0.48
const WORK2 = 0.53
const CMD_TYPE_START = 0.58
const CMD_TYPE_END = 0.68
const REPLY_LANDING = 0.86
const OUT_START = 0.24
const OUT_END = 0.38
const BACK_START = 0.72
const BACK_END = 0.84
const PEER_ACTIVE_START = ENV_HEAD
const PEER_ACTIVE_END = REPLY_LANDING

// The brief specifies an explicit caret window only for the you-line ask
// ("visible while p<.15") — a blink that starts at the very top of the
// scrub (before typing begins) and lingers briefly past the moment typing
// finishes (0.13), until the tool call actually fires (0.15). The peer's
// reply-cmd caret has no equivalent threshold in the brief. This mirrors
// the same before/during/just-after shape rather than inventing new
// numbers: it starts once the cursor would plausibly land on that line
// (WORK2, the second review comment, going up) and ends when the reply is
// "sent" — the moment the return packet actually departs (BACK_START).
const CMD_CARET_START = WORK2
const CMD_CARET_END = BACK_START

export const OUTBOUND_PACKET_TEXT = '[uplink id:6b3b20e6] →'
export const RETURN_PACKET_TEXT = '← [reply id:6b3b20e6]'

/**
 * Pure progress -> state mapping for the §2 signal-journey scrub.
 *
 * @param {number} p - track scroll progress, 0..1 (anything else clamps,
 *   see scrub.mjs's clamp01 — this function is total, never throws).
 * @param {{askText: string, cmdText: string}} texts - the two typed lines'
 *   full text, read by the caller from each span's own `data-text`
 *   attribute at runtime. Deliberately not hardcoded here: the markup is
 *   the single source of truth, so a copy edit in Journey.astro can never
 *   drift out of sync with what this function types out.
 *
 * No DOM access happens in this function — that's what makes it unit
 * testable without a browser (site/test/journey-state.test.mjs) and is
 * also why the travelling packet reports a fraction (`t`) instead of a
 * pixel offset: the beam/packet element widths are only known once
 * measured against a real layout, which is the DOM-driving caller's job
 * (site/src/scripts/enhance.mjs), not this module's.
 */
export function journeyState(p, { askText, cmdText }) {
  const progress = clamp01(p)

  const askT = seg(progress, ASK_TYPE_START, ASK_TYPE_END)
  const cmdT = seg(progress, CMD_TYPE_START, CMD_TYPE_END)

  const outT = seg(progress, OUT_START, OUT_END)
  const backT = seg(progress, BACK_START, BACK_END)
  const outbound = outT > 0 && outT < 1
  const inbound = backT > 0 && backT < 1

  let packet = null
  if (outbound) packet = { text: OUTBOUND_PACKET_TEXT, t: outT }
  else if (inbound) packet = { text: RETURN_PACKET_TEXT, t: 1 - backT }

  const peerActive = progress >= PEER_ACTIVE_START && progress < PEER_ACTIVE_END

  return {
    ask: {
      text: typedSlice(askText, askT),
      caretOn: progress < ASK_CARET_END,
    },
    sendCallOn: progress >= SEND_CALL,
    receiptOn: progress >= RECEIPT,
    envHeadOn: progress >= ENV_HEAD,
    envTailOn: progress >= ENV_TAIL,
    peerIdleOpacity: progress >= ENV_HEAD ? 0.35 : 1,
    work1On: progress >= WORK1,
    work2On: progress >= WORK2,
    cmd: {
      text: typedSlice(cmdText, cmdT),
      caretOn: progress >= CMD_CARET_START && progress < CMD_CARET_END,
    },
    replyOn: progress >= REPLY_LANDING,
    beamOn: outbound || inbound,
    packet,
    peerActive,
    youFaded: peerActive,
    youActive: !peerActive && progress >= ASK_TYPE_START,
    beat:
      progress < OUT_START ? '[1/4]' : progress < WORK1 ? '[2/4]' : progress < BACK_START ? '[3/4]' : '[4/4]',
  }
}
