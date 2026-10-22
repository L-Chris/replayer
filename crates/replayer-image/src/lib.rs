//! Bounded image decoding and SIMD preview resizing, independent of the UI.
use anyhow::{Result, anyhow};
use image::buffer::ConvertBuffer;
use std::io::Cursor;

pub struct Bitmap {
    pub width: usize,
    pub height: usize,
    pub tiles: Vec<image::RgbaImage>,
}
pub fn read_image(bytes: &[u8]) -> Result<image::DynamicImage> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(65536);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|_| anyhow!("图片损坏或尺寸过大 / Invalid or oversized image"))
}
pub fn decode(bytes: &[u8], max_width: u32) -> Result<Bitmap> {
    let mut image = read_image(bytes)?;
    let scale = (max_width as f64 / image.width() as f64)
        .min(1.0)
        .min((16.0 * 1024.0 * 1024.0 / (image.width() as f64 * image.height() as f64)).sqrt());
    if scale < 1.0 {
        let width = (image.width() as f64 * scale).max(1.0) as u32;
        let height = (image.height() as f64 * scale).max(1.0) as u32;
        let alpha = image.color().has_alpha();
        let rgba = into_rgba8(image);
        let mut resized = image::RgbaImage::new(width, height);
        fast_image_resize::Resizer::new().resize(
            &rgba,
            &mut resized,
            &fast_image_resize::ResizeOptions::new()
                .use_alpha(alpha)
                .resize_alg(fast_image_resize::ResizeAlg::Convolution(
                    fast_image_resize::FilterType::Bilinear,
                )),
        )?;
        image = image::DynamicImage::ImageRgba8(resized);
    }
    let width = image.width();
    let height = image.height();
    let pixels = into_rgba8(image).into_raw();
    let tiles = if height <= 2048 {
        vec![image::RgbaImage::from_raw(width, height, pixels).unwrap()]
    } else {
        pixels
            .chunks(width as usize * 2048 * 4)
            .map(|bytes| {
                image::RgbaImage::from_raw(
                    width,
                    (bytes.len() / (width as usize * 4)) as u32,
                    bytes.to_vec(),
                )
                .unwrap()
            })
            .collect()
    };
    Ok(Bitmap {
        width: width as usize,
        height: height as usize,
        tiles,
    })
}

fn into_rgba8(image: image::DynamicImage) -> image::RgbaImage {
    // Expanding 8-bit channels needs no color transform. ConvertBuffer retains
    // the source color metadata and avoids the general CMS casting path.
    match image {
        image::DynamicImage::ImageRgb8(rgb) => rgb.convert(),
        image::DynamicImage::ImageLuma8(gray) => gray.convert(),
        image::DynamicImage::ImageLumaA8(gray) => gray.convert(),
        other => other.into_rgba8(),
    }
}
