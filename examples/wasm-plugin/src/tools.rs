//! AI tools, luma histogram video scope, CPU visual effects, and expression driver.

use serde_json::{Value, json};

use crate::commands::run_add_marker;
use crate::{
    AiToolImage, AiToolOutput, DEFAULT_MARKER_NAME, DriverVars, EffectParam, ParamValue,
    ScopeDrawing, ScopeFrame, VideoFrame,
};

/// The id of the luma histogram scope.
pub(crate) const LUMA_SCOPE: &str = "example.luma-histogram";
/// Columns of the luma histogram: four code values each.
pub(crate) const LUMA_BINS: u32 = 64;
/// Rows of the luma histogram.
pub(crate) const LUMA_HEIGHT: u32 = 32;
/// The colour of a luma histogram bar.
const LUMA_BAR: [u8; 4] = [64, 200, 190, 255];

/// A 1x1 RGBA PNG, returned by the `example_swatch` AI tool.
const SWATCH_PNG: [u8; 70] = [
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0,
    0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 208, 170, 191, 242, 31, 0,
    4, 209, 2, 125, 61, 100, 161, 106, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

pub(crate) fn ai_tool_call(name: String, arguments_json: String) -> Result<AiToolOutput, String> {
    match name.as_str() {
        "example_add_marker" => {
            let arguments: Value =
                serde_json::from_str(&arguments_json).map_err(|err| err.to_string())?;
            let frame = arguments
                .get("frame")
                .and_then(Value::as_i64)
                .ok_or("frame must be a whole number")?;
            let name = arguments
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_MARKER_NAME);
            run_add_marker(&json!({ "frame": frame, "name": name }).to_string())?;
            Ok(AiToolOutput {
                text: format!("added marker {name} at frame {frame}"),
                images: Vec::new(),
            })
        }
        "example_swatch" => Ok(AiToolOutput {
            text: "the example swatch".to_owned(),
            images: vec![AiToolImage {
                label: "swatch".to_owned(),
                png: SWATCH_PNG.to_vec(),
            }],
        }),
        other => Err(format!("no AI tool {other}")),
    }
}

pub(crate) fn scope_measure(id: String, frame: ScopeFrame) -> Result<ScopeDrawing, String> {
    if id != LUMA_SCOPE {
        return Err(format!("no scope {id}"));
    }
    Ok(luma_histogram(&frame))
}

pub(crate) fn cpu_effect_process(
    id: String,
    params: Vec<EffectParam>,
    mut frame: VideoFrame,
) -> Result<VideoFrame, String> {
    if id == "example.meter" {
        let level = f32_to_f16(params.first().copied().unwrap_or(0.0));
        for px in frame.pixels.chunks_exact_mut(4) {
            px.copy_from_slice(&[level, level, level, f32_to_f16(1.0)]);
        }
        return Ok(frame);
    }
    if id != "example.cpu-invert" {
        return Err(format!("unknown CPU effect {id}"));
    }
    let amount = params.first().copied().unwrap_or(1.0);
    if amount < 0.0 {
        panic!("explicit CPU effect trap");
    }
    if amount > 1.0 {
        return Err("cpu effect guest error".to_owned());
    }
    if amount == 0.0 {
        return Ok(frame);
    }
    for px in frame.pixels.chunks_exact_mut(4) {
        for ch in &mut px[..3] {
            let v = f16_to_f32(*ch);
            *ch = f32_to_f16(v + (1.0 - 2.0 * v) * amount);
        }
    }
    Ok(frame)
}

pub(crate) fn driver_evaluate(
    id: String,
    expr: String,
    vars: DriverVars,
) -> Result<ParamValue, String> {
    if id != "wasm-expr" {
        return Err(format!("unknown driver language {id}"));
    }
    match expr.trim() {
        "double_time" => Ok(ParamValue::Scalar((vars.time * 2.0) as f32)),
        "plus_ten" => match vars.value {
            ParamValue::Scalar(v) => Ok(ParamValue::Scalar(v + 10.0)),
            other => Ok(other),
        },
        "canvas_vec" => Ok(ParamValue::Vec2((
            vars.canvas_width as f32,
            vars.canvas_height as f32,
        ))),
        "trap" => panic!("explicit driver trap"),
        other => Err(format!("unknown wasm-expr: {other}")),
    }
}

const fn f16_to_f32(bits: u16) -> f32 {
    let sign = ((bits & 0x8000) as u32) << 16;
    let exp = ((bits >> 10) & 0x1f) as u32;
    let mant = (bits & 0x03ff) as u32;
    if exp == 0 {
        if mant == 0 {
            return f32::from_bits(sign);
        }
        let shift = mant.leading_zeros() - 21;
        let mant = (mant << shift) & 0x03ff;
        let exp = (127 - 15 + 1) - shift;
        f32::from_bits(sign | (exp << 23) | (mant << 13))
    } else if exp == 0x1f {
        f32::from_bits(sign | 0x7f80_0000 | (mant << 13))
    } else {
        f32::from_bits(sign | ((exp + (127 - 15)) << 23) | (mant << 13))
    }
}

const fn f32_to_f16(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let mant = bits & 0x007f_ffff;
    if exp == 0xff {
        return if mant == 0 {
            sign | 0x7c00
        } else {
            sign | 0x7e00 | ((mant >> 13) as u16)
        };
    }
    let half_exp = exp - 127 + 15;
    if half_exp >= 0x1f {
        return sign | 0x7c00;
    }
    if half_exp <= 0 {
        if half_exp < -10 {
            return sign;
        }
        let full_mant = mant | 0x0080_0000;
        let shift = (14 - half_exp) as u32;
        let round_bit = 1u32 << (shift - 1);
        let mask = round_bit - 1;
        let mut half_mant = (full_mant >> shift) as u16;
        if (full_mant & round_bit) != 0 && ((full_mant & mask) != 0 || (half_mant & 1) != 0) {
            half_mant += 1;
        }
        return sign | half_mant;
    }
    let mut half_mant = (mant >> 13) as u16;
    if (mant & 0x1000) != 0 && ((mant & 0x0fff) != 0 || (half_mant & 1) != 0) {
        half_mant += 1;
    }
    sign | (((half_exp as u16) << 10) + half_mant)
}

/// Bin every pixel's Rec. 709 luma into [`LUMA_BINS`] columns and draw each
/// column's count as a bar, scaled so the fullest bin reaches the top.
fn luma_histogram(frame: &ScopeFrame) -> ScopeDrawing {
    let bins = LUMA_BINS as usize;
    let mut counts = vec![0_u32; bins];
    let mut total = 0_u64;
    for px in frame.pixels.chunks_exact(4) {
        let luma =
            (2126 * u32::from(px[0]) + 7152 * u32::from(px[1]) + 722 * u32::from(px[2])) / 10_000;
        total += u64::from(luma);
        counts[luma as usize * bins / 256] += 1;
    }
    let height = LUMA_HEIGHT as usize;
    let mut pixels = vec![0_u8; bins * height * 4];
    let fullest = counts.iter().copied().max().unwrap_or(0).max(1);
    for (column, count) in counts.iter().enumerate() {
        let rows = (*count as usize * height).div_ceil(fullest as usize);
        for row in height - rows..height {
            let at = (row * bins + column) * 4;
            pixels[at..at + 4].copy_from_slice(&LUMA_BAR);
        }
    }
    let samples = (frame.pixels.len() / 4).max(1) as u64;
    ScopeDrawing {
        pixels,
        readout: format!("mean luma {}", total / samples),
    }
}
