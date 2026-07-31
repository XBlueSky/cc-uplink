import fragmentSource from './shaders/ambience.frag?raw'

/*
 * Inlined from the now-deleted src/lib/contrast.mjs (Task 1 of the
 * ink-and-light reskin deletes that file; this shader is slated for
 * removal in a later task, but until then it still needs the exact same
 * LUM_CEILING it always used). Same math, same hardcoded FG anchor
 * (#e6e9f0 — the pre-reskin --fg, not tokens.css's Task-1 remap), same
 * resulting constant — a pure copy, not a behaviour change. See
 * site/test/contrast-ceiling.test.mjs for the sibling copy guarding this
 * same math via tests.
 */
const AMBIENCE_FG = '#e6e9f0'

function srgbToLinear(channel) {
  const s = channel / 255
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4
}

function relativeLuminance(hex) {
  const n = Number.parseInt(hex.replace('#', ''), 16)
  const r = (n >> 16) & 0xff
  const g = (n >> 8) & 0xff
  const b = n & 0xff
  return 0.2126 * srgbToLinear(r) + 0.7152 * srgbToLinear(g) + 0.0722 * srgbToLinear(b)
}

/** The highest background relative luminance that still holds `ratio` against `fgHex`. */
function maxBackgroundLuminance(fgHex = AMBIENCE_FG, ratio = 4.5) {
  return (relativeLuminance(fgHex) + 0.05) / ratio - 0.05
}

const LUM_CEILING = maxBackgroundLuminance()

const VERTEX_SOURCE = `
attribute vec2 aPosition;
void main() { gl_Position = vec4(aPosition, 0.0, 1.0); }
`

const MAX_DPR = 1.5

// Shared by pickRenderer's probe calls and initAmbience's real context: per
// the WebGL spec, only the FIRST getContext() call on a canvas actually
// creates the context — every later call (even with different attributes)
// just returns the same context, silently ignoring what it was passed. If
// the probe and the real acquisition disagreed, the probe's attributes
// (implicitly the defaults) would win and antialias/alpha/powerPreference
// below would be discarded.
const CONTEXT_ATTRS = { antialias: false, alpha: false, powerPreference: 'low-power' }

/**
 * Picks a renderer for `canvas` by probing WebGL2 then WebGL1.
 *
 * Side effect: this permanently allocates a GL context on `canvas`. A
 * canvas can only ever be bound to one context, so whichever mode this
 * returns is also the context `initAmbience` receives back from its own
 * `getContext` call — which is why both call sites must pass the same
 * `CONTEXT_ATTRS`.
 */
export function pickRenderer(canvas) {
  try {
    if (canvas.getContext('webgl2', CONTEXT_ATTRS)) return 'webgl2'
    if (canvas.getContext('webgl', CONTEXT_ATTRS)) return 'webgl'
  } catch {
    return 'css'
  }
  return 'css'
}

function compile(gl, type, source) {
  const shader = gl.createShader(type)
  gl.shaderSource(shader, source)
  gl.compileShader(shader)
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    const log = gl.getShaderInfoLog(shader)
    gl.deleteShader(shader)
    throw new Error(`shader compile failed: ${log}`)
  }
  return shader
}

/**
 * @param {{canvas: HTMLCanvasElement, reducedMotion: boolean}} options
 */
export function initAmbience({ canvas, reducedMotion }) {
  const mode = pickRenderer(canvas)
  if (mode === 'css') {
    // Leave the canvas hidden; the wrapper's gradient is the design.
    return { setProgress() {}, destroy() {} }
  }

  const gl = canvas.getContext(mode === 'webgl2' ? 'webgl2' : 'webgl', CONTEXT_ATTRS)

  let program = null
  let buffer = null
  let uResolution = null
  let uTime = null
  let uProgress = null
  let uLumCeiling = null
  let uPixelScale = null

  let progress = 0
  let frame = 0
  let resizeFrame = 0
  let visible = true
  let disposed = false

  function resize() {
    const dpr = Math.min(window.devicePixelRatio || 1, MAX_DPR)
    const scale = window.innerWidth < 700 ? 0.75 : 1
    const pixelScale = dpr * scale
    const width = Math.max(1, Math.floor(canvas.clientWidth * pixelScale))
    const height = Math.max(1, Math.floor(canvas.clientHeight * pixelScale))
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width
      canvas.height = height
    }
    gl.viewport(0, 0, canvas.width, canvas.height)
    gl.uniform2f(uResolution, canvas.width, canvas.height)
    // Lets the shader hold the scanline's period constant in CSS pixels —
    // see the comment on `scan` in ambience.frag.
    gl.uniform1f(uPixelScale, pixelScale)
  }

  // Builds (or, after a context-loss round trip, rebuilds) every GL object
  // this module owns. Split out of the top-level function body so
  // `webglcontextrestored` can re-run exactly this and nothing else — a
  // restored context is a *new* context object underneath the same `gl`
  // reference, with none of the previous program/buffer/uniform state.
  function setup() {
    const vs = compile(gl, gl.VERTEX_SHADER, VERTEX_SOURCE)
    const fs = compile(gl, gl.FRAGMENT_SHADER, fragmentSource)
    program = gl.createProgram()
    gl.attachShader(program, vs)
    gl.attachShader(program, fs)
    gl.linkProgram(program)
    // Flag both for deletion now: GL keeps a shader alive only while it
    // stays attached to a program, so these are actually freed the moment
    // destroy() (or the next setup()) deletes `program` — no need to hold
    // separate shader refs until then.
    gl.deleteShader(vs)
    gl.deleteShader(fs)
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      throw new Error(gl.getProgramInfoLog(program) || 'link failed')
    }

    gl.useProgram(program)

    buffer = gl.createBuffer()
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer)
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW)
    const aPosition = gl.getAttribLocation(program, 'aPosition')
    gl.enableVertexAttribArray(aPosition)
    gl.vertexAttribPointer(aPosition, 2, gl.FLOAT, false, 0, 0)

    uResolution = gl.getUniformLocation(program, 'uResolution')
    uTime = gl.getUniformLocation(program, 'uTime')
    uProgress = gl.getUniformLocation(program, 'uProgress')
    uLumCeiling = gl.getUniformLocation(program, 'uLumCeiling')
    uPixelScale = gl.getUniformLocation(program, 'uPixelScale')
    gl.uniform1f(uLumCeiling, LUM_CEILING)

    resize()
  }

  try {
    setup()
  } catch (error) {
    // A compile or link failure must fall back, not throw into the page.
    console.warn('[ambience] falling back to the CSS gradient:', error.message)
    return { setProgress() {}, destroy() {} }
  }

  function draw(timeMs) {
    // A stale rAF callback (already scheduled before dispose/context-loss)
    // can still fire once after either has happened — this closes that gap
    // and makes destroy() final against a still-live subscriber.
    if (disposed || gl.isContextLost()) return
    // Wrapped rather than raw ms/1000: sin()'s precision decays on very
    // large arguments, which would freeze the fbm grain/drift on a tab left
    // open for ~10+ hours. Wrapping at 3600s costs one visible texture
    // reseed per hour in exchange for grain that never dies.
    gl.uniform1f(uTime, reducedMotion ? 0 : (timeMs / 1000) % 3600)
    gl.uniform1f(uProgress, progress)
    gl.drawArrays(gl.TRIANGLES, 0, 3)
    canvas.dataset.ready = 'true'
  }

  // The one and only place a new frame gets requested. Every call site —
  // the initial kick, the loop's own continuation, the IntersectionObserver
  // callback, and context-restore — funnels through this same guard, so at
  // most one rAF chain is ever in flight. Without `!frame` here, the
  // IntersectionObserver's mandatory initial notification (which fires
  // shortly after `observe()` regardless of whether visibility actually
  // changed) schedules a second permanent chain on every load.
  function scheduleLoop() {
    if (visible && !reducedMotion && !disposed && !frame) frame = requestAnimationFrame(loop)
  }

  function loop(timeMs) {
    frame = 0
    if (disposed) return
    draw(timeMs)
    scheduleLoop()
  }

  if (reducedMotion) {
    // Exactly one frame, then nothing. No rAF loop at all.
    draw(0)
  } else {
    scheduleLoop()
  }

  const onResize = () => {
    // Trailing-rAF debounce: a resize storm (drag-resizing a window,
    // rotating a device) collapses to one buffer reallocation per animation
    // frame instead of one per event.
    if (resizeFrame) return
    resizeFrame = requestAnimationFrame(() => {
      resizeFrame = 0
      resize()
      if (reducedMotion) draw(0)
    })
  }
  window.addEventListener('resize', onResize, { passive: true })

  function onContextLost(event) {
    // Without preventDefault(), the browser never fires
    // webglcontextrestored and the canvas stays dead permanently.
    event.preventDefault()
    cancelAnimationFrame(frame)
    frame = 0
    // Dropping data-ready lets the canvas's own 400ms opacity transition
    // (Ambience.astro) hand the view back to the CSS gradient instead of
    // leaving a frozen frame from the dead context on top of it.
    delete canvas.dataset.ready
  }

  function onContextRestored() {
    try {
      setup()
    } catch (error) {
      console.warn('[ambience] failed to restore after context loss:', error.message)
      return
    }
    if (reducedMotion) {
      draw(0)
    } else {
      scheduleLoop()
    }
  }

  canvas.addEventListener('webglcontextlost', onContextLost, false)
  canvas.addEventListener('webglcontextrestored', onContextRestored, false)

  const observer = new IntersectionObserver(([entry]) => {
    visible = entry.isIntersecting
    scheduleLoop()
  })
  observer.observe(canvas)

  return {
    setProgress(next) {
      // Idempotent by design: enhance.mjs's initEnhance() drives this —
      // ambience's progress uniform rides the same shared scroll tick as
      // the §2/§3 scrubs, and that tick's `onScroll()` is called once
      // immediately after the listener is wired, so every path already
      // gets an initial call at start-of-page progress before any real
      // scroll event. The reduced-motion contract is exactly one frame — a
      // same-value call must not trigger a second draw.
      if (next === progress) return
      progress = next
      if (reducedMotion) draw(0)
    },
    destroy() {
      disposed = true
      cancelAnimationFrame(frame)
      cancelAnimationFrame(resizeFrame)
      window.removeEventListener('resize', onResize)
      canvas.removeEventListener('webglcontextlost', onContextLost)
      canvas.removeEventListener('webglcontextrestored', onContextRestored)
      observer.disconnect()
      gl.deleteProgram(program)
      gl.deleteBuffer(buffer)
      delete canvas.dataset.ready
      gl.getExtension('WEBGL_lose_context')?.loseContext()
    },
  }
}
