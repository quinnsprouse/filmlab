struct Lut {
  size: u32,
  unit: u32,
  offset: u32,
  pad: u32,
  lo: vec4f,
  hi: vec4f,
}

struct Params {
  zero: u32,
  flags: u32,
  radius: u32,
  count: u32,
  src: vec4u,
  dst: vec4u,
  bounds: vec4u,
  taps: vec4u,
  aux: vec4u,
  slope: vec4f,
  awg: array<vec4f, 3>,
  rec: array<vec4f, 3>,
  utility: Lut,
  negative: Lut,
  print: Lut,
}

struct Tap {
  lo: u32,
  hi: u32,
  f: f32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> src3: array<f32>;
@group(0) @binding(2) var<storage, read_write> dst3: array<f32>;
@group(0) @binding(3) var<storage, read> luts: array<vec4f>;
@group(0) @binding(4) var<storage, read> tables: array<f32>;
@group(0) @binding(5) var<storage, read> taps: array<u32>;
@group(0) @binding(6) var<storage, read> src1: array<f32>;
@group(0) @binding(7) var<storage, read_write> dst1: array<f32>;
@group(0) @binding(8) var<storage, read> src2: array<vec2f>;
@group(0) @binding(9) var<storage, read_write> dst2: array<vec2f>;

const contrastFlag = 1u;
const utilityFlag = 2u;
const coefficientsFlag = 4u;
const candidateFlag = 8u;
const highlightsFlag = 16u;
const lumaFlag = 128u;

const log2At = 0u;
const exp2At = 4097u;
const scanAt = 8194u;
const baseAt = 16387u;
const toneAt = 17388u;
const constsAt = 18389u;
const kneeAt = 18399u;
const domainAt = 18720u;
const cosAt = 24971u;
const transferAt = 29068u;
const transferSteps = 8000u;
const dwgAt = 37069u;
const maskAt = 37087u;
const domainIndexAt = 37108u;
const baseIndexAt = 45303u;
const transferIndexAt = 53498u;
const weightsAt = 61693u;
const domainLast = 6250u;
const gridSteps = 1000u;

const logcCut = bitcast<f32>(0x3c2d85e0u);
const logcA = bitcast<f32>(0x40b1c71du);
const logcB = bitcast<f32>(0x3d561b2au);
const logcC = bitcast<f32>(0x3e7d1f60u);
const logcD = bitcast<f32>(0x3ec5651bu);
const logcE = bitcast<f32>(0x40abc3d4u);
const logcF = bitcast<f32>(0x3dbe12a5u);
const log10of2 = bitcast<f32>(0x3e9a209bu);
const luma2020 = vec3f(bitcast<f32>(0x3e86809du), bitcast<f32>(0x3f2d9168u), bitcast<f32>(0x3d72e48fu));
const luma709 = vec3f(bitcast<f32>(0x3e59b3d0u), bitcast<f32>(0x3f371759u), bitcast<f32>(0x3d93dd98u));
const darkest = bitcast<f32>(0x3089705fu);
const grey = bitcast<f32>(0x3e3851ecu);
const decodeCut = bitcast<f32>(0x3da5e354u);
const encodeCut = bitcast<f32>(0x3c9374bcu);
const slope709 = bitcast<f32>(0x40900000u);
const scale709 = bitcast<f32>(0x3f8cac08u);
const offset709 = bitcast<f32>(0x3dcac083u);
const gamma709 = bitcast<f32>(0x3ee66666u);
const decodePower = bitcast<f32>(0x400e38e4u);
const kneeStart = bitcast<f32>(0x3f428f5cu);
const kneeSteps = 320u;
const tailStart = bitcast<f32>(0x3f8a3d71u);
const tailEnd = 2.0;
const grainOffset = bitcast<f32>(0x3ddb22d1u);
const flattenAmount = bitcast<f32>(0x3b72bb74u);
const flattenNorm = bitcast<f32>(0x3f7d1a22u);
const minusTwoLn2 = bitcast<f32>(0xbfb17218u);
const step23 = bitcast<f32>(0x34000000u);

// why: Metal fuses a * b + c and approximates division, so every step is forced to round on its own
// (an XOR with a runtime zero) and division is corrected to the nearest float, matching the CPU bit for bit
fn exact(x: f32) -> f32 {
  return bitcast<f32>(bitcast<u32>(x) ^ params.zero);
}

fn mul(a: f32, b: f32) -> f32 {
  return exact(a * b);
}

fn add(a: f32, b: f32) -> f32 {
  return exact(a + b);
}

fn sub(a: f32, b: f32) -> f32 {
  return exact(a - b);
}

fn miss(q: f32, a: f32, b: f32) -> f32 {
  return abs(exact(fma(-q, b, a)));
}

fn div(a: f32, b: f32) -> f32 {
  let rough = exact(a / b);
  let q = exact(fma(exact(fma(-rough, b, a)), exact(1.0 / b), rough));
  let up = bitcast<f32>(bitcast<u32>(q) + 1u);
  let down = bitcast<f32>(bitcast<u32>(q) - 1u);
  var best = q;
  var error = miss(q, a, b);
  let upError = miss(up, a, b);
  if (upError < error || (upError == error && (bitcast<u32>(up) & 1u) == 0u)) {
    best = up;
    error = upError;
  }
  let downError = miss(down, a, b);
  if (downError < error || (downError == error && (bitcast<u32>(down) & 1u) == 0u)) {
    best = down;
  }
  return best;
}

fn lerp(a: f32, z: f32, t: f32) -> f32 {
  return add(a, mul(sub(z, a), t));
}

fn lerp3(a: vec3f, z: vec3f, t: f32) -> vec3f {
  return vec3f(lerp(a.x, z.x, t), lerp(a.y, z.y, t), lerp(a.z, z.z, t));
}

fn blend(a: f32, b: f32, t: f32) -> f32 {
  return add(mul(a, sub(1.0, t)), mul(b, t));
}

fn row(m: vec4f, c: vec3f) -> f32 {
  return add(add(mul(m.x, c.x), mul(m.y, c.y)), mul(m.z, c.z));
}

fn luma(c: vec3f) -> f32 {
  return add(add(mul(luma709.x, c.x), mul(luma709.y, c.y)), mul(luma709.z, c.z));
}

// why: the partition point of u in the sorted table at..=at + steps, the binary search starting from the bucket of
// u's bits (engine/src/gpu.rs index); it ends where the search over the whole table would
fn ranked(at: u32, steps: u32, index: u32, u: f32) -> u32 {
  var lo = 0u;
  var hi = steps + 1u;
  if (u >= tables[at] && u <= tables[at + steps]) {
    let base = bitcast<u32>(tables[index]);
    let bits = max(select(bitcast<u32>(u), 0u, u == 0.0), base) - base;
    let b = 2u * min(bits >> u32(tables[index + 1u]), u32(tables[index + 2u]) - 1u);
    lo = u32(tables[index + 3u + b]);
    hi = u32(tables[index + 4u + b]);
  }
  while (lo < hi) {
    let mid = (lo + hi) / 2u;
    if (tables[at + mid] <= u) {
      lo = mid + 1u;
    } else {
      hi = mid;
    }
  }
  return lo;
}

fn tableAt(at: u32, steps: u32, t: f32) -> f32 {
  let i = min(u32(t), steps - 1u);
  let lo = tables[at + i];
  return add(lo, mul(sub(tables[at + i + 1u], lo), sub(t, f32(i))));
}

fn fastLog2(x: f32) -> f32 {
  let bits = bitcast<u32>(x);
  let exponent = f32(i32((bits >> 23u) & 0xffu) - 127);
  let mantissa = sub(bitcast<f32>((bits & 0x7fffffu) | 0x3f800000u), 1.0);
  let t = mul(mantissa, 4096.0);
  let i = min(u32(t), 4095u);
  let lo = tables[log2At + i];
  return add(add(exponent, lo), mul(sub(tables[log2At + i + 1u], lo), sub(t, f32(i))));
}

fn fastExp2(v: f32) -> f32 {
  let x = clamp(v, -126.0, 126.0);
  let whole = floor(x);
  let scale = bitcast<f32>(u32(i32(whole) + 127) << 23u);
  return mul(tableAt(exp2At, 4096u, mul(sub(x, whole), 4096.0)), scale);
}

fn logc3(x: f32) -> f32 {
  if (x > logcCut) {
    return add(mul(logcC, mul(fastLog2(add(mul(logcA, x), logcB)), log10of2)), logcD);
  }
  return add(mul(logcE, x), logcF);
}

fn logOf(s: vec3f) -> vec3f {
  let awg = vec3f(row(params.awg[0], s), row(params.awg[1], s), row(params.awg[2], s));
  return clamp(vec3f(logc3(max(awg.x, 0.0)), logc3(max(awg.y, 0.0)), logc3(max(awg.z, 0.0))), vec3f(0.0), vec3f(1.0));
}

fn contrast(s: vec3f) -> vec3f {
  let y = add(add(mul(luma2020.x, s.x), mul(luma2020.y, s.y)), mul(luma2020.z, s.z));
  if (y <= darkest) {
    return s;
  }
  let aim = mul(grey, fastExp2(mul(fastLog2(div(y, grey)), params.slope.x)));
  return vec3f(div(mul(s.x, aim), y), div(mul(s.y, aim), y), div(mul(s.z, aim), y));
}

fn cube(lut: Lut, c: vec3f) -> vec3f {
  let n = f32(lut.size - 1u);
  var base = vec3u(0u);
  var frac = vec3f(0.0);
  for (var k = 0u; k < 3u; k++) {
    var u = c[k];
    if (lut.unit == 0u) {
      u = div(sub(c[k], lut.lo[k]), sub(lut.hi[k], lut.lo[k]));
    }
    let t = mul(clamp(u, 0.0, 1.0), n);
    let lo = min(u32(floor(t)), lut.size - 2u);
    base[k] = lo;
    frac[k] = sub(t, f32(lo));
  }
  let gs = lut.size;
  let bs = lut.size * lut.size;
  let at = base.x + gs * base.y + bs * base.z;
  let o = lut.offset + at;
  let c00 = lerp3(luts[o].xyz, luts[o + 1u].xyz, frac.x);
  let c10 = lerp3(luts[o + gs].xyz, luts[o + gs + 1u].xyz, frac.x);
  let c01 = lerp3(luts[o + bs].xyz, luts[o + bs + 1u].xyz, frac.x);
  let c11 = lerp3(luts[o + bs + gs].xyz, luts[o + bs + gs + 1u].xyz, frac.x);
  return lerp3(lerp3(c00, c10, frac.y), lerp3(c01, c11, frac.y), frac.z);
}

fn display(code: vec3f) -> vec3f {
  let negative = cube(params.negative, clamp(code, vec3f(0.0), vec3f(1.0)));
  return cube(params.print, clamp(negative, vec3f(0.0), vec3f(1.0)));
}

fn scanLinear(v: f32) -> f32 {
  return tableAt(scanAt, 8192u, mul(clamp(v, 0.0, 1.0), 8192.0));
}

fn tapAt(offset: u32, i: u32) -> Tap {
  let at = (offset + i) * 3u;
  return Tap(taps[at], taps[at + 1u], bitcast<f32>(taps[at + 2u]));
}

fn inside(region: vec4u, x: i32, y: i32) -> u32 {
  let cx = u32(clamp(x, i32(region.x), i32(region.x + region.z) - 1)) - region.x;
  let cy = u32(clamp(y, i32(region.y), i32(region.y + region.w) - 1)) - region.y;
  return cy * region.z + cx;
}

fn read3(i: u32) -> vec3f {
  return vec3f(src3[i * 3u], src3[i * 3u + 1u], src3[i * 3u + 2u]);
}

fn write3(i: u32, v: vec3f) {
  dst3[i * 3u] = v.x;
  dst3[i * 3u + 1u] = v.y;
  dst3[i * 3u + 2u] = v.z;
}

fn onGrid(x: f32, at: u32) -> f32 {
  return tableAt(at, gridSteps, mul(clamp(x, 0.0, 1.0), f32(gridSteps)));
}

fn tone(q: f32, d: f32) -> f32 {
  let low = tables[constsAt];
  let high = tables[constsAt + 1u];
  let bend2 = tables[constsAt + 2u];
  let bend3 = tables[constsAt + 3u];
  let u = add(add(add(onGrid(q, baseAt), d), mul(mul(bend2, d), d)), mul(mul(mul(bend3, d), d), d));
  let top = tables[baseAt + gridSteps];
  var out: f32;
  if (u > top) {
    out = add(tables[toneAt + gridSteps], mul(high, sub(u, top)));
  } else if (u < 0.0) {
    out = add(tables[toneAt], mul(low, u));
  } else {
    let i = clamp(ranked(baseAt, gridSteps, baseIndexAt, u), 1u, gridSteps) - 1u;
    let f = div(sub(u, tables[baseAt + i]), sub(tables[baseAt + i + 1u], tables[baseAt + i]));
    out = add(tables[toneAt + i], mul(sub(tables[toneAt + i + 1u], tables[toneAt + i]), f));
  }
  return out;
}

fn toLinear(v: f32) -> f32 {
  if (v < decodeCut) {
    return div(v, slope709);
  }
  return fastExp2(mul(fastLog2(div(add(v, offset709), scale709)), decodePower));
}

fn toCode(l: f32) -> f32 {
  if (l < encodeCut) {
    return mul(slope709, l);
  }
  return sub(mul(scale709, fastExp2(mul(fastLog2(l), gamma709))), offset709);
}

fn knee(s: f32) -> f32 {
  if (s <= kneeStart) {
    return s;
  }
  if (s > tailStart) {
    let e = sub(min(s, tailEnd), tailStart);
    let curve = add(add(tables[constsAt + 6u], mul(tables[constsAt + 7u], e)), mul(mul(tables[constsAt + 8u], e), e));
    return add(curve, mul(tables[constsAt + 9u], max(sub(s, tailEnd), 0.0)));
  }
  return tableAt(kneeAt, kneeSteps, mul(sub(s, kneeStart), 1000.0));
}

fn desaturate(lin: vec3f) -> vec3f {
  let top = max(max(lin.x, lin.y), lin.z);
  if (top <= 0.0) {
    return vec3f(0.0);
  }
  let s = sub(1.0, div(min(min(lin.x, lin.y), lin.z), top));
  var k = 1.0;
  if (s > 0.0) {
    k = div(knee(s), s);
  }
  let y = luma(lin);
  let level = luma(max(lin, vec3f(0.0)));
  return vec3f(add(level, mul(k, sub(lin.x, y))), add(level, mul(k, sub(lin.y, y))), add(level, mul(k, sub(lin.z, y))));
}

fn saturated(cand: vec3f) -> vec3f {
  return saturatedFree(min(cand, vec3f(1.0)));
}

// after the halation the stage takes its values unclipped; only its output is clipped
fn saturatedFree(value: vec3f) -> vec3f {
  let lin = vec3f(toLinear(value.x), toLinear(value.y), toLinear(value.z));
  let soft = desaturate(desaturate(lin));
  return clamp(vec3f(toCode(soft.x), toCode(soft.y), toCode(soft.z)), vec3f(0.0), vec3f(1.0));
}

fn place(id: vec3u) -> vec2i {
  return vec2i(i32(params.dst.x + id.x), i32(params.dst.y + id.y));
}

fn outside(id: vec3u) -> bool {
  return id.x >= params.dst.z || id.y >= params.dst.w;
}

@compute @workgroup_size(16, 16)
fn code(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  let i = id.y * params.dst.z + id.x;
  var c = read3(i);
  if ((params.flags & contrastFlag) != 0u) {
    c = contrast(c);
  }
  var v = logOf(c);
  if ((params.flags & utilityFlag) != 0u) {
    v = cube(params.utility, v);
  }
  write3(i, v);
}

@compute @workgroup_size(16, 16)
fn blurVertical(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  let p = place(id);
  let r = i32(params.radius);
  var sum = vec3f(0.0);
  for (var k = -r; k <= r; k++) {
    let y = clamp(p.y + k, 0, i32(params.bounds.y) - 1);
    let s = read3(inside(params.src, p.x, y));
    let w = tables[weightsAt + u32(k + r)];
    sum = vec3f(add(sum.x, mul(w, s.x)), add(sum.y, mul(w, s.y)), add(sum.z, mul(w, s.z)));
  }
  write3(id.y * params.dst.z + id.x, sum);
}

@compute @workgroup_size(16, 16)
fn blurHorizontal(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  let p = place(id);
  let r = i32(params.radius);
  var sum = vec3f(0.0);
  for (var k = -r; k <= r; k++) {
    let x = clamp(p.x + k, 0, i32(params.bounds.x) - 1);
    let s = read3(inside(params.src, x, p.y));
    let w = tables[weightsAt + u32(k + r)];
    sum = vec3f(add(sum.x, mul(w, s.x)), add(sum.y, mul(w, s.y)), add(sum.z, mul(w, s.z)));
  }
  let i = id.y * params.dst.z + id.x;
  write3(i, sum);
  if ((params.flags & lumaFlag) != 0u) {
    dst1[i] = luma(sum);
  }
}

@compute @workgroup_size(16, 16)
fn lumaPlane(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  let i = id.y * params.dst.z + id.x;
  dst1[i] = luma(read3(i));
}

@compute @workgroup_size(16, 16)
fn reduce(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  let p = place(id);
  let tx = tapAt(params.taps.x, u32(p.x));
  let ty = tapAt(params.taps.y, u32(p.y));
  let left = blend(src1[inside(params.src, i32(tx.lo), i32(ty.lo))], src1[inside(params.src, i32(tx.lo), i32(ty.hi))], ty.f);
  let right = blend(src1[inside(params.src, i32(tx.hi), i32(ty.lo))], src1[inside(params.src, i32(tx.hi), i32(ty.hi))], ty.f);
  let s = blend(left, right, tx.f);
  dst2[id.y * params.dst.z + id.x] = vec2f(s, mul(s, s));
}

fn boxed(sum: vec2f) -> vec2f {
  let n = f32(2u * params.radius + 1u);
  let mean = vec2f(div(sum.x, n), div(sum.y, n));
  if ((params.flags & coefficientsFlag) == 0u) {
    return mean;
  }
  let variance = sub(mean.y, mul(mean.x, mean.x));
  let a = div(variance, add(variance, tables[constsAt + 4u]));
  return vec2f(a, mul(mean.x, sub(1.0, a)));
}

@compute @workgroup_size(16, 16)
fn boxVertical(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  let p = place(id);
  let r = i32(params.radius);
  var sum = vec2f(0.0);
  for (var k = -r; k <= r; k++) {
    let s = src2[inside(params.src, p.x, clamp(p.y + k, 0, i32(params.bounds.y) - 1))];
    sum = vec2f(add(sum.x, s.x), add(sum.y, s.y));
  }
  let n = f32(2u * params.radius + 1u);
  dst2[id.y * params.dst.z + id.x] = vec2f(div(sum.x, n), div(sum.y, n));
}

@compute @workgroup_size(16, 16)
fn boxHorizontal(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  let p = place(id);
  let r = i32(params.radius);
  var sum = vec2f(0.0);
  for (var k = -r; k <= r; k++) {
    let s = src2[inside(params.src, clamp(p.x + k, 0, i32(params.bounds.x) - 1), p.y)];
    sum = vec2f(add(sum.x, s.x), add(sum.y, s.y));
  }
  dst2[id.y * params.dst.z + id.x] = boxed(sum);
}

@compute @workgroup_size(16, 16)
fn detail(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  let p = place(id);
  let tx = tapAt(params.taps.z, u32(p.x));
  let ty = tapAt(params.taps.w, u32(p.y));
  let p00 = src2[inside(params.aux, i32(tx.lo), i32(ty.lo))];
  let p01 = src2[inside(params.aux, i32(tx.lo), i32(ty.hi))];
  let p10 = src2[inside(params.aux, i32(tx.hi), i32(ty.lo))];
  let p11 = src2[inside(params.aux, i32(tx.hi), i32(ty.hi))];
  let a = blend(blend(p00.x, p01.x, ty.f), blend(p10.x, p11.x, ty.f), tx.f);
  let b = blend(blend(p00.y, p01.y, ty.f), blend(p10.y, p11.y, ty.f), tx.f);
  let at = inside(params.src, p.x, p.y);
  let y = src1[at];
  let q = add(mul(a, y), b);
  let yOut = tone(q, sub(y, q));
  let c = read3(at);
  let chroma = tables[constsAt + 5u];
  let out = vec3f(add(yOut, mul(chroma, sub(c.x, y))), add(yOut, mul(chroma, sub(c.y, y))), add(yOut, mul(chroma, sub(c.z, y))));
  if ((params.flags & candidateFlag) != 0u) {
    write3(id.y * params.dst.z + id.x, min(out, vec3f(1.0)));
  } else {
    write3(id.y * params.dst.z + id.x, saturated(out));
  }
}

@compute @workgroup_size(256)
fn dust(@builtin(global_invocation_id) id: vec3u) {
  if (id.x >= params.count) {
    return;
  }
  let k = taps[2u * id.x];
  let s = bitcast<f32>(taps[2u * id.x + 1u]);
  var v = vec3f(0.0);
  for (var c = 0u; c < 3u; c++) {
    let x = dst3[3u * k + c];
    v[c] = exact(x + exact(s * exact(1.0 - x)));
    dst3[3u * k + c] = v[c];
  }
  if ((params.flags & lumaFlag) != 0u) {
    dst1[k] = luma(v);
  }
}

@compute @workgroup_size(256)
fn samples(@builtin(global_invocation_id) id: vec3u) {
  if (id.x >= params.count) {
    return;
  }
  let d = display(read3(id.x * params.aux.x));
  dst1[id.x] = luma(d);
}

@compute @workgroup_size(16, 16)
fn finish(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  let i = id.y * params.dst.z + id.x;
  let d = display(read3(i));
  let lin = vec3f(scanLinear(d.x), scanLinear(d.y), scanLinear(d.z));
  write3(i, vec3f(row(params.rec[0], lin), row(params.rec[1], lin), row(params.rec[2], lin)));
}

fn forward(v: f32) -> f32 {
  return tableAt(domainAt, domainLast, mul(clamp(v, 0.0, 1.25), 5000.0));
}

fn inverse(u: f32) -> f32 {
  if (u <= tables[domainAt]) {
    return 0.0;
  }
  if (u >= tables[domainAt + domainLast]) {
    return 1.25;
  }
  let i = clamp(ranked(domainAt, domainLast, domainIndexAt, u), 1u, domainLast) - 1u;
  let f = div(sub(u, tables[domainAt + i]), sub(tables[domainAt + i + 1u], tables[domainAt + i]));
  return div(add(f32(i), f), 5000.0);
}

fn mirror(i: i32, n: i32) -> i32 {
  var r = i;
  if (i < 0) {
    r = -i - 1;
  } else if (i >= n) {
    r = 2 * n - i - 1;
  }
  return clamp(r, 0, n - 1);
}

fn pcg(v: u32) -> u32 {
  let state = v * 747796405u + 2891336453u;
  let word = ((state >> ((state >> 28u) + 4u)) ^ state) * 277803737u;
  return (word >> 22u) ^ word;
}

fn hashUniform(ix: i32, iy: i32, seed: u32) -> f32 {
  let h = pcg(bitcast<u32>(ix) + pcg(bitcast<u32>(iy) + pcg(seed)));
  return mul(add(f32(h >> 9u), 0.5), step23);
}

fn white(ix: i32, iy: i32, seed: u32) -> f32 {
  let u1 = hashUniform(ix, iy, seed * 2u + 1u);
  let u2 = hashUniform(ix, iy, seed * 2u + 2u);
  let radius = fastExp2(mul(fastLog2(mul(minusTwoLn2, fastLog2(u1))), 0.5));
  return mul(radius, tableAt(cosAt, 4096u, mul(u2, 4096.0)));
}

fn flatten(z: f32) -> f32 {
  return div(sub(z, mul(flattenAmount, mul(mul(z, z), z))), flattenNorm);
}

@compute @workgroup_size(16, 16)
fn defocusVertical(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  let p = place(id);
  let r = i32(params.radius);
  var sum = vec3f(0.0);
  for (var k = -r; k <= r; k++) {
    let s = read3(inside(params.src, p.x, mirror(p.y + k, i32(params.bounds.y))));
    let w = tables[params.aux.z + u32(k + r)];
    sum = vec3f(add(sum.x, mul(w, forward(s.x))), add(sum.y, mul(w, forward(s.y))), add(sum.z, mul(w, forward(s.z))));
  }
  write3(id.y * params.dst.z + id.x, sum);
}

@compute @workgroup_size(16, 16)
fn defocusHorizontal(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  let p = place(id);
  let r = i32(params.radius);
  var sum = vec3f(0.0);
  for (var k = -r; k <= r; k++) {
    let s = read3(inside(params.src, mirror(p.x + k, i32(params.bounds.x)), p.y));
    let w = tables[params.aux.z + u32(k + r)];
    sum = vec3f(add(sum.x, mul(w, s.x)), add(sum.y, mul(w, s.y)), add(sum.z, mul(w, s.z)));
  }
  write3(id.y * params.dst.z + id.x, vec3f(inverse(sum.x), inverse(sum.y), inverse(sum.z)));
}

@compute @workgroup_size(16, 16)
fn grainNoise(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  let x = bitcast<i32>(params.aux.x) + i32(id.x);
  let y = bitcast<i32>(params.aux.y) + i32(id.y);
  dst1[id.y * params.dst.z + id.x] = white(x, y, params.count);
}

@compute @workgroup_size(16, 16)
fn grainVertical(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  let taps = 2u * params.radius + 1u;
  let at = params.aux.z + taps;
  var v = vec3f(0.0);
  for (var t = 0u; t < taps; t++) {
    let n = src1[(id.y + t) * params.src.z + id.x];
    v = vec3f(add(v.x, mul(tables[at + t], n)), add(v.y, mul(tables[at + 2u * taps + t], n)), add(v.z, mul(tables[at + 4u * taps + t], n)));
  }
  write3(id.y * params.dst.z + id.x, v);
}

@compute @workgroup_size(16, 16)
fn grainHorizontal(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  let taps = 2u * params.radius + 1u;
  let at = params.aux.z;
  var sum = vec3f(0.0);
  for (var t = 0u; t < taps; t++) {
    let c = read3(id.y * params.src.z + id.x + t);
    sum = vec3f(add(sum.x, mul(tables[at + t], c.x)), add(sum.y, mul(tables[at + 2u * taps + t], c.y)), add(sum.z, mul(tables[at + 4u * taps + t], c.z)));
  }
  dst1[id.y * params.dst.z + id.x] = flatten(add(add(add(0.0, sum.x), sum.y), sum.z));
}

@compute @workgroup_size(16, 16)
fn softVertical(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  let taps = 2u * params.radius + 1u;
  var sum = 0.0;
  for (var t = 0u; t < taps; t++) {
    sum = add(sum, mul(tables[params.aux.z + t], src1[(id.y + t) * params.src.z + id.x]));
  }
  dst1[id.y * params.dst.z + id.x] = sum;
}

@compute @workgroup_size(16, 16)
fn grainApply(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  var z = src1[id.y * params.src.z + id.x];
  if (params.radius > 0u) {
    let taps = 2u * params.radius + 1u;
    z = 0.0;
    for (var t = 0u; t < taps; t++) {
      z = add(z, mul(tables[params.aux.z + t], src1[id.y * params.src.z + id.x + t]));
    }
  }
  let i = id.y * params.dst.z + id.x;
  let c = read3(i);
  let gain = add(1.0, mul(params.slope.y, sub(z, grainOffset)));
  let out = vec3f(inverse(mul(forward(c.x), gain)), inverse(mul(forward(c.y), gain)), inverse(mul(forward(c.z), gain)));
  write3(i, clamp(out, vec3f(0.0), vec3f(1.0)));
}

// Film Look Creator halation (engine/src/halation.rs): DaVinci Wide Gamut layers in a measured transfer domain,
// three octave blurs per layer, an optional highlights-only weight; every step mirrors the CPU's order

fn transfer(w: f32) -> f32 {
  if (w < 0.0) {
    return add(tables[transferAt], mul(tables[maskAt + 1u], w));
  }
  return tableAt(transferAt, transferSteps, mul(min(w, 1.0), f32(transferSteps)));
}

fn untransfer(u: f32) -> f32 {
  let first = tables[transferAt];
  if (u < first) {
    return div(sub(u, first), tables[maskAt + 1u]);
  }
  let i = clamp(ranked(transferAt, transferSteps, transferIndexAt, u), 1u, transferSteps) - 1u;
  let a = tables[transferAt + i];
  let f = div(sub(u, a), sub(tables[transferAt + i + 1u], a));
  return div(add(f32(i), f), f32(transferSteps));
}

// halation::Headroom: the photo's top stop spread toward a cinema camera's clip for the halation, easing just past
// it, and brought back down the same curve; on, knee, span, range and most sit after the weight totals (params.count)
const headroomEase = 0.1;

fn lifted(v: f32) -> f32 {
  let at = params.count + 3u;
  let knee = tables[at + 1u];
  if (tables[at] == 0.0 || !(v > knee)) {
    return v;
  }
  let t = div(sub(v, knee), tables[at + 2u]);
  var g = t;
  if (t > 1.0) {
    let e = sub(t, 1.0);
    g = add(1.0, div(mul(headroomEase, e), add(headroomEase, e)));
  }
  return add(knee, mul(tables[at + 3u], g));
}

fn lowered(u: f32) -> f32 {
  let at = params.count + 3u;
  let knee = tables[at + 1u];
  if (tables[at] == 0.0 || !(u > knee)) {
    return u;
  }
  let g = div(sub(u, knee), tables[at + 3u]);
  var t = g;
  if (g > 1.0) {
    let e = min(sub(g, 1.0), tables[at + 4u]);
    t = add(1.0, div(mul(headroomEase, e), sub(headroomEase, e)));
  }
  return add(knee, mul(tables[at + 2u], t));
}

fn times(at: u32, v: vec3f) -> vec3f {
  var out = vec3f(0.0);
  for (var j = 0u; j < 3u; j++) {
    let r = at + 3u * j;
    out[j] = add(add(mul(tables[r], v.x), mul(tables[r + 1u], v.y)), mul(tables[r + 2u], v.z));
  }
  return out;
}


fn gray(w: vec3f) -> f32 {
  let mean = div(add(add(w.x, w.y), w.z), 3.0);
  var g = mean;
  var term = 0u;
  for (var a = 0u; a < 3u; a++) {
    g = add(g, mul(tables[maskAt + 2u + term], sub(w[a], mean)));
    term++;
  }
  for (var a = 0u; a < 3u; a++) {
    for (var b = a; b < 3u; b++) {
      g = add(g, mul(tables[maskAt + 2u + term], sub(mul(w[a], w[b]), mul(mean, mean))));
      term++;
    }
  }
  for (var a = 0u; a < 3u; a++) {
    for (var b = a; b < 3u; b++) {
      for (var c = b; c < 3u; c++) {
        g = add(g, mul(tables[maskAt + 2u + term], sub(mul(mul(w[a], w[b]), w[c]), mul(mul(mean, mean), mean))));
        term++;
      }
    }
  }
  return clamp(g, min(min(w.x, w.y), w.z), max(max(w.x, w.y), w.z));
}

// why: the highlight weight decodes c clamped to [0, 1] and encodes its DWG values floored at 0; where c is already
// in (0, 1] and a DWG value positive, those are the layers' own linear values and codes, so they're reused
fn highlight(c: vec3f, lin: vec3f, d: vec3f, code: vec3f) -> f32 {
  var l = lin;
  var same = true;
  for (var k = 0u; k < 3u; k++) {
    if (!(c[k] > 0.0 && c[k] <= 1.0)) {
      l[k] = toLinear(clamp(c[k], 0.0, 1.0));
      same = false;
    }
  }
  var e = d;
  if (!same) {
    e = times(dwgAt, l);
  }
  let floored = max(e, vec3f(0.0));
  var w = code;
  for (var k = 0u; k < 3u; k++) {
    if (!(same && e[k] > 0.0)) {
      w[k] = toCode(floored[k]);
    }
  }
  w = clamp(w, vec3f(0.0), vec3f(1.0));
  let v = mul(tables[maskAt], sub(transfer(gray(w)), tables[transferAt]));
  return mul(v, clamp(v, 0.0, 1.0));
}

@compute @workgroup_size(16, 16)
fn halationLayers(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  let i = id.y * params.dst.z + id.x;
  let read = read3(i);
  let c = vec3f(lifted(read.x), lifted(read.y), lifted(read.z));
  let lin = vec3f(toLinear(c.x), toLinear(c.y), toLinear(c.z));
  let d = times(dwgAt, lin);
  let code = vec3f(toCode(d.x), toCode(d.y), toCode(d.z));
  write3(i, vec3f(transfer(code.x), transfer(code.y), transfer(code.z)));
  if ((params.flags & highlightsFlag) != 0u) {
    dst1[i] = highlight(c, lin, d, code);
  }
}


// the halation pyramid (engine/src/pyramid.rs): the decimation by s (params.count), the level blur and the
// interpolation along one axis, each output sample summing its taps by fused multiply-add in the CPU's order; regions
// are in each grid's own sample coordinates

const verticalFlag = 32u;
const accumulateFlag = 64u;
const addFlag = 256u;
const singleFlag = 512u;

fn fma3(w: f32, v: vec3f, acc: vec3f) -> vec3f {
  return vec3f(fma(w, v.x, acc.x), fma(w, v.y, acc.y), fma(w, v.z, acc.z));
}

fn along(p: vec2i, k: i32) -> u32 {
  if ((params.flags & verticalFlag) != 0u) {
    return inside(params.src, p.x, k);
  }
  return inside(params.src, k, p.y);
}

fn axisOf(p: vec2i) -> i32 {
  return select(p.x, p.y, (params.flags & verticalFlag) != 0u);
}

fn lengthOf() -> i32 {
  return i32(select(params.bounds.x, params.bounds.y, (params.flags & verticalFlag) != 0u));
}

// the highlight mask (singleFlag) holds one channel, read from src1 and written to dst1
fn readAlong(i: u32) -> vec3f {
  if ((params.flags & singleFlag) != 0u) {
    return vec3f(src1[i], 0.0, 0.0);
  }
  return read3(i);
}

// an octave's last axis weighs its samples into the running sum at src1 (addFlag, accumulateFlag), as
// halation::apply's add_into, the weights at aux.w
fn writeAlong(id: vec3u, p: vec2i, v: vec3f) {
  let i = id.y * params.dst.z + id.x;
  if ((params.flags & singleFlag) != 0u) {
    dst1[i] = v.x;
    return;
  }
  if ((params.flags & addFlag) == 0u) {
    write3(i, v);
    return;
  }
  var base = vec3f(0.0);
  if ((params.flags & accumulateFlag) != 0u) {
    let at = 3u * inside(params.taps, p.x, p.y);
    base = vec3f(src1[at], src1[at + 1u], src1[at + 2u]);
  }
  let w = params.aux.w;
  write3(i, vec3f(fma(tables[w], v.x, base.x), fma(tables[w + 1u], v.y, base.y), fma(tables[w + 2u], v.z, base.z)));
}

@compute @workgroup_size(16, 16)
fn pyramidReduce(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  let p = place(id);
  let m = axisOf(p);
  let n = lengthOf();
  let s = i32(params.count);
  let r = i32(params.radius);
  var acc = vec3f(0.0);
  for (var t = 0; t <= 2 * r; t++) {
    acc = fma3(tables[params.aux.z + u32(t)], readAlong(along(p, mirror(s * m + t - r, n))), acc);
  }
  writeAlong(id, p, acc);
}

@compute @workgroup_size(16, 16)
fn pyramidBlur(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  let p = place(id);
  let m = axisOf(p);
  let n = lengthOf();
  let r = i32(params.radius);
  var acc = vec3f(0.0);
  for (var t = 0; t <= 2 * r; t++) {
    acc = fma3(tables[params.aux.z + u32(t)], readAlong(along(p, mirror(m + t - r, n))), acc);
  }
  writeAlong(id, p, acc);
}

@compute @workgroup_size(16, 16)
fn pyramidExpand(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  let p = place(id);
  let q = axisOf(p);
  let n = lengthOf();
  let s = i32(params.count);
  let m0 = q / s;
  let phase = u32(q - m0 * s);
  var acc = vec3f(0.0);
  for (var k = 0; k < 4; k++) {
    acc = fma3(tables[params.aux.z + 4u * phase + u32(k)], readAlong(along(p, mirror(m0 + k - 1, n))), acc);
  }
  writeAlong(id, p, acc);
}

@compute @workgroup_size(16, 16)
fn pyramidCombine(@builtin(global_invocation_id) id: vec3u) {
  if (outside(id)) {
    return;
  }
  let p = place(id);
  let at = 3u * inside(params.src, p.x, p.y);
  let base = vec3f(src1[at], src1[at + 1u], src1[at + 2u]);
  let s = read3(inside(params.aux, p.x, p.y));
  var m = 1.0;
  if ((params.flags & highlightsFlag) != 0u) {
    m = min(bitcast<f32>(taps[inside(params.taps, p.x, p.y)]), 1.0);
  }
  let total = vec3f(tables[params.count], tables[params.count + 1u], tables[params.count + 2u]);
  var acc = vec3f(0.0);
  for (var c = 0u; c < 3u; c++) {
    acc[c] = add(base[c], mul(m, sub(s[c], mul(total[c], base[c]))));
  }
  let lin = times(dwgAt + 9u, vec3f(toLinear(untransfer(acc.x)), toLinear(untransfer(acc.y)), toLinear(untransfer(acc.z))));
  write3(id.y * params.dst.z + id.x, saturatedFree(vec3f(lowered(toCode(lin.x)), lowered(toCode(lin.y)), lowered(toCode(lin.z)))));
}
