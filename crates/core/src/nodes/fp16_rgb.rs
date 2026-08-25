use std::sync::LazyLock;

use anyhow::{anyhow, ensure, Result};
use half::f16;
use half::slice::HalfFloatSliceExt;
use rayon::prelude::*;
use rayon::{ThreadPool, ThreadPoolBuilder};

const CHUNK: usize = 4096;
const PARALLEL_BANDS: usize = 2;
static RGB_CONVERSION_POOL: LazyLock<std::result::Result<ThreadPool, String>> =
    LazyLock::new(|| {
        ThreadPoolBuilder::new()
            .num_threads(PARALLEL_BANDS)
            .thread_name(|index| format!("fp16-rgb-{index}"))
            .build()
            .map_err(|error| error.to_string())
    });

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
    let mut rgb = vec![0_u8; crop.output_height * crop.output_width * 3];
    if rgb.is_empty() {
        return Ok(rgb);
    }
    let row_bytes = crop.output_width * 3;
    let rows_per_band = crop.output_height.div_ceil(PARALLEL_BANDS);

    let pool = RGB_CONVERSION_POOL
        .as_ref()
        .map_err(|error| anyhow!("failed to initialize FP16 RGB worker pool: {error}"))?;
    pool.install(|| {
        rgb.par_chunks_mut(rows_per_band * row_bytes)
            .enumerate()
            .for_each(|(band, output_band)| {
                let mut channel_buffers = [[0.0_f32; CHUNK]; 3];
                for (row, output_row) in output_band.chunks_mut(row_bytes).enumerate() {
                    let source_row = (band * rows_per_band + row) * crop.source_width;
                    let mut x = 0;
                    while x < crop.output_width {
                        let len = CHUNK.min(crop.output_width - x);
                        for channel in 0..3 {
                            channels[channel][source_row + x..source_row + x + len]
                                .convert_to_f32_slice(&mut channel_buffers[channel][..len]);
                        }
                        write_rgb_chunk(
                            &channel_buffers,
                            &mut output_row[x * 3..(x + len) * 3],
                            len,
                            quantization,
                        );
                        x += len;
                    }
                }
            });
    });

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f16_conversion_preserves_row_order_and_truncation() {
        let values = [
            f16::from_f32(0.0),
            f16::from_f32(0.5),
            f16::from_f32(1.0),
            f16::from_f32(0.25),
            f16::from_f32(1.0),
            f16::from_f32(0.0),
            f16::from_f32(0.5),
            f16::from_f32(0.75),
            f16::from_f32(0.25),
            f16::from_f32(0.5),
            f16::from_f32(0.75),
            f16::from_f32(1.0),
        ];

        let rgb = f16_nchw_to_rgb(&values, NchwCrop::full(2, 2), Quantization::Truncate)
            .expect("valid NCHW input should convert");

        assert_eq!(rgb, [0, 255, 63, 127, 0, 127, 255, 127, 191, 63, 191, 255]);
    }

    #[test]
    fn f16_conversion_crops_padded_rows_and_columns() {
        let channel = [
            f16::from_f32(0.0),
            f16::from_f32(0.25),
            f16::from_f32(1.0),
            f16::from_f32(0.5),
            f16::from_f32(0.75),
            f16::from_f32(1.0),
            f16::from_f32(0.125),
            f16::from_f32(0.375),
            f16::from_f32(0.625),
        ];
        let values = [channel, channel, channel].concat();

        let rgb = f16_nchw_to_rgb(
            &values,
            NchwCrop {
                source_height: 3,
                source_width: 3,
                output_height: 3,
                output_width: 2,
            },
            Quantization::Truncate,
        )
        .expect("valid cropped NCHW input should convert");

        assert_eq!(
            rgb,
            [0, 0, 0, 63, 63, 63, 127, 127, 127, 191, 191, 191, 31, 31, 31, 95, 95, 95,]
        );
    }

    #[test]
    fn f16_conversion_accepts_empty_output_dimensions() {
        let zero_height = f16_nchw_to_rgb(&[], NchwCrop::full(0, 2), Quantization::Truncate)
            .expect("zero-height output should remain valid");
        let zero_width = f16_nchw_to_rgb(&[], NchwCrop::full(2, 0), Quantization::Truncate)
            .expect("zero-width output should remain valid");

        assert!(zero_height.is_empty());
        assert!(zero_width.is_empty());
    }
}
