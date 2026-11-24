#version 100
precision highp float;

varying vec2 frag_position;

uniform vec2  u_resolution;
uniform float u_time;
uniform float u_seed;
uniform float u_scale;
uniform float u_curl;
uniform float u_warp;
uniform float u_openness;
uniform float u_grain;
uniform float u_count;
uniform vec3 u_c0; uniform vec3 u_c1; uniform vec3 u_c2; uniform vec3 u_c3;
uniform vec3 u_c4; uniform vec3 u_c5; uniform vec3 u_c6; uniform vec3 u_c7;

vec3 mod289_3(vec3 x) { return x - floor(x * (1.0 / 289.0)) * 289.0; }
vec2 mod289_2(vec2 x) { return x - floor(x * (1.0 / 289.0)) * 289.0; }
vec3 permute(vec3 x) { return mod289_3(((x * 34.0) + 1.0) * x); }

float snoise(vec2 v) {
  const vec4 C = vec4(0.211324865405187, 0.366025403784439,
                     -0.577350269189626, 0.024390243902439);
  vec2 i  = floor(v + dot(v, C.yy));
  vec2 x0 = v - i + dot(i, C.xx);
  vec2 i1 = (x0.x > x0.y) ? vec2(1.0, 0.0) : vec2(0.0, 1.0);
  vec4 x12 = x0.xyxy + C.xxzz;
  x12.xy -= i1;
  i = mod289_2(i);
  vec3 p = permute(permute(i.y + vec3(0.0, i1.y, 1.0)) + i.x + vec3(0.0, i1.x, 1.0));
  vec3 m = max(0.5 - vec3(dot(x0, x0), dot(x12.xy, x12.xy), dot(x12.zw, x12.zw)), 0.0);
  m = m * m; m = m * m;
  vec3 x = 2.0 * fract(p * C.www) - 1.0;
  vec3 h = abs(x) - 0.5;
  vec3 ox = floor(x + 0.5);
  vec3 a0 = x - ox;
  m *= 1.79284291400159 - 0.85373472095314 * (a0 * a0 + h * h);
  vec3 g;
  g.x  = a0.x  * x0.x  + h.x  * x0.y;
  g.yz = a0.yz * x12.xz + h.yz * x12.yw;
  return 130.0 * dot(m, g);
}

float fbm2(vec2 p) {
  float sum = snoise(p) * 0.5;
  sum += snoise(p * 2.0) * 0.25;
  return sum / 0.75;
}

float spread(float x, float sd) {
  float k = 1.07 * sd;
  float s = x / sqrt(x * x + k * k);
  return clamp(0.5 + 0.5 * s / 0.96, 0.0, 1.0);
}

vec2 curlNoise(vec2 p) {
  float e = 0.05;
  float dx = (snoise(p + vec2(e, 0.0)) - snoise(p - vec2(e, 0.0))) / (2.0 * e);
  float dy = (snoise(p + vec2(0.0, e)) - snoise(p - vec2(0.0, e))) / (2.0 * e);
  return vec2(dy, -dx);
}

float grain(vec2 uv, float time) {
  vec2 st = uv * 200.0 + time * 0.1;
  return fract(sin(dot(st, vec2(12.9898, 78.233))) * 43758.5453);
}

vec3 paletteLab(float t) {
  float x = clamp(t, 0.0, 1.0) * (u_count - 1.0);
  vec3 c = u_c0;
  c = mix(c, u_c1, clamp(x - 0.0, 0.0, 1.0));
  c = mix(c, u_c2, clamp(x - 1.0, 0.0, 1.0));
  c = mix(c, u_c3, clamp(x - 2.0, 0.0, 1.0));
  c = mix(c, u_c4, clamp(x - 3.0, 0.0, 1.0));
  c = mix(c, u_c5, clamp(x - 4.0, 0.0, 1.0));
  c = mix(c, u_c6, clamp(x - 5.0, 0.0, 1.0));
  c = mix(c, u_c7, clamp(x - 6.0, 0.0, 1.0));
  return c;
}

vec3 oklabToSrgb(vec3 c) {
  float l_ = c.x + 0.3963377774 * c.y + 0.2158037573 * c.z;
  float m_ = c.x - 0.1055613458 * c.y - 0.0638541728 * c.z;
  float s_ = c.x - 0.0894841775 * c.y - 1.2914855480 * c.z;
  float l = l_ * l_ * l_;
  float m = m_ * m_ * m_;
  float s = s_ * s_ * s_;
  vec3 rgb = vec3( 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
                  -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
                  -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s);
  rgb = clamp(rgb, 0.0, 1.0);
  return mix(rgb * 12.92, 1.055 * pow(rgb, vec3(1.0 / 2.4)) - 0.055,
             step(vec3(0.0031308), rgb));
}

void main() {
  float aspect = u_resolution.x / u_resolution.y;
  vec2 uv = frag_position * 0.5 * vec2(aspect, 1.0) + 0.5;

  float curlScale = u_scale * u_curl;
  vec2 seed = vec2(u_seed);

  float a = u_time * 0.35;
  vec2 o1 = 0.30 * vec2(cos(a * 1.00),       sin(a * 1.00 + 1.0));
  vec2 o2 = 0.25 * vec2(cos(a * 0.77 + 2.1), sin(a * 0.63 + 4.0));
  vec2 o3 = 0.20 * vec2(cos(a * 0.53 + 4.2), sin(a * 0.91 + 0.5));
  vec2 o4 = 0.15 * vec2(sin(a * 0.41 + 1.7), cos(a * 0.37 + 3.3));

  vec2 advected = uv;
  advected += curlNoise(advected * curlScale + seed + o1) * u_warp * 0.3;
  advected += curlNoise(advected * curlScale + seed + o2) * u_warp * 0.3;
  advected += curlNoise(advected * curlScale + seed + o3) * u_warp * 0.3;

  float t = fbm2(advected * u_scale + seed + o4);
  t = spread(t, 0.35);
  t = mix(t, t * t, u_openness);

  vec3 color = oklabToSrgb(paletteLab(t));

  float g = grain(uv, u_time) - 0.5;
  color += g * u_grain;

  gl_FragColor = vec4(clamp(color, 0.0, 1.0), 1.0);
}

