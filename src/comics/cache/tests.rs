use super::*;
use std::{io::Cursor, time::Instant};

fn legacy_decode(bytes: &[u8], max_width: u32) -> Result<Bitmap> {
    let mut image = read_image(bytes)?;
    let scale = (max_width as f64 / image.width() as f64)
        .min(1.0)
        .min((16.0 * 1024.0 * 1024.0 / (image.width() as f64 * image.height() as f64)).sqrt());
    if scale < 1.0 {
        image = image.resize_exact(
            (image.width() as f64 * scale).max(1.0) as u32,
            (image.height() as f64 * scale).max(1.0) as u32,
            image::imageops::FilterType::Triangle,
        );
    }
    let (width, height) = (image.width(), image.height());
    let tiles = (0..height)
        .step_by(2048)
        .map(|y| {
            image
                .crop_imm(0, y, width, (height - y).min(2048))
                .to_rgba8()
        })
        .collect();
    Ok(Bitmap {
        width: width as usize,
        height: height as usize,
        tiles,
    })
}

#[test]
fn preview_resize_preserves_alpha_dimensions_and_tile_rows() {
    let mut bytes = Cursor::new(Vec::new());
    let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(8, 8, |x, _| {
        if x < 4 {
            image::Rgba([255, 0, 0, 0])
        } else {
            image::Rgba([0, 0, 255, 255])
        }
    }));
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    let resized = decode(bytes.get_ref(), 4).unwrap();
    assert_eq!((resized.width, resized.height), (4, 4));
    for pixel in resized.tiles[0].pixels() {
        if pixel[3] > 0 {
            assert_eq!(pixel[0], 0, "transparent RGB leaked into a visible edge");
        }
    }
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(4, 4100, |_, y| {
        image::Rgba([(y % 251) as u8, 0, 0, 255])
    }))
    .write_to(&mut bytes, image::ImageFormat::Png)
    .unwrap();
    let bitmap = decode(bytes.get_ref(), 1800).unwrap();
    assert_eq!(
        bitmap.tiles.iter().map(|t| t.height()).collect::<Vec<_>>(),
        vec![2048, 2048, 4]
    );
    for (i, pixel) in bitmap
        .tiles
        .iter()
        .flat_map(|t| t.rows().map(|mut r| *r.next().unwrap()))
        .enumerate()
    {
        assert_eq!(pixel[0], (i % 251) as u8);
    }
}

fn samples() -> Vec<(String, Vec<u8>)> {
    let mut samples = Vec::new();
    if let Ok(files) = std::fs::read_dir("target/komiic-research/live-cache") {
        for file in files.flatten().take(1) {
            samples.push(("Komiic JPEG".into(), std::fs::read(file.path()).unwrap()));
        }
    }
    for (name, format, width, height) in [
        ("large JPEG", image::ImageFormat::Jpeg, 3200, 4800),
        ("long PNG", image::ImageFormat::Png, 1200, 12000),
        ("WebP", image::ImageFormat::WebP, 1600, 2400),
    ] {
        let image =
            image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(width, height, |x, y| {
                let ink = if x % 280 < 4 || y % 360 < 4 { 0 } else { 220 };
                image::Rgb([
                    ink,
                    ((x / 16 + y / 12) % 240) as u8,
                    ((x / 30 + y / 16) % 240) as u8,
                ])
            }));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, format).unwrap();
        samples.push((name.into(), bytes.into_inner()));
    }
    samples
}
fn measure(mut work: impl FnMut()) -> f64 {
    work();
    let start = Instant::now();
    for _ in 0..3 {
        work();
    }
    start.elapsed().as_secs_f64() * 1000.0 / 3.0
}

#[test]
#[ignore = "Local decode benchmark; generates large images under target"]
fn comic_decode_benchmark() {
    let mut results = Vec::new();
    for (name, bytes) in samples() {
        let read_ms = measure(|| {
            std::hint::black_box(read_image(&bytes).unwrap());
        });
        let legacy_ms = measure(|| {
            std::hint::black_box(legacy_decode(&bytes, 1800).unwrap());
        });
        let decode_ms = measure(|| {
            std::hint::black_box(decode(&bytes, 1800).unwrap());
        });
        let bitmap = decode(&bytes, 1800).unwrap();
        let color_ms = measure(|| {
            for tile in &bitmap.tiles {
                std::hint::black_box(eframe::egui::ColorImage::from_rgba_unmultiplied(
                    [tile.width() as usize, tile.height() as usize],
                    tile.as_raw(),
                ));
            }
        });
        println!(
            "{name}: codec={read_ms:.2}ms, legacy={legacy_ms:.2}ms, decode={decode_ms:.2}ms, color={color_ms:.2}ms"
        );
        results.push(serde_json::json!({"sample":name,"codec_ms":read_ms,"legacy_ms":legacy_ms,"decode_ms":decode_ms,"color_ms":color_ms}));
    }
    let directory = Path::new("target/comic-performance");
    std::fs::create_dir_all(directory).unwrap();
    std::fs::write(
        directory.join("decode-benchmark.json"),
        serde_json::to_vec_pretty(&results).unwrap(),
    )
    .unwrap();
}
