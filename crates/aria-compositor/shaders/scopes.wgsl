// Scope counts over one premultiplied Rgba8Unorm frame: the red, green, blue,
// and luma histograms, the sampled count, the waveform of one to three
// channels, and the vectorscope.
//
// The arithmetic is the fixed point `aria_analysis::scopes::measure` counts
// with on the CPU: straight colour by `RgbaImage::unpremultiply`, luma by
// `luma709`, Cb and Cr codes by `chroma_codes709`, and the vectorscope cell by
// `vectorscope_cell`. Both read the same pixels, every `stride`-th of every
// `stride`-th row, so both count the same bins.

struct Grid {
    width: u32,
    height: u32,
    stride: u32,
    columns: u32,
    size: u32,
    // Waveform channels counted, 1 to 3, numbered as `ScopeChannel`.
    channels: u32,
    channel0: u32,
    channel1: u32,
    channel2: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var<uniform> grid: Grid;
// Red, green, blue, and luma histograms of 256 counts each, then the sampled
// count, then each waveform channel column after column, then the vectorscope
// top row first.
@group(0) @binding(2) var<storage, read_write> counts: array<atomic<u32>>;

const BINS: u32 = 256u;
const SAMPLED: u32 = 1024u;
const HEAD: u32 = 1025u;
const CHROMA_ONE: u32 = 8355840u;
const CHROMA_HALF: u32 = 4177920u;

// The histograms and the sampled count for one workgroup, added to `counts`
// once at the end, so a flat frame does not send every invocation to one
// global bin.
var<workgroup> local: array<atomic<u32>, 1025>;

fn straight(premultiplied: u32, alpha: u32) -> u32 {
    if alpha == 255u {
        return premultiplied;
    }
    return min((premultiplied * 255u + alpha / 2u) / alpha, 255u);
}

// `offset` is in [0, CHROMA_ONE]; `offset * last` stays within u32 for every
// `last` up to 511.
fn cell(offset: u32, last: u32) -> u32 {
    return min((offset * last + CHROMA_HALF) / CHROMA_ONE, last);
}

// The waveform code of `channel`: `offset * 255` stays within u32.
fn channel_code(channel: u32, rgb: vec3<u32>, luma: u32, right: u32, up: u32) -> u32 {
    switch channel {
        case 1u: { return rgb.r; }
        case 2u: { return rgb.g; }
        case 3u: { return rgb.b; }
        case 4u: { return (right * 255u + CHROMA_HALF) / CHROMA_ONE; }
        case 5u: { return (up * 255u + CHROMA_HALF) / CHROMA_ONE; }
        default: { return luma; }
    }
}

@compute @workgroup_size(16, 16)
fn cs(
    @builtin(global_invocation_id) id: vec3<u32>,
    @builtin(local_invocation_index) lane: u32,
) {
    for (var i = lane; i < HEAD; i = i + 256u) {
        atomicStore(&local[i], 0u);
    }
    workgroupBarrier();
    let x = id.x * grid.stride;
    let y = id.y * grid.stride;
    if x < grid.width && y < grid.height {
        let texel = textureLoad(frame, vec2<u32>(x, y), 0);
        let code = vec4<u32>(round(clamp(texel, vec4<f32>(0.0), vec4<f32>(1.0)) * 255.0));
        let alpha = code.a;
        if alpha != 0u {
            let r = straight(code.r, alpha);
            let g = straight(code.g, alpha);
            let b = straight(code.b, alpha);
            let luma = (13933u * r + 46871u * g + 4732u * b + 32768u) >> 16u;
            atomicAdd(&local[r], 1u);
            atomicAdd(&local[BINS + g], 1u);
            atomicAdd(&local[2u * BINS + b], 1u);
            atomicAdd(&local[3u * BINS + luma], 1u);
            atomicAdd(&local[SAMPLED], 1u);
            // Cb + 1/2 and Cr + 1/2, each in [0, CHROMA_ONE].
            let right = (16384u * b + CHROMA_HALF) - (3754u * r + 12630u * g);
            let up = (16384u * r + CHROMA_HALF) - (14882u * g + 1502u * b);
            let column = x * grid.columns / grid.width;
            let plane = grid.columns * BINS;
            var selected = array<u32, 3>(grid.channel0, grid.channel1, grid.channel2);
            for (var i = 0u; i < grid.channels; i = i + 1u) {
                let bin = channel_code(selected[i], vec3<u32>(r, g, b), luma, right, up);
                atomicAdd(&counts[HEAD + i * plane + column * BINS + bin], 1u);
            }
            let last = grid.size - 1u;
            let vectorscope = HEAD + grid.channels * plane;
            let down = CHROMA_ONE - up;
            atomicAdd(&counts[vectorscope + cell(down, last) * grid.size + cell(right, last)], 1u);
        }
    }
    workgroupBarrier();
    for (var i = lane; i < HEAD; i = i + 256u) {
        let n = atomicLoad(&local[i]);
        if n != 0u {
            atomicAdd(&counts[i], n);
        }
    }
}
