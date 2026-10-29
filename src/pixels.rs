//! Application jobs use the same remdia graph as playback. No local codec.
use anyhow::{Context, Result, ensure};
pub use remdia::image::Image;
use remdia::{
    Cancellation, MediaRequest,
    image::{ImageInput, ImageOperation as Op, ImageRequest},
};
use serde_json::json;

pub struct Bitmap {
    pub width: usize,
    pub height: usize,
    pub tiles: Vec<Image>,
}
fn run(input: ImageInput, operations: Vec<Op>) -> Result<Vec<remdia::Frame>> {
    let engine = crate::media_backend::engine()?;
    let request = MediaRequest::Image(ImageRequest::new(input, operations));
    let mut session = unsafe { engine.open_job(request, Cancellation::default())? };
    let mut result = Vec::new();
    while let Some(frame) = session.read()? {
        result.push(frame)
    }
    Ok(result)
}
fn one(input: ImageInput, operations: Vec<Op>) -> Result<Image> {
    let mut frames = run(input, operations)?;
    ensure!(frames.len() == 1, "expected one image");
    Image::from_buffer(frames.pop().unwrap().buffer().clone())
}
pub fn read_image(bytes: &[u8]) -> Result<Image> {
    one(
        ImageInput::Bytes(bytes.into()),
        vec![Op::new("decode", json!({}))],
    )
}
pub fn thumbnail(bytes: &[u8], w: u32, h: u32) -> Result<Image> {
    one(
        ImageInput::Bytes(bytes.into()),
        vec![Op::new(
            "decode",
            json!({"max_width":w,"max_height":h,"max_pixels":16*1024*1024}),
        )],
    )
}
pub fn decode(bytes: &[u8], max_width: u32) -> Result<Bitmap> {
    let frames = run(
        ImageInput::Bytes(bytes.into()),
        vec![
            Op::new(
                "decode",
                json!({"max_width":max_width,"max_pixels":16*1024*1024}),
            ),
            Op::new("tile", json!({"height":2048})),
        ],
    )?;
    let tiles = frames
        .into_iter()
        .map(|frame| Image::from_buffer(frame.buffer().clone()))
        .collect::<Result<Vec<_>>>()?;
    let width = tiles.first().context("no image tiles")?.width() as usize;
    let height = tiles.iter().map(|t| t.height() as usize).sum();
    Ok(Bitmap {
        width,
        height,
        tiles,
    })
}
pub fn resize(image: &Image, width: u32, height: u32, fit: bool) -> Result<Image> {
    if width == image.width() && height == image.height() {
        return Ok(image.clone());
    }
    one(
        ImageInput::Pixels(image.clone()),
        vec![Op::new(
            "scale",
            json!({"width":width,"height":height,"fit":fit}),
        )],
    )
}
pub fn crop(image: &Image, x: u32, y: u32, width: u32, height: u32) -> Result<Image> {
    one(
        ImageInput::Pixels(image.clone()),
        vec![Op::new(
            "crop",
            json!({"x":x,"y":y,"width":width,"height":height}),
        )],
    )
}
pub fn paste(canvas: &Image, patch: &Image, x: u32, y: u32) -> Result<Image> {
    one(
        ImageInput::Sequence(vec![canvas.clone(), patch.clone()]),
        vec![Op::new("compose", json!({"x":x,"y":y}))],
    )
}
pub fn encode(image: &Image, format: &str, quality: f64) -> Result<Vec<u8>> {
    let mut ops = Vec::new();
    if format == "jpeg" {
        ops.push(Op::new("alpha", json!({"background":[255,255,255]})))
    }
    ops.push(Op::new(
        "encode",
        json!({"format":format,"quality":quality,"max_output_bytes":40*1024*1024}),
    ));
    let mut frames = run(ImageInput::Pixels(image.clone()), ops)?;
    ensure!(frames.len() == 1, "expected one encoded image");
    Ok(frames.pop().unwrap().bytes().to_vec())
}
pub fn png(image: &Image) -> Result<Vec<u8>> {
    encode(image, "png", 1.0)
}
pub fn jpeg(image: &Image) -> Result<Vec<u8>> {
    encode(image, "jpeg", 0.95)
}

#[cfg(test)]
pub fn generated(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Image {
    let mut bytes = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        for x in 0..width {
            bytes.extend_from_slice(&pixel(x, y))
        }
    }
    Image::from_rgba(width, height, bytes).unwrap()
}

#[cfg(test)]
pub fn solid(width: u32, height: u32, color: [u8; 4]) -> Image {
    generated(width, height, |_, _| color)
}
#[cfg(test)]
pub fn black(width: u32, height: u32) -> Image {
    solid(width, height, [0, 0, 0, 255])
}
#[cfg(test)]
pub fn format(bytes: &[u8]) -> Result<String> {
    let frames = run(
        ImageInput::Bytes(bytes.into()),
        vec![Op::new("probe", json!({}))],
    )?;
    let meta: serde_json::Value = serde_json::from_slice(frames[0].bytes())?;
    Ok(
        match meta["format"].as_str().context("missing image format")? {
            "public.jpeg" => "jpeg",
            "public.png" => "png",
            "org.webmproject.webp" => "webp",
            other => other,
        }
        .to_owned(),
    )
}
#[cfg(test)]
pub mod test_support {
    use super::*;
    pub fn webp_supported() -> bool {
        let caps = unsafe {
            crate::media_backend::engine()
                .unwrap()
                .image_capabilities()
                .unwrap()
        };
        caps["decode"]
            .as_array()
            .unwrap()
            .iter()
            .any(|format| format == "org.webmproject.webp")
    }
    pub trait TestImageOutput {
        fn write_to(&self, writer: &mut impl std::io::Write, format: &str) -> Result<()>;
        fn save(&self, path: impl AsRef<std::path::Path>) -> Result<()>;
    }
    impl TestImageOutput for Image {
        fn write_to(&self, writer: &mut impl std::io::Write, format: &str) -> Result<()> {
            writer.write_all(&encode(self, format, 0.95)?)?;
            Ok(())
        }
        fn save(&self, path: impl AsRef<std::path::Path>) -> Result<()> {
            let path = path.as_ref();
            let format = if path.extension().is_some_and(|s| s == "jpg" || s == "jpeg") {
                "jpeg"
            } else {
                "png"
            };
            std::fs::write(path, encode(self, format, 0.95)?)?;
            Ok(())
        }
    }
}
