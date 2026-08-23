use anyhow::{ensure, Result};
use half::f16;
use half::slice::HalfFloatSliceExt;

const CHUNK: usize = 4096;

#[derive(Clone, Copy)]
pub(crate) enum Quantization {
    Truncate,
    RoundNearest,
}

#[derive(Clone, Copy)]
pub(crate) struct NchwCrop {
    pub(crate) source_height: usize,
    pub(crate) source_width: usize,
    pub(crate) output_height: usize,
    pub(crate) output_width: usize,
}

impl NchwCrop {
    pub(crate) const fn full(height: usize, width: usize) -> Self {
        Self {
            source_height: height,
            source_width: width,
            output_height: height,
            output_width: width,
        }
    }
}

pub(crate) fn f16_nchw_to_rgb(
    values: &[f16],
    crop: NchwCrop,
    quantization: Quantization,
) -> Result<Vec<u8>> {
    let plane_size = validate_layout(values.len(), crop)?;
    let channels = [
        &values[..plane_size],
        &values[plane_size..2 * plane_size],
        &values[2 * plane_size..],
    ];
    let mut channel_buffers = [[0.0_f32; CHUNK]; 3];
    let mut rgb = vec![0_u8; crop.output_height * crop.output_width * 3];

    for y in 0..crop.output_height {
        let source_row = y * crop.source_width;
        let output_row = y * crop.output_width;
        let mut x = 0;
        while x < crop.output_width {
            let len = CHUNK.min(crop.output_width - x);
            for channel in 0..3 {
                channels[channel][source_row + x..source_row + x + len]
                    .convert_to_f32_slice(&mut channel_buffers[channel][..len]);
            }
            write_rgb_chunk(
                &channel_buffers,
                &mut rgb[(output_row + x) * 3..(output_row + x + len) * 3],
                len,
                quantization,
            );
            x += len;
        }
    }

    Ok(rgb)
}

pub(crate) fn f16_bits_nchw_to_rgb(
    values: &[u16],
    crop: NchwCrop,
    quantization: Quantization,
) -> Result<Vec<u8>> {
    let plane_size = validate_layout(values.len(), crop)?;
    let channels = [
        &values[..plane_size],
        &values[plane_size..2 * plane_size],
        &values[2 * plane_size..],
    ];
    let mut f16_buffers = [[f16::ZERO; CHUNK]; 3];
    let mut channel_buffers = [[0.0_f32; CHUNK]; 3];
    let mut rgb = vec![0_u8; crop.output_height * crop.output_width * 3];

    for y in 0..crop.output_height {
        let source_row = y * crop.source_width;
        let output_row = y * crop.output_width;
        let mut x = 0;
        while x < crop.output_width {
            let len = CHUNK.min(crop.output_width - x);
            for channel in 0..3 {
                for (sample, bits) in f16_buffers[channel][..len]
                    .iter_mut()
                    .zip(&channels[channel][source_row + x..source_row + x + len])
                {
                    *sample = f16::from_bits(*bits);
                }
                f16_buffers[channel][..len]
                    .convert_to_f32_slice(&mut channel_buffers[channel][..len]);
            }
            write_rgb_chunk(
                &channel_buffers,
                &mut rgb[(output_row + x) * 3..(output_row + x + len) * 3],
                len,
                quantization,
            );
            x += len;
        }
    }

    Ok(rgb)
}

fn validate_layout(value_len: usize, crop: NchwCrop) -> Result<usize> {
    ensure!(
        crop.output_height <= crop.source_height && crop.output_width <= crop.source_width,
        "FP16 NCHW crop {}x{} exceeds source {}x{}",
        crop.output_width,
        crop.output_height,
        crop.source_width,
        crop.source_height
    );
    let plane_size = crop.source_height * crop.source_width;
    let expected = plane_size * 3;
    ensure!(
        value_len == expected,
        "FP16 NCHW length mismatch: expected {expected}, got {value_len}"
    );
    Ok(plane_size)
}

fn write_rgb_chunk(
    channels: &[[f32; CHUNK]; 3],
    rgb: &mut [u8],
    len: usize,
    quantization: Quantization,
) {
    for pixel in 0..len {
        rgb[pixel * 3] = quantize(channels[0][pixel], quantization);
        rgb[pixel * 3 + 1] = quantize(channels[1][pixel], quantization);
        rgb[pixel * 3 + 2] = quantize(channels[2][pixel], quantization);
    }
}

fn quantize(value: f32, quantization: Quantization) -> u8 {
    let scaled = match quantization {
        Quantization::Truncate => value * 255.0,
        Quantization::RoundNearest => value.mul_add(255.0, 0.5),
    };
    scaled.clamp(0.0, 255.0) as u8
}
