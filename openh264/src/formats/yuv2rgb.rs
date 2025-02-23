/// Converts 8 float values into a f32x8 SIMD lane, taking into account block size.
///
/// If you have a (pixel buffer) slice of at least 8 f32 values like so `[012345678...]`, this function
/// will convert the first N <= 8 elements into a packed f32x8 SIMD struct. For example
///
/// - if block size `1` (like for Y values), you will get  `f32x8(012345678)`.
/// - if block size is `2` (for U and V), you will get `f32x8(00112233)`
macro_rules! f32x8_from_slice_with_blocksize {
    ($buf:expr, $block_size:expr) => {{
        wide::f32x8::from([
            (f32::from($buf[0])),
            (f32::from($buf[1 / $block_size])),
            (f32::from($buf[2 / $block_size])),
            (f32::from($buf[3 / $block_size])),
            (f32::from($buf[4 / $block_size])),
            (f32::from($buf[5 / $block_size])),
            (f32::from($buf[6 / $block_size])),
            (f32::from($buf[7 / $block_size])),
        ])
    }};
}

/// Write RGB8 data from YUV420 using scalar (non SIMD) math.
pub fn write_rgb8_scalar(
    y_plane: &[u8],
    u_plane: &[u8],
    v_plane: &[u8],
    dim: (usize, usize),
    strides: (usize, usize, usize),
    target: &mut [u8],
) {
    for y in 0..dim.1 {
        for x in 0..dim.0 {
            let base_tgt = (y * dim.0 + x) * 3;
            let base_y = y * strides.0 + x;
            let base_u = (y / 2 * strides.1) + (x / 2);
            let base_v = (y / 2 * strides.2) + (x / 2);

            let rgb_pixel = &mut target[base_tgt..base_tgt + 3];

            let y = f32::from(y_plane[base_y]);
            let u = f32::from(u_plane[base_u]);
            let v = f32::from(v_plane[base_v]);

            rgb_pixel[0] = 1.402f32.mul_add(v - 128.0, y) as u8;
            rgb_pixel[1] = 0.714f32.mul_add(-(v - 128.0), 0.344f32.mul_add(-(u - 128.0), y)) as u8;
            rgb_pixel[2] = 1.772f32.mul_add(u - 128.0, y) as u8;
        }
    }
}

/// Write RGB8 data from YUV420 using f32x8 SIMD.
#[allow(clippy::identity_op)]
pub fn write_rgb8_f32x8(
    y_plane: &[u8],
    u_plane: &[u8],
    v_plane: &[u8],
    dim: (usize, usize),
    strides: (usize, usize, usize),
    target: &mut [u8],
) {
    const RGB_PIXEL_LEN: usize = 3;

    // this assumes we are decoding YUV420
    assert_eq!(y_plane.len(), u_plane.len() * 4);
    assert_eq!(y_plane.len(), v_plane.len() * 4);
    assert_eq!(dim.0 % 8, 0);

    let (width, height) = dim;
    let rgb_bytes_per_row: usize = RGB_PIXEL_LEN * width; // rgb pixel size in bytes

    for y in 0..(height / 2) {
        // load U and V values for two rows of pixels
        let base_u = y * strides.1;
        let u_row = &u_plane[base_u..base_u + strides.1];
        let base_v = y * strides.2;
        let v_row = &v_plane[base_v..base_v + strides.2];

        // load Y values for first row
        let base_y = 2 * y * strides.0;
        let y_row = &y_plane[base_y..base_y + strides.0];

        // calculate first RGB row
        let base_tgt = 2 * y * rgb_bytes_per_row;
        let row_target = &mut target[base_tgt..base_tgt + rgb_bytes_per_row];
        write_rgb8_f32x8_row(y_row, u_row, v_row, width, row_target);

        // load Y values for second row
        let base_y = (2 * y + 1) * strides.0;
        let y_row = &y_plane[base_y..base_y + strides.0];

        // calculate second RGB row
        let base_tgt = (2 * y + 1) * rgb_bytes_per_row;
        let row_target = &mut target[base_tgt..(base_tgt + rgb_bytes_per_row)];
        write_rgb8_f32x8_row(y_row, u_row, v_row, width, row_target);
    }
}

/// Write a single RGB8 row from YUV420 row data using f32x8 SIMD.
#[allow(clippy::inline_always)]
#[allow(clippy::similar_names)]
#[inline(always)]
fn write_rgb8_f32x8_row(y_row: &[u8], u_row: &[u8], v_row: &[u8], width: usize, target: &mut [u8]) {
    const STEP: usize = 8;
    const UV_STEP: usize = STEP / 2;
    const TGT_STEP: usize = STEP * 3;

    assert_eq!(y_row.len(), u_row.len() * 2);
    assert_eq!(y_row.len(), v_row.len() * 2);

    let rv_mul = wide::f32x8::splat(1.402);
    let gu_mul = wide::f32x8::splat(-0.344);
    let gv_mul = wide::f32x8::splat(-0.714);
    let bu_mul = wide::f32x8::splat(1.772);

    let upper_bound = wide::f32x8::splat(255.0);
    let lower_bound = wide::f32x8::splat(0.0);

    assert_eq!(y_row.len() % STEP, 0);

    assert_eq!(u_row.len() % UV_STEP, 0);
    assert_eq!(v_row.len() % UV_STEP, 0);

    assert_eq!(target.len() % TGT_STEP, 0);

    let mut base_y = 0;
    let mut base_uv = 0;
    let mut base_tgt = 0;

    for _ in (0..width).step_by(STEP) {
        let pixels = &mut target[base_tgt..(base_tgt + TGT_STEP)];

        let y_pack: wide::f32x8 = f32x8_from_slice_with_blocksize!(y_row[base_y..], 1);
        let u_pack: wide::f32x8 = f32x8_from_slice_with_blocksize!(u_row[base_uv..], 2) - 128.0;
        let v_pack: wide::f32x8 = f32x8_from_slice_with_blocksize!(v_row[base_uv..], 2) - 128.0;

        let r_pack = v_pack.mul_add(rv_mul, y_pack);
        let g_pack = v_pack.mul_add(gv_mul, u_pack.mul_add(gu_mul, y_pack));
        let b_pack = u_pack.mul_add(bu_mul, y_pack);

        let (r_pack, g_pack, b_pack) = (
            r_pack.fast_min(upper_bound).fast_max(lower_bound).fast_trunc_int(),
            g_pack.fast_min(upper_bound).fast_max(lower_bound).fast_trunc_int(),
            b_pack.fast_min(upper_bound).fast_max(lower_bound).fast_trunc_int(),
        );

        let (r_pack, g_pack, b_pack) = (r_pack.as_array_ref(), g_pack.as_array_ref(), b_pack.as_array_ref());

        for i in 0..STEP {
            pixels[3 * i] = r_pack[i] as u8;
            pixels[(3 * i) + 1] = g_pack[i] as u8;
            pixels[(3 * i) + 2] = b_pack[i] as u8;
        }

        base_y += STEP;
        base_uv += UV_STEP;
        base_tgt += TGT_STEP;
    }
}

#[cfg(test)]
mod test {
    use crate::decoder::{Decoder, DecoderConfig};
    use crate::formats::yuv2rgb::{write_rgb8_f32x8, write_rgb8_scalar};
    use crate::formats::YUVSource;
    use crate::OpenH264API;

    #[test]
    fn write_rgb8_f32x8_matches_scalar() {
        let source = include_bytes!("../../tests/data/single_512x512_cavlc.h264");

        let api = OpenH264API::from_source();
        let config = DecoderConfig::default();
        let mut decoder = Decoder::with_api_config(api, config).unwrap();

        let mut rgb = vec![0; 2000 * 2000 * 3];
        let yuv = decoder.decode(&source[..]).unwrap().unwrap();
        let dim = yuv.dimensions();
        let rgb_len = dim.0 * dim.1 * 3;

        let tgt = &mut rgb[0..rgb_len];

        write_rgb8_scalar(yuv.y(), yuv.u(), yuv.v(), yuv.dimensions(), yuv.strides(), tgt);

        let mut tgt2 = vec![0; tgt.len()];
        write_rgb8_f32x8(yuv.y(), yuv.u(), yuv.v(), yuv.dimensions(), yuv.strides(), &mut tgt2);

        assert_eq!(tgt, tgt2);
    }
}
