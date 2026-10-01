//! Copies between three-channel NCHW planes of different sizes.
//!
//! Inference stages pad frames to model-aligned sizes and crop the outputs
//! back. These helpers write into caller-provided (usually pooled) buffers
//! and overwrite every output element.

use anyhow::{ensure, Result};

const CHANNELS: usize = 3;

/// Copies the top-left `out_h`x`out_w` region of three `plane_h`x`plane_w`
/// planes in `planes` into `output`.
pub(crate) fn crop_planes_into<T: Copy>(
    planes: &[T],
    plane_h: usize,
    plane_w: usize,
    out_h: usize,
    out_w: usize,
    output: &mut [T],
) -> Result<()> {
    ensure!(
        planes.len() == CHANNELS * plane_h * plane_w && out_h <= plane_h && out_w <= plane_w,
        "cannot crop {out_w}x{out_h} from {} samples of {plane_w}x{plane_h} planes",
        planes.len()
    );
    ensure!(
        output.len() == CHANNELS * out_h * out_w,
        "crop output holds {} samples, expected {}",
        output.len(),
        CHANNELS * out_h * out_w
    );
    for channel in 0..CHANNELS {
        let source_plane = &planes[channel * plane_h * plane_w..][..plane_h * plane_w];
        let output_plane = &mut output[channel * out_h * out_w..][..out_h * out_w];
        if out_w == plane_w {
            output_plane.copy_from_slice(&source_plane[..out_h * out_w]);
            continue;
        }
        for (y, output_row) in output_plane.chunks_exact_mut(out_w).enumerate() {
            output_row.copy_from_slice(&source_plane[y * plane_w..][..out_w]);
        }
    }
    Ok(())
}

/// Copies three `h`x`w` planes into the top-left of three `padded_h`x`padded_w`
/// planes and fills the extra rows and columns by mirroring the edge
/// (row `h + i` repeats row `h - 1 - i`; columns likewise).
pub(crate) fn reflect_pad_planes_into<T: Copy>(
    planes: &[T],
    h: usize,
    w: usize,
    padded_h: usize,
    padded_w: usize,
    output: &mut [T],
) -> Result<()> {
    ensure!(
        planes.len() == CHANNELS * h * w,
        "expected {} samples for 3x{h}x{w} planes, got {}",
        CHANNELS * h * w,
        planes.len()
    );
    ensure!(
        (h..=2 * h).contains(&padded_h) && (w..=2 * w).contains(&padded_w),
        "cannot reflect-pad {w}x{h} planes to {padded_w}x{padded_h}"
    );
    ensure!(
        output.len() == CHANNELS * padded_h * padded_w,
        "padded output holds {} samples, expected {}",
        output.len(),
        CHANNELS * padded_h * padded_w
    );
    for channel in 0..CHANNELS {
        let source_plane = &planes[channel * h * w..][..h * w];
        let output_plane = &mut output[channel * padded_h * padded_w..][..padded_h * padded_w];
        for (y, output_row) in output_plane.chunks_exact_mut(padded_w).enumerate() {
            let source_y = if y < h { y } else { 2 * h - 1 - y };
            let source_row = &source_plane[source_y * w..][..w];
            output_row[..w].copy_from_slice(source_row);
            for x in w..padded_w {
                output_row[x] = source_row[2 * w - 1 - x];
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference reflection: index `i` past the edge maps to `2 * len - 1 - i`.
    fn reflect(index: usize, len: usize) -> usize {
        if index < len {
            index
        } else {
            2 * len - 1 - index
        }
    }

    fn planes(h: usize, w: usize) -> Vec<u32> {
        (0..(CHANNELS * h * w) as u32).collect()
    }

    #[test]
    fn crop_takes_the_top_left_region_of_each_plane() {
        let source = planes(4, 5);
        let mut output = vec![u32::MAX; CHANNELS * 3 * 2];

        crop_planes_into(&source, 4, 5, 3, 2, &mut output).unwrap();

        let expected: Vec<u32> = (0..CHANNELS)
            .flat_map(|c| {
                (0..3).flat_map(move |y| (0..2).map(move |x| (c * 20 + y * 5 + x) as u32))
            })
            .collect();
        assert_eq!(output, expected);
    }

    #[test]
    fn crop_of_full_width_planes_copies_leading_rows() {
        let source = planes(4, 5);
        let mut output = vec![u32::MAX; CHANNELS * 2 * 5];

        crop_planes_into(&source, 4, 5, 2, 5, &mut output).unwrap();

        let expected: Vec<u32> = (0..CHANNELS)
            .flat_map(|c| (0..10).map(move |i| (c * 20 + i) as u32))
            .collect();
        assert_eq!(output, expected);
    }

    #[test]
    fn crop_rejects_mismatched_buffers() {
        let source = planes(4, 4);
        let mut short = vec![0; CHANNELS * 2 * 2 - 1];
        assert!(crop_planes_into(&source, 4, 4, 2, 2, &mut short).is_err());
        let mut tall = vec![0; CHANNELS * 5 * 2];
        assert!(crop_planes_into(&source, 4, 4, 5, 2, &mut tall).is_err());
    }

    #[test]
    fn reflect_pad_mirrors_bottom_rows_and_right_columns() {
        for (h, w, padded_h, padded_w) in [(3, 5, 4, 8), (4, 4, 4, 4), (3, 3, 6, 3), (1, 2, 2, 4)] {
            let source = planes(h, w);
            let mut output = vec![u32::MAX; CHANNELS * padded_h * padded_w];

            reflect_pad_planes_into(&source, h, w, padded_h, padded_w, &mut output).unwrap();

            for c in 0..CHANNELS {
                for y in 0..padded_h {
                    for x in 0..padded_w {
                        let expected = source[c * h * w + reflect(y, h) * w + reflect(x, w)];
                        let actual = output[c * padded_h * padded_w + y * padded_w + x];
                        assert_eq!(
                            actual, expected,
                            "{h}x{w}->{padded_h}x{padded_w} c{c} y{y} x{x}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn reflect_pad_rejects_padding_beyond_one_mirror() {
        let source = planes(2, 2);
        let mut output = vec![0; CHANNELS * 5 * 2];
        assert!(reflect_pad_planes_into(&source, 2, 2, 5, 2, &mut output).is_err());
    }
}
