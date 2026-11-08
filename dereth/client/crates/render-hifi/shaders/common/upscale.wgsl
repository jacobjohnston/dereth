// Enlarging a picture by its edges and sharpening it by its contrast, modelled on the two passes
// of AMD's FidelityFX Super Resolution 1 as AMD describes them: EASU, a Lanczos-like filter of
// twelve texels stretched along the edge it finds, and RCAS, a sharpening of four neighbours as
// strong as it can be without leaving their range. Written here from those descriptions.

// The brightness the analysis works on: perceptual, so a picture in linear light is brought
// near one first.
fn upscale_luma(c: vec3<f32>, linear: bool) -> f32 {
    let l = dot(max(c, vec3<f32>(0.0)), vec3<f32>(0.25, 0.5, 0.25));
    return select(l, sqrt(l), linear);
}

// One of the four texels around the place, centre `c` with its neighbours left, right, up and
// down, weighted `w`: its gradient, and in z how much it is an edge (1, the brightness steps one
// way) rather than a line (0, it turns back).
fn easu_feature(w: f32, l: f32, c: f32, r: f32, u: f32, d: f32) -> vec3<f32> {
    let gx = r - l;
    let gy = d - u;
    let ex = clamp(abs(gx) / max(max(abs(r - c), abs(c - l)), 1e-5), 0.0, 1.0);
    let ey = clamp(abs(gy) / max(max(abs(d - c), abs(c - u)), 1e-5), 0.0, 1.0);
    return vec3<f32>(gx, gy, ex * ex + ey * ey) * w;
}

// `src` (texels 0..`hi`) at `at`, a place in its texels with their centres at a half: twelve
// texels round it filtered by a windowed Lanczos 2, turned to the edge found in the four
// nearest and stretched along it, and held to those four's range so no edge rings.
fn easu(src: texture_2d<f32>, at: vec2<f32>, hi: vec2<i32>, linear: bool) -> vec3<f32> {
    let pp = at - vec2<f32>(0.5);
    let base = vec2<i32>(floor(pp));
    let f = pp - floor(pp);
    //     0 1
    //   2 3 4 5
    //   6 7 8 9
    //    10 11
    var offsets = array<vec2<i32>, 12>(
        vec2<i32>(0, -1), vec2<i32>(1, -1),
        vec2<i32>(-1, 0), vec2<i32>(0, 0), vec2<i32>(1, 0), vec2<i32>(2, 0),
        vec2<i32>(-1, 1), vec2<i32>(0, 1), vec2<i32>(1, 1), vec2<i32>(2, 1),
        vec2<i32>(0, 2), vec2<i32>(1, 2),
    );
    var colour: array<vec3<f32>, 12>;
    var luma: array<f32, 12>;
    for (var i = 0; i < 12; i = i + 1) {
        let c = textureLoad(src, clamp(base + offsets[i], vec2<i32>(0), hi), 0).rgb;
        colour[i] = c;
        luma[i] = upscale_luma(c, linear);
    }
    // The four nearest, weighted by how near.
    let feature = easu_feature((1.0 - f.x) * (1.0 - f.y), luma[2], luma[3], luma[4], luma[0], luma[7])
        + easu_feature(f.x * (1.0 - f.y), luma[3], luma[4], luma[5], luma[1], luma[8])
        + easu_feature((1.0 - f.x) * f.y, luma[6], luma[7], luma[8], luma[3], luma[10])
        + easu_feature(f.x * f.y, luma[7], luma[8], luma[9], luma[4], luma[11]);
    var dir = feature.xy;
    let dd = dot(dir, dir);
    if (dd < 1.0 / 32768.0) {
        dir = vec2<f32>(1.0, 0.0);
    } else {
        dir = dir * inverseSqrt(dd);
    }
    var edge = feature.z * 0.5;
    edge = edge * edge;
    // Along the edge the kernel stretches, the more on a diagonal; across it, it narrows; and
    // its negative lobe shrinks the more it is an edge.
    let stretch = 1.0 / max(abs(dir.x), abs(dir.y));
    let axes = vec2<f32>(1.0 + (stretch - 1.0) * edge, 1.0 - 0.5 * edge);
    let window = 0.5 - 0.29 * edge;
    let reach = 1.0 / window;
    var sum = vec3<f32>(0.0);
    var total = 0.0;
    for (var i = 0; i < 12; i = i + 1) {
        let v = vec2<f32>(offsets[i]) - f;
        let r = vec2<f32>(dot(v, dir), dot(v, vec2<f32>(-dir.y, dir.x))) * axes;
        let d2 = min(dot(r, r), reach);
        // Lanczos 2 by a polynomial, (25/16 (2/5 d² - 1)² - 9/16) (window d² - 1)².
        let lobe = 0.4 * d2 - 1.0;
        let win = window * d2 - 1.0;
        let w = (25.0 / 16.0 * lobe * lobe - 9.0 / 16.0) * win * win;
        sum = sum + colour[i] * w;
        total = total + w;
    }
    let lo = min(min(colour[3], colour[4]), min(colour[7], colour[8]));
    let top = max(max(colour[3], colour[4]), max(colour[7], colour[8]));
    return clamp(sum / max(total, 1e-5), lo, top);
}

// The most an RCAS lobe may take from each neighbour.
const RCAS_LIMIT: f32 = 0.1875;

// `src` (texels 0..`hi`) at `p`, sharpened by its four neighbours: the most negative weight on
// them that keeps the result inside 0..1 for their darkest and brightest, by `sharpness` (1 the
// most), and less where the centre stands out of them alone, as noise does.
fn rcas(src: texture_2d<f32>, p: vec2<i32>, hi: vec2<i32>, sharpness: f32) -> vec3<f32> {
    let e = textureLoad(src, clamp(p, vec2<i32>(0), hi), 0).rgb;
    let b = textureLoad(src, clamp(p + vec2<i32>(0, -1), vec2<i32>(0), hi), 0).rgb;
    let d = textureLoad(src, clamp(p + vec2<i32>(-1, 0), vec2<i32>(0), hi), 0).rgb;
    let f = textureLoad(src, clamp(p + vec2<i32>(1, 0), vec2<i32>(0), hi), 0).rgb;
    let h = textureLoad(src, clamp(p + vec2<i32>(0, 1), vec2<i32>(0), hi), 0).rgb;
    let ring_lo = min(min(b, d), min(f, h));
    let ring_hi = max(max(b, d), max(f, h));
    // Darker than black from the darkest, brighter than white from the brightest.
    let from_dark = -ring_lo / max(4.0 * ring_hi, vec3<f32>(1e-5));
    let from_bright = (vec3<f32>(1.0) - ring_hi) / min(4.0 * ring_lo - vec3<f32>(4.0), vec3<f32>(-1e-5));
    let each = max(from_dark, from_bright);
    var lobe = max(-RCAS_LIMIT, min(max(each.r, max(each.g, each.b)), 0.0)) * sharpness;
    let lb = upscale_luma(b, false);
    let ld = upscale_luma(d, false);
    let le = upscale_luma(e, false);
    let lf = upscale_luma(f, false);
    let lh = upscale_luma(h, false);
    let spread = max(max(max(lb, ld), max(lf, lh)), le) - min(min(min(lb, ld), min(lf, lh)), le);
    let noise = clamp(abs(0.25 * (lb + ld + lf + lh) - le) / max(spread, 1e-5), 0.0, 1.0);
    lobe = lobe * (1.0 - 0.5 * noise);
    return (e + lobe * (b + d + f + h)) / (1.0 + 4.0 * lobe);
}
