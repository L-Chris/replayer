use super::*;
use std::sync::OnceLock;

/// Use remdia system components for alpha-aware branding image scaling.
fn rasterize(size: [usize; 2]) -> egui::ColorImage {
    static SOURCE: OnceLock<crate::pixels::Image> = OnceLock::new();
    let source = SOURCE.get_or_init(|| {
        crate::pixels::read_image(include_bytes!(
            "../../assets/branding/replayer-mark-256.png"
        ))
        .expect("embedded brand image through remdia")
    });
    let image = crate::pixels::resize(source, size[0] as u32, size[1] as u32, false)
        .expect("brand resize through remdia");
    let pixels = image
        .bytes()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| Color32::from_rgba_unmultiplied(pixel[0], pixel[1], pixel[2], pixel[3]))
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
