import { LUM_CEILING } from '../lib/contrast.mjs'
import fragmentSource from './shaders/ambience.frag?raw'

const VERTEX_SOURCE = `
attribute vec2 aPosition;
void main() { gl_Position = vec4(aPosition, 0.0, 1.0); }
`

const MAX_DPR = 1.5

export function pickRenderer(canvas) {
  try {
    if (canvas.getContext('webgl2')) return 'webgl2'
    if (canvas.getContext('webgl')) return 'webgl'
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

  const gl = canvas.getContext(mode === 'webgl2' ? 'webgl2' : 'webgl', {
    antialias: false,
    alpha: false,
    powerPreference: 'low-power',
  })

  let program
  try {
    program = gl.createProgram()
    gl.attachShader(program, compile(gl, gl.VERTEX_SHADER, VERTEX_SOURCE))
    gl.attachShader(program, compile(gl, gl.FRAGMENT_SHADER, fragmentSource))
    gl.linkProgram(program)
    if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
      throw new Error(gl.getProgramInfoLog(program) ?? 'link failed')
    }
  } catch (error) {
    // A compile or link failure must fall back, not throw into the page.
    console.warn('[ambience] falling back to the CSS gradient:', error.message)
    return { setProgress() {}, destroy() {} }
  }

  gl.useProgram(program)

  const buffer = gl.createBuffer()
  gl.bindBuffer(gl.ARRAY_BUFFER, buffer)
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW)
  const aPosition = gl.getAttribLocation(program, 'aPosition')
  gl.enableVertexAttribArray(aPosition)
  gl.vertexAttribPointer(aPosition, 2, gl.FLOAT, false, 0, 0)

  const uResolution = gl.getUniformLocation(program, 'uResolution')
  const uTime = gl.getUniformLocation(program, 'uTime')
  const uProgress = gl.getUniformLocation(program, 'uProgress')
  const uLumCeiling = gl.getUniformLocation(program, 'uLumCeiling')
  gl.uniform1f(uLumCeiling, LUM_CEILING)

  let progress = 0
  let frame = 0
  let visible = true
  let disposed = false

  function resize() {
    const dpr = Math.min(window.devicePixelRatio || 1, MAX_DPR)
    const scale = window.innerWidth < 700 ? 0.75 : 1
    const width = Math.max(1, Math.floor(canvas.clientWidth * dpr * scale))
    const height = Math.max(1, Math.floor(canvas.clientHeight * dpr * scale))
    if (canvas.width !== width || canvas.height !== height) {
      canvas.width = width
      canvas.height = height
    }
    gl.viewport(0, 0, canvas.width, canvas.height)
    gl.uniform2f(uResolution, canvas.width, canvas.height)
  }

  function draw(timeMs) {
    gl.uniform1f(uTime, reducedMotion ? 0 : timeMs / 1000)
    gl.uniform1f(uProgress, progress)
    gl.drawArrays(gl.TRIANGLES, 0, 3)
    canvas.dataset.ready = 'true'
  }

  function loop(timeMs) {
    if (disposed) return
    draw(timeMs)
    if (visible) frame = requestAnimationFrame(loop)
  }

  resize()

  if (reducedMotion) {
    // Exactly one frame, then nothing. No rAF loop at all.
    draw(0)
  } else {
    frame = requestAnimationFrame(loop)
  }

  const onResize = () => {
    resize()
    if (reducedMotion) draw(0)
  }
  window.addEventListener('resize', onResize, { passive: true })

  const observer = new IntersectionObserver(([entry]) => {
    visible = entry.isIntersecting
    if (visible && !reducedMotion && !disposed) frame = requestAnimationFrame(loop)
  })
  observer.observe(canvas)

  return {
    setProgress(next) {
      // Idempotent by design: initTimeline emits an initial onProgress(0) on
      // every path, and the reduced-motion contract is exactly one frame — a
      // same-value call must not trigger a second draw.
      if (next === progress) return
      progress = next
      if (reducedMotion) draw(0)
    },
    destroy() {
      disposed = true
      cancelAnimationFrame(frame)
      window.removeEventListener('resize', onResize)
      observer.disconnect()
    },
  }
}
