use super::*;
#[cfg(test)]
use crate::pixels::test_support::TestImageOutput;
use std::{io::Cursor, time::Instant};

fn full_decode_then_scale(bytes: &[u8], max_width: u32) -> Result<Bitmap> {
    let mut image = read_image(bytes)?;
    let scale = (max_width as f64 / image.width() as f64)
        .min(1.0)
        .min((16.0 * 1024.0 * 1024.0 / (image.width() as f64 * image.height() as f64)).sqrt());
    if scale < 1.0 {
        image = crate::pixels::resize(
            &image,
            (image.width() as f64 * scale).max(1.0) as u32,
            (image.height() as f64 * scale).max(1.0) as u32,
            false,
        )?;
    }
    let (width, height) = (image.width(), image.height());
    let tiles = (0..height)
        .step_by(2048)
        .map(|y| crate::pixels::crop(&image, 0, y, width, (height - y).min(2048)))
        .collect::<Result<Vec<_>>>()?;
    Ok(Bitmap {
        width: width as usize,
        height: height as usize,
        tiles,
    })
}

#[test]
fn preview_resize_preserves_alpha_dimensions_and_tile_rows() {
    let mut bytes = Cursor::new(Vec::new());
    let image = std::convert::identity(crate::pixels::generated(8, 8, |x, _| {
        if x < 4 {
            [255, 0, 0, 0]
        } else {
            [0, 0, 255, 255]
        }
    }));
    image.write_to(&mut bytes, "png").unwrap();
    let resized = decode(bytes.get_ref(), 4).unwrap();
    assert_eq!((resized.width, resized.height), (4, 4));
    for pixel in resized.tiles[0].pixels() {
        if pixel[3] > 0 {
            assert_eq!(pixel[0], 0, "transparent RGB leaked into a visible edge");
        }
    }
    let mut bytes = Cursor::new(Vec::new());
    std::convert::identity(crate::pixels::generated(4, 4100, |_, y| {
        [(y % 251) as u8, 0, 0, 255]
    }))
    .write_to(&mut bytes, "png")
    .unwrap();
    let bitmap = decode(bytes.get_ref(), 1800).unwrap();
    assert_eq!(
        bitmap.tiles.iter().map(|t| t.height()).collect::<Vec<_>>(),
        vec![2048, 2048, 4]
    );
    for (i, pixel) in bitmap
        .tiles
        .iter()
        .flat_map(|t| t.bytes().chunks(t.info().stride).map(|row| row[0]))
        .enumerate()
    {
        assert_eq!(pixel, (i % 251) as u8);
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
        ("large JPEG", "jpeg", 3200, 4800),
        ("long PNG", "png", 1200, 12000),
        ("WebP", "webp", 1600, 2400),
    ] {
        if format == "webp" {
            samples.push((
                name.into(),
                include_bytes!("../../../tests/fixtures/images/comic.webp").to_vec(),
            ));
            continue;
        }
        let image = crate::pixels::generated(width, height, |x, y| {
            let ink = if x % 280 < 4 || y % 360 < 4 { 0 } else { 220 };
            [
                ink,
                ((x / 16 + y / 12) % 240) as u8,
                ((x / 30 + y / 16) % 240) as u8,
                255,
            ]
        });
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
        let full_decode_ms = measure(|| {
            std::hint::black_box(full_decode_then_scale(&bytes, 1800).unwrap());
        });
        let decode_ms = measure(|| {
            std::hint::black_box(decode(&bytes, 1800).unwrap());
        });
        let bitmap = decode(&bytes, 1800).unwrap();
        let color_ms = measure(|| {
            for tile in &bitmap.tiles {
                std::hint::black_box(eframe::egui::ColorImage::from_rgba_unmultiplied(
                    [tile.width() as usize, tile.height() as usize],
                    tile.bytes(),
                ));
            }
        });
        println!(
            "{name}: codec={read_ms:.2}ms, full-decode={full_decode_ms:.2}ms, decode={decode_ms:.2}ms, color={color_ms:.2}ms"
        );
        results.push(serde_json::json!({"sample":name,"codec_ms":read_ms,"full_decode_ms":full_decode_ms,"decode_ms":decode_ms,"color_ms":color_ms}));
    }
    let directory = Path::new("target/comic-performance");
    std::fs::create_dir_all(directory).unwrap();
    std::fs::write(
        directory.join("decode-benchmark.json"),
        serde_json::to_vec_pretty(&results).unwrap(),
    )
    .unwrap();
}
