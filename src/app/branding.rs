use super::*;

/// Use the approved brand artwork; the playback triangle is a separate control.
pub(super) fn paint(painter: &Painter, rect: Rect) {
    let ctx = painter.ctx();
    let id = Id::new("replayer-brand-mark");
    let cached = ctx.data(|data| data.get_temp::<egui::TextureHandle>(id));
    let texture = cached.unwrap_or_else(|| {
        let image = image::load_from_memory(include_bytes!(
            "../../assets/branding/replayer-mark-256.png"
        ))
        .expect("embedded replayer brand image")
        .into_rgba8();
        let texture = ctx.load_texture(
            "replayer-brand-mark",
            egui::ColorImage::from_rgba_unmultiplied(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            ),
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
