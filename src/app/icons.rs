//! The UI uses one Phosphor Regular subset, not the full icon font.
use eframe::egui::{self, Color32, Painter, Rect};

egui_phosphor::subset! {
    #[allow(dead_code)]
    pub mod glyphs {
        use regular::{
            HOUSE, GEAR_SIX, BOOK_OPEN, MUSIC_NOTES, FILM_SLATE, HEART,
            CLOCK_COUNTER_CLOCKWISE, DOWNLOAD_SIMPLE, ARROW_LEFT, PLUS, MINUS,
            X, MAGNIFYING_GLASS, TRASH, FOLDER_OPEN, MAGNET, QUEUE, SUBTITLES,
            REPEAT, REPEAT_ONCE, TRANSLATE, FILES, CARET_DOUBLE_UP,
            INFO, CARET_DOWN, CARET_RIGHT, DOTS_THREE, ARROWS_CLOCKWISE,
        };
    }
}
pub use glyphs::regular::*;

pub fn install(ctx: &egui::Context) {
    ctx.add_font(egui::epaint::text::FontInsert {
        name: FONT_NAME.into(),
        data: egui::FontData::from_static(&FONT),
        families: vec![egui::epaint::text::InsertFontFamily {
            family: family(),
            priority: egui::epaint::text::FontPriority::Highest,
        }],
    });
}

pub fn paint(p: &Painter, rect: Rect, color: Color32, glyph: &str) {
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        // Font glyphs include side bearings; match the visible size of the
        // icon slot rather than making every imported icon appear smaller.
        egui::FontId::new(rect.width().min(rect.height()) * 1.15, family()),
        color,
    );
}

pub fn refresh(p: &Painter, rect: Rect, color: Color32) {
    paint(p, rect, color, ARROWS_CLOCKWISE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_family_keeps_real_glyphs_after_loading_text_fonts_at_every_scale() {
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let ctx = egui::Context::default();
            install(&ctx);
            super::super::setup_fonts(&ctx);
            ctx.set_pixels_per_point(scale);
            let mut output = ctx.run_ui(Default::default(), |ui| {
                for glyph in [
                    HOUSE,
                    GEAR_SIX,
                    BOOK_OPEN,
                    MUSIC_NOTES,
                    FILM_SLATE,
                    HEART,
                    CLOCK_COUNTER_CLOCKWISE,
                    DOWNLOAD_SIMPLE,
                    TRANSLATE,
                    FILES,
                ] {
                    let font = egui::FontId::new(20.0, family());
                    assert!(ui.fonts_mut(|fonts| fonts.has_glyphs(&font, glyph)));
                    let (rect, _) =
                        ui.allocate_exact_size(egui::Vec2::splat(20.0), egui::Sense::hover());
                    paint(ui.painter(), rect, Color32::WHITE, glyph);
                }
                assert!(ui.fonts(|fonts| fonts.definitions().font_data.contains_key(FONT_NAME)));
            });
            output.textures_delta.clear();
        }
    }
}
