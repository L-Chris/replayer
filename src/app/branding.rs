use super::*;
use std::sync::OnceLock;

/// Resample premultiplied colors so transparent pixels cannot darken the edge.
/// Match egui's gamma-space blending when filtering the approved artwork.
fn rasterize(size: [usize; 2]) -> egui::ColorImage {
    static SOURCE: OnceLock<image::Rgba32FImage> = OnceLock::new();
    let source = SOURCE.get_or_init(|| {
        let mut image = image::load_from_memory(include_bytes!(
            "../../assets/branding/replayer-mark-256.png"
        ))
        .expect("embedded replayer brand image")
        .into_rgba32f();
        for pixel in image.pixels_mut() {
            let alpha = pixel[3];
            for channel in &mut pixel.0[..3] {
                *channel *= alpha;
            }
        }
        image
    });
    let image = image::imageops::resize(
        source,
        size[0] as u32,
        size[1] as u32,
        image::imageops::FilterType::Lanczos3,
    );
    let pixels = image
        .pixels()
        .map(|pixel| {
            let alpha = pixel[3].clamp(0.0, 1.0);
            let byte = |value: f32| (value * 255.0).round() as u8;
            // Lanczos can overshoot; retain valid premultiplied colors.
            Color32::from_rgba_premultiplied(
                byte(pixel[0].clamp(0.0, alpha)),
                byte(pixel[1].clamp(0.0, alpha)),
                byte(pixel[2].clamp(0.0, alpha)),
                byte(alpha),
            )
        })
        .collect();
    egui::ColorImage::new(size, pixels)
}

/// Use the approved brand artwork; the playback triangle is a separate control.
pub(super) fn paint(painter: &Painter, rect: Rect) {
    if !rect.is_positive() {
        return;
    }
    let ctx = painter.ctx();
    let scale = painter.pixels_per_point();
    let size = [
        (rect.width() * scale).round().max(1.0) as usize,
        (rect.height() * scale).round().max(1.0) as usize,
    ];
    // One texel per physical pixel, including fractional Windows display scales.
    let rect = Rect::from_min_size(
        pos2(
            (rect.min.x * scale).round() / scale,
            (rect.min.y * scale).round() / scale,
        ),
        vec2(size[0] as f32 / scale, size[1] as f32 / scale),
    );
    let id = Id::new("replayer-brand-mark");
    let cached = ctx.data(|data| data.get_temp::<egui::TextureHandle>(id));
    let texture = cached
        .filter(|texture| texture.size() == size)
        .unwrap_or_else(|| {
            let texture = ctx.load_texture(
                "replayer-brand-mark",
                rasterize(size),
                egui::TextureOptions::LINEAR,
            );
            ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
            texture
        });
    painter.image(
        texture.id(),
        rect,
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}
