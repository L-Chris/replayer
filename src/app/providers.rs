use super::*;
use crate::ai::{ModelRef, Provider};

struct Pending {
    connection: Provider,
    job: crate::jobs::Job<Vec<String>>,
}
struct Models {
    connection: Provider,
    ids: Vec<String>,
}
#[derive(Default)]
pub(super) struct State {
    pending: HashMap<String, Pending>,
    models: HashMap<String, Models>,
    errors: HashMap<String, String>,
    manual: HashMap<String, String>,
    expanded: Option<String>,
    initialized: bool,
}

impl State {
    fn poll(&mut self, settings: &crate::ai::Settings, ctx: &egui::Context) {
        self.pending.retain(|id, pending| {
            let current = settings.providers.iter().find(|p| &p.id == id);
            if !current.is_some_and(|p| p.same_connection(&pending.connection)) {
                return false;
            }
            let Some(result) = pending.job.poll() else {
                return true;
            };
            match result {
                Ok(ids) => {
                    self.errors.remove(id);
                    self.models.insert(
                        id.clone(),
                        Models {
                            connection: pending.connection.clone(),
                            ids,
                        },
                    );
                }
                Err(error) => {
                    self.errors.insert(id.clone(), error.to_string());
                }
            }
            false
        });
        self.models.retain(|id, models| {
            settings
                .providers
                .iter()
                .any(|p| &p.id == id && p.same_connection(&models.connection))
        });
        self.errors
            .retain(|id, _| settings.providers.iter().any(|p| &p.id == id));
        if !self.pending.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}

impl App {
    pub(super) fn preferences_providers(&mut self, ui: &mut Ui) {
        let l = self.settings.language;
        let state = &mut self.providers_ui;
        state.poll(&self.preferences_draft.ai, ui.ctx());
        if !state.initialized {
            state.expanded = self
                .preferences_draft
                .ai
                .providers
                .first()
                .map(|p| p.id.clone());
            state.initialized = true;
        }
        if let Some(error) = &self.preferences_draft.credential_error {
            ui.colored_label(Color32::LIGHT_RED, l.error(error));
            if design::control(
                ui,
                l.text("重新读取密钥", "Reload saved keys"),
                Some(icons::refresh),
                design::Control::Outline,
                0.0,
            )
            .clicked()
            {
                self.config_message = match self
                    .preferences_draft
                    .load_credentials(&crate::credentials::SystemVault)
                {
                    Ok(()) => l.text("已重新读取密钥", "Keys reloaded").into(),
                    Err(error) => l.error(&error.to_string()),
                };
            }
        }
        let mut remove = None;
        for provider in &mut self.preferences_draft.ai.providers {
            ui.push_id(provider.id.clone(), |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(21, 25, 28))
                    .stroke(Stroke::new(1.0, design::LINE))
                    .corner_radius(10)
                    .inner_margin(16)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 12.0;
                        if provider_editor(ui, provider, state, l) {
                            remove = Some(provider.id.clone());
                        }
                    });
            });
            ui.add_space(4.0);
        }
        if let Some(id) = remove {
            self.preferences_draft.ai.providers.retain(|p| p.id != id);
            state.pending.remove(&id);
            if state.expanded.as_deref() == Some(&id) {
                state.expanded = self
                    .preferences_draft
                    .ai
                    .providers
                    .first()
                    .map(|p| p.id.clone());
            }
            if self.preferences_draft.llm.selected.provider == id {
                self.preferences_draft.llm.selected = Default::default();
            }
            if self.preferences_draft.comics.image_selected.provider == id {
                self.preferences_draft.comics.image_selected = Default::default();
            }
        }
        if design::control(
            ui,
            l.text("添加提供商", "Add provider"),
            Some(draw_add),
            design::Control::Outline,
            0.0,
        )
        .clicked()
        {
            let provider = Provider {
                name: l.text("新提供商", "New provider").into(),
                ..Provider::new()
            };
            state.expanded = Some(provider.id.clone());
            self.preferences_draft.ai.providers.push(provider);
        }
    }
}

fn provider_editor(ui: &mut Ui, provider: &mut Provider, state: &mut State, l: Language) -> bool {
    let open = state.expanded.as_deref() == Some(&provider.id);
    let mut remove = false;
    super::preferences::form_row(ui, l.text("名称", "Name"), l, |ui| {
        let width = (ui.available_width() - 80.0).max(40.0);
        if open {
            design::input_field(
                ui,
                &mut provider.name,
                width,
                l.text("提供商名称", "Provider name"),
            );
        } else {
            let response = ui
                .add_sized(
                    vec2(width, design::CONTROL_HEIGHT),
                    egui::Label::new(RichText::new(&provider.name).strong())
                        .truncate()
                        .sense(Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.clicked() {
                state.expanded = Some(provider.id.clone());
            }
        }
        if design::compact_icon_button(
            ui,
            if open {
                icons::CARET_UP
            } else {
                icons::CARET_DOWN
            },
            l.text("展开 / 收起", "Expand / collapse"),
            false,
        )
        .clicked()
        {
            state.expanded = if open {
                None
            } else {
                Some(provider.id.clone())
            };
        }
        if design::compact_icon_button(
            ui,
            icons::TRASH,
            l.text("移除提供商", "Remove provider"),
            false,
        )
        .clicked()
        {
            remove = true;
        }
    });
    if !open {
        return remove;
    }
    let before = provider.clone();
    text_field(
        ui,
        l.text("服务地址", "Base URL"),
        &mut provider.base_url,
        "https://api.example.com/v1",
        l,
    );
    secret_field(ui, &provider.id, &mut provider.api_key, l);
    if !before.same_connection(provider) {
        state.pending.remove(&provider.id);
        state.models.remove(&provider.id);
        state.errors.remove(&provider.id);
    }
    super::preferences::form_row(ui, l.text("模型", "Models"), l, |ui| {
        let label = format!("{} {}", provider.models.len(), l.text("个模型", "models"));
        let width = ui.available_width();
        let (rect, response) =
            ui.allocate_exact_size(vec2(width, design::CONTROL_HEIGHT), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &label)
        });
        let popup = egui::Popup::menu(&response)
            .gap(4.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
        let is_open = popup.is_open();
        let hovered = (response.hovered() || response.has_focus()) && ui.is_enabled();
        ui.painter().rect_filled(
            rect,
            8,
            if is_open {
                design::FIELD_FOCUS
            } else if hovered {
                design::FIELD_HOVER
            } else {
                design::FIELD
            },
        );
        ui.painter().rect_stroke(
            rect,
            8,
            Stroke::new(
                if is_open { 1.5 } else { 1.0 },
                if is_open {
                    ACCENT
                } else if hovered {
                    design::BORDER
                } else {
                    design::LINE
                },
            ),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            pos2(rect.min.x + 12.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            &label,
            egui::FontId::proportional(14.0),
            design::TEXT,
        );
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(rect.max.x - 16.0, rect.center().y), vec2(14.0, 14.0)),
            if is_open { ACCENT } else { MUTED },
            if is_open {
                icons::CARET_UP
            } else {
                icons::CARET_DOWN
            },
        );
        let _ = response.clone().on_hover_cursor(egui::CursorIcon::PointingHand);
        popup.show(|ui| models_panel(ui, provider, state, l));
    });
    remove
}

fn models_panel(ui: &mut Ui, provider: &mut Provider, state: &mut State, l: Language) {
    ui.set_width((ui.ctx().content_rect().width() - 32.0).clamp(220.0, 350.0));
    ui.spacing_mut().item_spacing.y = 8.0;
    let query_id = ui.id().with("search");
    let mut query = ui
        .data_mut(|d| d.get_temp::<String>(query_id))
        .unwrap_or_default();
    let loading = state.pending.contains_key(&provider.id);
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 48.0).max(120.0);
        design::search_field(ui, &mut query, width, l.text("搜索模型", "Search models"));
        if loading {
            ui.spinner();
        } else if ui
            .add_enabled_ui(!provider.base_url.trim().is_empty(), |ui| {
                design::icon_button(
                    ui,
                    icons::ARROWS_CLOCKWISE,
                    l.text("获取模型", "Fetch models"),
                    false,
                )
            })
            .inner
            .clicked()
        {
            let connection = provider.clone();
            let request = connection.clone();
            state.errors.remove(&provider.id);
            state.pending.insert(
                provider.id.clone(),
                Pending {
                    connection,
                    job: crate::jobs::Job::spawn(move |_| async move {
                        crate::ai::fetch_models(&request).await
                    }),
                },
            );
            ui.ctx().request_repaint();
        }
    });
    ui.data_mut(|d| d.insert_temp(query_id, query.clone()));
    if let Some(error) = state.errors.get(&provider.id) {
        ui.colored_label(Color32::LIGHT_RED, l.error(error));
    }
    let mut ids = provider.models.clone();
    if let Some(models) = state.models.get(&provider.id) {
        ids.extend(models.ids.iter().cloned());
    }
    ids.sort();
    ids.dedup();
    let query = query.to_lowercase();
    let filtered: Vec<_> = ids
        .iter()
        .filter(|m| m.to_lowercase().contains(&query))
        .collect();
    design::scroll()
        .auto_shrink([false, true])
        .max_height((ui.available_height() - 150.0).clamp(0.0, 220.0))
        .show_rows(ui, 32.0, filtered.len(), |ui, rows| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for index in rows {
                let id = filtered[index];
                let selected = provider.models.contains(id);
                let row_w = ui.available_width().max(120.0);
                let (rect, resp) = ui.allocate_exact_size(vec2(row_w, 30.0), Sense::click());
                if selected || resp.hovered() {
                    ui.painter().rect_filled(
                        rect,
                        6,
                        if selected {
                            design::SELECTED
                        } else {
                            design::HOVER
                        },
                    );
                }
                if selected {
                    icons::paint(
                        ui.painter(),
                        Rect::from_center_size(
                            pos2(rect.min.x + 14.0, rect.center().y),
                            vec2(14.0, 14.0),
                        ),
                        ACCENT,
                        icons::CHECK,
                    );
                }
                let text_clip = Rect::from_min_max(
                    pos2(rect.min.x + 28.0, rect.min.y),
                    pos2(rect.max.x - 8.0, rect.max.y),
                )
                .intersect(ui.clip_rect());
                let galley = ui.painter().layout_no_wrap(
                    id.into(),
                    egui::FontId::proportional(13.5),
                    if selected { ACCENT } else { design::TEXT },
                );
                ui.painter().with_clip_rect(text_clip).galley(
                    pos2(rect.min.x + 28.0, rect.center().y - galley.size().y * 0.5),
                    galley,
                    design::TEXT,
                );
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    if selected {
                        provider.models.retain(|m| m != id);
                    } else {
                        provider.models.push(id.clone());
                    }
                }
            }
        });
    ui.collapsing(l.text("手动添加", "Add manually"), |ui| {
        ui.horizontal(|ui| {
            let manual = state.manual.entry(provider.id.clone()).or_default();
            let width = (ui.available_width() - 76.0).max(100.0);
            design::input_field(ui, manual, width, l.text("模型 ID", "Model ID"));
            let can_add = !manual.trim().is_empty();
            if ui
                .add_enabled_ui(can_add, |ui| {
                    design::control(
                        ui,
                        l.text("添加", "Add"),
                        None,
                        design::Control::Primary,
                        64.0,
                    )
                })
                .inner
                .clicked()
            {
                let id = manual.trim().to_owned();
                if !provider.models.contains(&id) {
                    provider.models.push(id);
                }
                manual.clear();
            }
        });
    });
}

pub(super) fn model_selector(
    ui: &mut Ui,
    selected: &mut ModelRef,
    ai: &crate::ai::Settings,
    l: Language,
) -> Response {
    let current = ai
        .providers
        .iter()
        .find(|p| p.id == selected.provider)
        .filter(|p| p.models.contains(&selected.model))
        .map(|p| format!("{} / {}", p.name, selected.model))
        .unwrap_or_else(|| {
            if selected.provider.is_empty() {
                l.text("请选择模型", "Select a model").into()
            } else {
                l.text("模型已移除，请重新选择", "Model removed; select another")
                    .into()
            }
        });
    let width = ui.available_width().clamp(120.0, 360.0);
    ui.push_id("business-model", |ui| {
        let (rect, response) =
            ui.allocate_exact_size(vec2(width, design::CONTROL_HEIGHT), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, ui.is_enabled(), &current)
        });
        let popup = egui::Popup::menu(&response)
            .gap(4.0)
            .width(width.max(240.0))
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
        let is_open = popup.is_open();
        let hovered = (response.hovered() || response.has_focus()) && ui.is_enabled();
        ui.painter().rect_filled(
            rect,
            8,
            if is_open {
                design::FIELD_FOCUS
            } else if hovered {
                design::FIELD_HOVER
            } else {
                design::FIELD
            },
        );
        ui.painter().rect_stroke(
            rect,
            8,
            Stroke::new(
                if is_open { 1.5 } else { 1.0 },
                if is_open {
                    ACCENT
                } else if hovered {
                    design::BORDER
                } else {
                    design::LINE
                },
            ),
            egui::StrokeKind::Inside,
        );
        let text_clip = Rect::from_min_max(
            pos2(rect.min.x + 12.0, rect.min.y),
            pos2((rect.max.x - 32.0).max(rect.min.x + 12.0), rect.max.y),
        )
        .intersect(ui.clip_rect());
        let galley = ui.painter().layout_no_wrap(
            current.into(),
            egui::FontId::proportional(14.0),
            if selected.provider.is_empty() {
                MUTED
            } else {
                design::TEXT
            },
        );
        ui.painter().with_clip_rect(text_clip).galley(
            pos2(rect.min.x + 12.0, rect.center().y - galley.size().y * 0.5),
            galley,
            design::TEXT,
        );
        icons::paint(
            ui.painter(),
            Rect::from_center_size(pos2(rect.max.x - 16.0, rect.center().y), vec2(14.0, 14.0)),
            if is_open { ACCENT } else { MUTED },
            if is_open {
                icons::CARET_UP
            } else {
                icons::CARET_DOWN
            },
        );
        popup.show(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 2.0);
            let search_id = ui.id().with("model-search");
            let mut query = ui
                .data_mut(|d| d.get_temp::<String>(search_id))
                .unwrap_or_default();
            design::search_field(
                ui,
                &mut query,
                ui.available_width(),
                l.text("搜索模型或提供商", "Search models or providers"),
            );
            ui.add_space(4.0);
            ui.data_mut(|d| d.insert_temp(search_id, query.clone()));
            let query = query.to_lowercase();
            design::menu_value(
                ui,
                selected,
                ModelRef::default(),
                l.text("未配置", "Not configured"),
            );
            for provider in &ai.providers {
                if provider.models.is_empty() {
                    continue;
                }
                let models: Vec<_> = provider
                    .models
                    .iter()
                    .filter(|model| {
                        provider.name.to_lowercase().contains(&query)
                            || model.to_lowercase().contains(&query)
                    })
                    .collect();
                if models.is_empty() {
                    continue;
                }
                ui.add_space(4.0);
                ui.label(RichText::new(&provider.name).size(12.0).color(MUTED));
                for model in models {
                    design::menu_value(
                        ui,
                        selected,
                        ModelRef {
                            provider: provider.id.clone(),
                            model: model.clone(),
                        },
                        model,
                    );
                }
            }
        });
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    })
    .inner
}

fn text_field(ui: &mut Ui, label: &str, value: &mut String, hint: &str, l: Language) {
    super::preferences::form_row(ui, label, l, |ui| {
        design::input_field(ui, value, ui.available_width(), hint);
    });
}

pub(super) fn secret_field(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    value: &mut String,
    l: Language,
) {
    super::preferences::form_row(ui, "API Key", l, |ui| {
        design::secret_field(
            ui,
            id_salt,
            value,
            ui.available_width(),
            l.text("输入 API Key", "Enter API key"),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn model_results_are_discarded_after_editing_or_removing_a_connection() {
        let ctx = egui::Context::default();
        let provider = Provider {
            id: "p".into(),
            name: "P".into(),
            base_url: "https://old.example/v1".into(),
            ..Default::default()
        };
        for replacement in [
            None,
            Some(Provider {
                api_key: "changed-key".into(),
                ..provider.clone()
            }),
        ] {
            let mut state = State::default();
            state.pending.insert(
                provider.id.clone(),
                Pending {
                    connection: provider.clone(),
                    job: crate::jobs::Job::spawn(|_| async { Ok(vec!["obsolete".into()]) }),
                },
            );
            state.models.insert(
                provider.id.clone(),
                Models {
                    connection: provider.clone(),
                    ids: vec!["obsolete".into()],
                },
            );
            state.poll(
                &crate::ai::Settings {
                    providers: replacement.into_iter().collect(),
                },
                &ctx,
            );
            assert!(state.pending.is_empty() && state.models.is_empty());
        }
    }
}
