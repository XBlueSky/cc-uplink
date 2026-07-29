#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif

uniform vec2 uResolution;
uniform float uTime;
uniform float uProgress;
uniform float uLumCeiling;
uniform float uPixelScale;

float hash(vec2 p) {
  return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

float noise(vec2 p) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(
    mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x),
    mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x),
    u.y
  );
}

float fbm(vec2 p) {
  float value = 0.0;
  float amplitude = 0.5;
  for (int i = 0; i < 5; i++) {
    value += amplitude * noise(p);
    p *= 2.0;
    amplitude *= 0.5;
  }
  return value;
}

void main() {
  vec2 uv = gl_FragCoord.xy / uResolution;
  vec2 aspect = vec2(uResolution.x / uResolution.y, 1.0);

  // The diagonal runs lower-left to upper-right. The CSS fallback gradient
  // in Ambience.astro is 148deg to match this same slope — a 62deg
  // linear-gradient renders the mirrored stripe, so the two are reconciled
  // at 148deg, not 62deg (CSS 148deg ≙ this shader's dir = normalize(1.0, 0.62)).
  vec2 dir = normalize(vec2(1.0, 0.62));
  vec2 rel = (uv - vec2(0.15, 0.10)) * aspect;
  float along = dot(rel, dir);
  float across = abs(dot(rel, vec2(-dir.y, dir.x)));

  float drift = fbm(vec2(along * 2.0, uTime * 0.03)) - 0.5;
  float beam = exp(-pow((across + drift * 0.06) * 7.0, 2.0));
  beam *= smoothstep(-0.20, 0.35, along) * smoothstep(1.60, 0.70, along);

  // Energy rides the scroll: dim while the session is idle, full through
  // the send, settling to a steady field as the camera pulls back.
  float energy = mix(0.35, 1.0, smoothstep(0.0, 0.45, uProgress))
               * mix(1.0, 0.72, smoothstep(0.78, 1.0, uProgress));

  float haze = fbm(uv * 3.0 + vec2(0.0, uTime * 0.01)) * 0.06;

  // Scanline modulation, from ambience reference A. Divided by uPixelScale
  // (the effective dpr * mobile-scale used to size the drawing buffer) so
  // the period is stable in CSS pixels; without it, the MAX_DPR cap and the
  // sub-700px 0.75 scale each make the buffer:display ratio non-integer,
  // and the scanline beats against the buffer grid instead of the screen.
  float scan = 0.5 + 0.5 * sin(gl_FragCoord.y / uPixelScale * 3.14159265);

  vec3 base = vec3(0.024, 0.027, 0.047);
  vec3 indigo = vec3(0.180, 0.247, 0.490);
  vec3 cyan = vec3(0.345, 0.773, 0.831);

  vec3 col = base;
  col += mix(indigo, cyan, smoothstep(0.20, 0.90, uProgress)) * beam * energy * 0.42;
  col += indigo * haze;
  col *= 1.0 - 0.05 * scan;
  col += (hash(gl_FragCoord.xy + vec2(uTime)) - 0.5) * 0.012;

  // Hard ceiling on relative luminance so body text keeps WCAG AA against
  // the brightest state this shader can reach. uLumCeiling is derived from
  // WCAG in contrast.mjs and unit-tested there. The current palette's
  // analytic peak sits at ~61% of uLumCeiling, so this clamp is inactive
  // today — it is a safety net for future palette or intensity changes,
  // not an active constraint.
  vec3 linear = pow(max(col, 0.0), vec3(2.2));
  float lum = dot(linear, vec3(0.2126, 0.7152, 0.0722));
  if (lum > uLumCeiling) {
    col *= pow(uLumCeiling / lum, 1.0 / 2.2);
  }

  gl_FragColor = vec4(col, 1.0);
}
