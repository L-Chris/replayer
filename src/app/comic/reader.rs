use super::*;
mod controls;

pub(super) const INIT_MIN_HEIGHT: f32 = 520.0;
pub(super) const INIT_MAX_HEIGHT: f32 = 1200.0;
pub(super) const INIT_MIN_WIDTH: f32 = 380.0;
pub(super) const INIT_WEBTOON_MIN_WIDTH: f32 = 520.0;
pub(super) const INIT_MAX_WIDTH: f32 = 960.0;

pub(super) fn page_dimensions(
    viewport_h: f32,
    avail_w: f32,
    img_size: (f32, f32),
    compare: bool,
    zoom: f32,
) -> Vec2 {
    let img_w = img_size.0.max(1.0);
    let img_h = img_size.1.max(1.0);
    let aspect = img_w / img_h;
    let target_h = viewport_h.clamp(INIT_MIN_HEIGHT, INIT_MAX_HEIGHT);
    let raw_w = target_h * aspect;
    let max_single_w = if compare {
        ((avail_w - 10.0) * 0.5).max(120.0)
    } else {
        avail_w.max(120.0)
    };
    let max_w = INIT_MAX_WIDTH.min(max_single_w);
    let desired_min_w = if img_h / img_w > 2.2 {
        INIT_WEBTOON_MIN_WIDTH
    } else {
        INIT_MIN_WIDTH
    };
    let min_w = desired_min_w.min(max_w);
    let base_single_w = raw_w.clamp(min_w, max_w);
    let single_w = base_single_w * zoom;
    let page_h = single_w / aspect;
    let total_w = if compare {
        single_w * 2.0 + 10.0
    } else {
        single_w
    };
    vec2(total_w, page_h)
}

impl State {
    pub(super) fn reader_ui(
        &mut self,
        ui: &mut Ui,
        settings: &crate::settings::Settings,
        settings_open: &mut bool,
    ) {
        let l = settings.language;
        let mut back = false;
        let mut chapter_action = None;
        let mut retry = false;
        let Some(reader) = &mut self.reader else {
            self.view = View::Detail;
            return;
        };
        let status = controls::Status {
            message: &self.status,
            total: self.translation_total,
            done: self.translation_done,
            failed: self.translation_failed,
            working: self.translation.is_some() || !self.translations.is_empty(),
        };
        let has_status = status.has_activity(!reader.errors.is_empty());
        let screen = ui.available_rect_before_wrap();
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let bar = controls::layout(ui, screen, l, has_status);
        let near_edge = pointer.is_some_and(|p| {
            screen.contains(p)
                && (p.y < screen.min.y + controls::BAR_HEIGHT + 8.0
                    || p.y > bar.footer.min.y - 12.0)
        });
        if near_edge || egui::Popup::is_any_open(ui.ctx()) || ui.ctx().dragged_id().is_some() {
            reader.toolbar_until = Instant::now() + Duration::from_secs(2);
        }
        if ui.input(|i| i.key_pressed(Key::Escape)) {
            let fullscreen = window::fullscreen(ui.ctx());
            if fullscreen {
                window::set_fullscreen(ui.ctx(), false);
            } else if reader.sidebar {
                reader.sidebar = false;
            } else {
                back = true;
            }
        }
        let mut key_scroll_y = 0.0_f32;
        if !ui.ctx().egui_wants_keyboard_input() && !*settings_open {
            let delta = if ui.input(|i| i.key_pressed(Key::ArrowRight)) {
                1
            } else if ui.input(|i| i.key_pressed(Key::ArrowLeft)) {
                -1
            } else {
                0
            };
            if delta != 0 {
                reader.current = (reader.current as i64 + delta)
                    .clamp(0, reader.urls.len().saturating_sub(1) as i64)
                    as usize;
                reader.fraction = 0.0;
                reader.restore = true;
                reader.dirty = true;
                reader.toolbar_until = Instant::now() + Duration::from_secs(2);
            }
            if ui.input(|i| i.key_pressed(Key::ArrowDown)) {
                key_scroll_y -= 140.0;
            }
            if ui.input(|i| i.key_pressed(Key::ArrowUp)) {
                key_scroll_y += 140.0;
            }
            if ui.input(|i| i.key_pressed(Key::PageDown) || i.key_pressed(Key::Space)) {
                key_scroll_y -= screen.height() * 0.85;
            }
            if ui.input(|i| i.key_pressed(Key::PageUp)) {
                key_scroll_y += screen.height() * 0.85;
            }
        }
        let controls = Instant::now() < reader.toolbar_until;
        let header = Rect::from_min_size(screen.min, vec2(screen.width(), controls::BAR_HEIGHT));
        let footer = bar.footer;
        let side_width = if reader.sidebar {
            (screen.width() * 0.21).clamp(200.0, 270.0)
        } else {
            0.0
        };
        let side_reserve = if reader.sidebar {
            side_width + 32.0
        } else {
            0.0
        };
        let canvas = Rect::from_min_max(
            screen.min,
            pos2(
                (screen.max.x - side_reserve).max(screen.min.x + 120.0),
                screen.max.y,
            ),
        );

        let mut canvas_ui = workspace::child(ui, "comic_canvas", canvas);
        if reader.pages_job.is_some() {
            let status_box = Rect::from_center_size(canvas.center(), vec2(240.0, 40.0));
            let mut loading_ui = workspace::child(&mut canvas_ui, "comic_loading", status_box);
            loading_ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.spinner();
                ui.label(l.text("正在加载章节…", "Loading chapter…"));
            });
        }
        if let Some(error) = &reader.pages_error {
            let err_box = Rect::from_center_size(canvas.center(), vec2(360.0, 96.0));
            let mut err_ui = workspace::child(&mut canvas_ui, "comic_error", err_box);
            err_ui.vertical_centered(|ui| {
                ui.colored_label(Color32::LIGHT_RED, error);
                ui.add_space(8.0);
                if design::control(
                    ui,
                    l.text("重试章节", "Retry chapter"),
                    Some(icons::refresh),
                    design::Control::Outline,
                    0.0,
                )
                .clicked()
                {
                    retry = true;
                }
            });
        }
        let count = reader.urls.len();
        let mut requested = vec![reader.current];
        if count > 0 {
            let viewport_h = canvas.height();
            let avail_w =
                (canvas.width() - ui.spacing().scroll.allocated_width() - 32.0).max(120.0);
            let compare = reader.display == Display::Compare;
            let zoom = reader.zoom;
            let size_for = |index: usize| {
                let raw = reader.sizes.get(&index).copied().unwrap_or((800.0, 1120.0));
                page_dimensions(viewport_h, avail_w, raw, compare, zoom)
            };
            let content_width = (0..count).map(|i| size_for(i).x).fold(0.0_f32, f32::max);
            let offset = (0..reader.current)
                .map(|i| size_for(i).y + 10.0)
                .sum::<f32>()
                + size_for(reader.current).y * reader.fraction;
            let mut scroll = egui::ScrollArea::both()
                .id_salt(("comic_vertical", reader.book.key(), reader.chapter.key()))
                .auto_shrink([false, false])
                .content_margin(0)
                .animated(false);
            let restore = reader.restore;
            if reader.restore {
                scroll = scroll
                    .vertical_scroll_offset(offset)
                    .horizontal_scroll_offset(
                        ((content_width - canvas.width() + ui.spacing().scroll.allocated_width())
                            * 0.5)
                            .max(0.0),
                    );
                reader.restore = false;
            }
            let output = scroll.show_viewport(&mut canvas_ui, |ui, view| {
                center_scroll_content(ui, view, content_width, restore);
                if key_scroll_y != 0.0 {
                    ui.scroll_with_delta(vec2(0.0, key_scroll_y));
                }
                ui.spacing_mut().item_spacing.y = 10.0;
                let viewport = ui.clip_rect();
                let mut closest = (f32::MAX, reader.current, reader.fraction);
                for index in 0..count {
                    let page_size = size_for(index);
                    // Keep the entire viewport scrollable, including the
                    // margins around a centered page.
                    let (row, _) = ui.allocate_exact_size(
                        vec2(view.width().max(content_width), page_size.y),
                        Sense::hover(),
                    );
                    let rect = Rect::from_center_size(row.center(), page_size);
                    let response = ui.interact(rect, ui.id().with(("page", index)), Sense::click());
                    if !viewport.intersects(rect) {
                        continue;
                    }
                    requested.push(index);
                    let top = (viewport.min.y - rect.min.y).max(0.0);
                    let distance = (rect.min.y - viewport.min.y).abs();
                    let distance = if rect.contains(pos2(rect.center().x, viewport.min.y + 2.0)) {
                        0.0
                    } else {
                        distance
                    };
                    if distance < closest.0 {
                        closest = (distance, index, (top / page_size.y).clamp(0.0, 1.0));
                    }
                    paint_page(ui, rect, reader.assets.get(&index), reader.display, l);
                    if let Some(error) = reader.errors.get(&index) {
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            error,
                            egui::FontId::proportional(13.0),
                            Color32::LIGHT_RED,
                        );
                    }
                    if response.double_clicked() {
                        reader.zoom = if reader.zoom > 1.1 { 1.0 } else { 1.5 };
                        reader.restore = true;
                    }
                    if index == count - 1
                        && rect.max.y <= viewport.max.y + 2.0
                        && reader.assets.contains_key(&index)
                    {
                        reader.completed = true;
                        reader.dirty = true;
                    }
                }
                closest
            });
            let (_, index, fraction) = output.inner;
            if reader.current != index || (reader.fraction - fraction).abs() > 0.005 {
                reader.current = index;
                reader.fraction = fraction;
                reader.dirty = true;
            }
        }
        if controls {
            overlay(ui, "reader-header", header, egui::Order::Middle, |ui| {
                window::drag_region(ui);
                ui.painter().rect_filled(header, 0, BACKGROUND);
                ui.painter().line_segment(
                    [header.left_bottom(), header.right_bottom()],
                    Stroke::new(1.0, design::LINE),
                );
                let pad_y = (controls::BAR_HEIGHT - design::CONTROL_HEIGHT) * 0.5;
                let caption = Rect::from_min_max(
                    pos2(screen.min.x + 16.0, header.min.y + pad_y),
                    pos2(
                        screen.max.x - window::controls_width(ui.ctx()) - 16.0,
                        header.max.y - pad_y,
                    ),
                );
                let mut caption_ui = workspace::child(ui, "reader-caption", caption);
                caption_ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if design::back_button(ui, l.text("返回目录", "Back to chapters")).clicked()
                    {
                        back = true;
                    }
                    ui.add(egui::Label::new(RichText::new(&reader.book.title).strong()).truncate());
                    let (sep_rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                    icons::paint(ui.painter(), sep_rect, MUTED, icons::CARET_RIGHT);
                    ui.add(
                        egui::Label::new(RichText::new(&reader.chapter.name).color(MUTED))
                            .truncate(),
                    );
                });
                window::caption_resize_edges(ui, screen);
            });
        }

        if reader.sidebar {
            let rect = Rect::from_min_max(
                pos2(screen.max.x - side_width - 20.0, header.max.y + 8.0),
                pos2(screen.max.x - 20.0, footer.min.y - 8.0),
            );
            ui.painter().rect_filled(rect, 10, SURFACE);
            ui.painter().rect_stroke(
                rect,
                10,
                Stroke::new(1.0, design::LINE),
                egui::StrokeKind::Inside,
            );
            let mut sidebar = workspace::child(ui, "reader-chapter-panel", rect.shrink(14.0));
            sidebar.horizontal(|ui| {
                ui.label(
                    RichText::new(l.text("章节目录", "Chapters"))
                        .size(17.0)
                        .strong(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if design::compact_icon_button(
                        ui,
                        icons::SIDEBAR_SIMPLE,
                        l.text("收起章节栏", "Collapse chapters"),
                        false,
                    )
                    .clicked()
                    {
                        reader.sidebar = false;
                    }
                });
            });
            sidebar.add_space(8.0);
            let entry = self
                .library
                .as_ref()
                .and_then(|lib| lib.get(&reader.book.key()))
                .cloned();
            if let Some(entry) = entry {
                let chapters: Vec<_> = entry
                    .chapters
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.kind == reader.chapter.kind)
                    .collect();
                let position = chapters
                    .iter()
                    .position(|(i, _)| *i == reader.index)
                    .unwrap_or(0);
                let mut scroll = design::scroll()
                    .id_salt(("reader_chapters", reader.chapter.kind as u8))
                    .auto_shrink([false, false]);
                if reader.sidebar_follow {
                    scroll = scroll.vertical_scroll_offset(
                        (position as f32 * 80.0 - rect.height() * 0.4).max(0.0),
                    );
                }
                scroll.show_rows(&mut sidebar, 70.0, chapters.len(), |ui, rows| {
                    for row_index in rows {
                        let (index, chapter) = chapters[row_index];
                        let current = index == reader.index;
                        let (row, response) = ui
                            .allocate_exact_size(vec2(ui.available_width(), 70.0), Sense::click());
                        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                        if current || response.hovered() {
                            ui.painter().rect_filled(
                                row,
                                8,
                                if current { ACCENT } else { BACKGROUND },
                            );
                        }
                        let art = Rect::from_min_size(
                            row.min + vec2(8.0, 9.0),
                            vec2(52.0 * COVER_RATIO, 52.0),
                        );
                        if let Some(cover) = self.covers.get(&reader.book.key()) {
                            cover.cover(ui, art, 4);
                        } else {
                            ui.painter().rect_filled(art, 4, BACKGROUND);
                            ui.painter().text(
                                art.center(),
                                egui::Align2::CENTER_CENTER,
                                (index + 1).to_string(),
                                egui::FontId::proportional(14.0),
                                MUTED,
                            );
                        }
                        let mut text = workspace::child(
                            ui,
                            ("chapter", index),
                            Rect::from_min_max(
                                row.min + vec2(56.0, 12.0),
                                row.max - vec2(8.0, 6.0),
                            ),
                        );
                        text.style_mut().interaction.selectable_labels = false;
                        text.add(
                            egui::Label::new(RichText::new(&chapter.name).color(if current {
                                BACKGROUND
                            } else {
                                design::TEXT
                            }))
                            .truncate(),
                        );
                        text.label(
                            RichText::new(if current {
                                l.text("正在阅读", "Reading")
                            } else if entry.read.contains(chapter.key()) {
                                l.text("已读", "Read")
                            } else if entry.new_unread.contains(chapter.key()) {
                                l.text("新章节", "New chapter")
                            } else {
                                l.text("未读", "Unread")
                            })
                            .small()
                            .color(if current {
                                BACKGROUND.gamma_multiply(0.75)
                            } else {
                                MUTED
                            }),
                        );
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &chapter.name)
                        });
                        if current && reader.sidebar_follow {
                            ui.scroll_to_rect(row, Some(Align::Center));
                        }
                        if response.on_hover_text(&chapter.name).clicked() {
                            chapter_action = Some(index);
                        }
                    }
                });
            }
        }
        reader.sidebar_follow = false;
        let neighbors = self
            .library
            .as_ref()
            .and_then(|lib| lib.get(&reader.book.key()))
            .map_or([None, None], |entry| {
                chapter_neighbors(&entry.chapters, reader.index)
            });
        let overlay_rect = if controls { bar.footer } else { bar.status };
        let actions = if controls || has_status {
            overlay(
                ui,
                "reader-footer",
                overlay_rect,
                egui::Order::Foreground,
                |ui| controls::show(ui, reader, &bar, neighbors, l, controls, status),
            )
        } else {
            controls::Actions::default()
        };
        chapter_action = actions.chapter.or(chapter_action);
        let translate = actions.translate;
        retry |= actions.retry;
        let cancel_translation = actions.cancel;
        let policy = self
            .source
            .as_ref()
            .map(|source| source.image_policy(&reader.book))
            .unwrap_or_default();
        let now = Instant::now();
        for index in &requested {
            if let Some(asset) = reader.assets.get_mut(index) {
                asset.last_used = now;
            }
        }
        reader.visible = requested.clone();
        let requested = page_requests(reader.current, count, requested, policy);
        reader.maintain_requests(&requested, policy.concurrency, now);
        if let Some(position) = self.translations.iter().position(|r| {
            r.book == reader.book.key()
                && r.chapter == reader.chapter.key()
                && r.index == reader.current
        }) && let Some(request) = self.translations.remove(position)
        {
            self.translations.push_front(request);
        }
        if cancel_translation {
            self.translation = None;
            self.translations.clear();
            self.translation_total = self.translation_done;
        }
        if back {
            self.flush();
            self.reader = None;
            self.view = View::Detail;
            self.translations.clear();
            return;
        }
        if retry
            && let Some(reader) = &mut self.reader
            && let Some(source) = self.source.clone()
        {
            let book = reader.book.clone();
            let chapter = reader.chapter.clone();
            reader.jobs.clear();
            reader.errors.clear();
            reader.pages_error = None;
            reader.assets.clear();
            let policy = source.image_policy(&book);
            for index in (0..reader.urls.len()).filter(|_| !policy.retain_cache_on_refresh) {
                let key = format!("{}:{}:{index}", book.key(), chapter.key());
                let _ = std::fs::remove_file(
                    domain::root()
                        .join("cache/pages")
                        .join(format!("{}.img", domain::digest(key.as_bytes()))),
                );
            }
            reader.pages_job = Some(Job::spawn(move |_| async move {
                source.pages(&book, &chapter, true).await
            }));
        }
        if let Some(index) = chapter_action {
            self.read_chapter(index, settings, false);
            return;
        }
        for index in requested {
            self.ensure_page(index, settings);
        }
        if let Some(all) = translate {
            self.queue_translation(all, settings);
        }
        ui.allocate_space(vec2(ui.available_width(), screen.height().max(0.0)));
    }
}
// Separate layers keep page clicks and wheel events out of visible toolbars.
// Their rectangles never participate in the reading canvas layout.
fn overlay<R>(
    ui: &Ui,
    id: &str,
    rect: Rect,
    order: egui::Order,
    content: impl FnOnce(&mut Ui) -> R,
) -> R {
    egui::Area::new(ui.id().with(id))
        .order(order)
        .fixed_pos(rect.min)
        .default_size(rect.size())
        .constrain(false)
        .movable(false)
        .sense(Sense::hover())
        .show(ui.ctx(), |area| {
            area.set_max_size(rect.size());
            area.set_min_size(rect.size());
            let mut child = workspace::child(area, id, rect);
            content(&mut child)
        })
        .inner
}

/// Recenter when zoom, scrollbars or window geometry changes; retain user
/// horizontal panning while the viewport and content dimensions stay stable.
fn center_scroll_content(ui: &Ui, viewport: Rect, width: f32, reset: bool) {
    let id = ui.id().with("comic-horizontal-center");
    let geometry = vec2(viewport.width(), width);
    let changed = ui.ctx().data_mut(|data| {
        let previous = data.get_temp::<Vec2>(id);
        data.insert_temp(id, geometry);
        previous.is_none_or(|previous| (previous - geometry).abs().max_elem() > 0.5)
    });
    if reset || changed {
        let center = ((width - viewport.width()) * 0.5).max(0.0);
        ui.scroll_with_delta(vec2(viewport.min.x - center, 0.0));
    }
}

fn paint_page(ui: &Ui, rect: Rect, asset: Option<&Asset>, display: Display, l: Language) {
    ui.painter().rect_filled(rect, 0, BACKGROUND);
    let Some(asset) = asset else {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            l.text("正在加载图片…", "Loading image…"),
            egui::FontId::proportional(14.0),
            MUTED,
        );
        return;
    };
    match display {
        Display::Original => asset.original.paint(ui, rect),
        Display::Translated => {
            if let Some(translated) = &asset.translated {
                translated.paint(ui, rect);
            } else {
                paint_translation_pending(ui, rect, l);
            }
        }
        Display::Compare => {
            let width = (rect.width() - 10.0) / 2.0;
            let left = Rect::from_min_size(rect.min, vec2(width, rect.height()));
            let right = Rect::from_min_size(pos2(left.max.x + 10.0, rect.min.y), left.size());
            asset.original.paint(ui, left);
            if let Some(translated) = &asset.translated {
                translated.paint(ui, right);
            } else {
                paint_translation_pending(ui, right, l);
            }
        }
    }
}
fn paint_translation_pending(ui: &Ui, rect: Rect, l: Language) {
    ui.painter().rect_filled(rect, 6, SURFACE);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        l.text("本页尚未翻译", "Not translated yet"),
        egui::FontId::proportional(16.0),
        MUTED,
    );
}
fn reader_slot(ui: &mut Ui, id: &str, rect: Rect) -> Ui {
    workspace::child(ui, id, rect)
}

pub(super) fn reader_book(p: &Painter, r: Rect, c: Color32) {
    icons::paint(p, r, c, icons::BOOK_OPEN);
}
