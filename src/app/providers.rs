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
            if ui
                .button(l.text("重新读取密钥", "Reload saved keys"))
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
                if provider_editor(ui, provider, state, l) {
                    remove = Some(provider.id.clone());
                }
            });
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);
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
        if workspace::primary(ui, l.text("添加提供商", "Add provider")).clicked() {
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
        let width = (ui.available_width() - 92.0).max(40.0);
        if open {
            ui.add(
                design::text_edit(&mut provider.name)
                    .hint_text(l.text("提供商名称", "Provider name"))
                    .desired_width(width),
            );
        } else {
            let response = ui
                .add_sized(
                    vec2(width, design::CONTROL_HEIGHT),
                    egui::Label::new(&provider.name)
                        .truncate()
                        .sense(Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.clicked() {
                state.expanded = Some(provider.id.clone());
            }
        }
        if design::control(
            ui,
            "",
            Some(if open { chevron_down } else { chevron_right }),
            design::Control::Quiet,
            34.0,
        )
        .on_hover_text(l.text("展开 / 收起", "Expand / collapse"))
        .clicked()
        {
            state.expanded = if open {
                None
            } else {
                Some(provider.id.clone())
            };
        }
        if design::control(ui, "", Some(draw_trash), design::Control::Quiet, 34.0)
            .on_hover_text(l.text("移除提供商", "Remove provider"))
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
    secret_field(ui, &mut provider.api_key, l);
    if !before.same_connection(provider) {
        state.pending.remove(&provider.id);
        state.models.remove(&provider.id);
        state.errors.remove(&provider.id);
    }
    super::preferences::form_row(ui, l.text("模型", "Models"), l, |ui| {
        let label = format!("{} {}", provider.models.len(), l.text("个模型", "models"));
        let width = ui.available_width();
        let response = design::control(ui, &label, None, design::Control::Outline, width);
        chevron_down(
            ui.painter(),
            Rect::from_center_size(
                pos2(response.rect.max.x - 16.0, response.rect.center().y),
                vec2(15.0, 11.0),
            ),
            MUTED,
        );
        egui::Popup::menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| models_panel(ui, provider, state, l));
    });
    remove
}

fn models_panel(ui: &mut Ui, provider: &mut Provider, state: &mut State, l: Language) {
    ui.set_width((ui.ctx().content_rect().width() - 32.0).clamp(200.0, 340.0));
    let query_id = ui.id().with("search");
    let mut query = ui
        .data_mut(|d| d.get_temp::<String>(query_id))
        .unwrap_or_default();
    let loading = state.pending.contains_key(&provider.id);
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 46.0).max(120.0);
        ui.add(
            design::text_edit(&mut query)
                .desired_width(width)
                .hint_text(l.text("搜索模型", "Search models")),
        );
        if loading {
            ui.spinner();
        } else if ui
            .add_enabled_ui(!provider.base_url.trim().is_empty(), |ui| {
                design::control(ui, "", Some(icons::refresh), design::Control::Quiet, 34.0)
                    .on_hover_text(l.text("获取模型", "Fetch models"))
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
        .show_rows(ui, 34.0, filtered.len(), |ui, rows| {
            for index in rows {
                let id = filtered[index];
                let mut selected = provider.models.contains(id);
                if ui.checkbox(&mut selected, id).changed() {
                    if selected {
                        provider.models.push(id.clone());
                    } else {
                        provider.models.retain(|m| m != id);
                    }
                }
            }
        });
    ui.collapsing(l.text("手动添加", "Add manually"), |ui| {
        ui.horizontal(|ui| {
            let manual = state.manual.entry(provider.id.clone()).or_default();
            let width = (ui.available_width() - 72.0).max(100.0);
            ui.add(
                design::text_edit(manual)
                    .hint_text(l.text("模型 ID", "Model ID"))
                    .desired_width(width),
            );
            if ui
                .add_enabled(
                    !manual.trim().is_empty(),
                    egui::Button::new(l.text("添加", "Add")),
                )
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
fn chevron_down(p: &Painter, r: Rect, c: Color32) {
    icons::paint(
        p,
        Rect::from_center_size(r.center(), Vec2::splat(18.0)),
        c,
        icons::CARET_DOWN,
    );
}
fn chevron_right(p: &Painter, r: Rect, c: Color32) {
    icons::paint(
        p,
        Rect::from_center_size(r.center(), Vec2::splat(18.0)),
        c,
        icons::CARET_RIGHT,
    );
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
    egui::ComboBox::from_id_salt("business-model")
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(ui.available_width().clamp(120.0, 440.0))
        .selected_text(current)
        .show_ui(ui, |ui| {
            let search_id = ui.id().with("model-search");
            let mut query = ui
                .data_mut(|d| d.get_temp::<String>(search_id))
                .unwrap_or_default();
            ui.add(
                design::text_edit(&mut query)
                    .hint_text(l.text("搜索模型或提供商", "Search models or providers")),
            );
            ui.data_mut(|d| d.insert_temp(search_id, query.clone()));
            let query = query.to_lowercase();
            if design::selectable_value(
                ui,
                selected,
                ModelRef::default(),
                l.text("未配置", "Not configured"),
            )
            .clicked()
            {
                ui.close();
            }
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
                ui.separator();
                ui.label(RichText::new(&provider.name).strong());
                for model in models {
                    if design::selectable_value(
                        ui,
                        selected,
                        ModelRef {
                            provider: provider.id.clone(),
                            model: model.clone(),
                        },
                        model,
                    )
                    .clicked()
                    {
                        ui.close();
                    }
                }
            }
        })
        .response
}

fn text_field(ui: &mut Ui, label: &str, value: &mut String, hint: &str, l: Language) {
    super::preferences::form_row(ui, label, l, |ui| {
        ui.add(
            design::text_edit(value)
                .hint_text(hint)
                .desired_width(f32::INFINITY),
        );
    });
}

pub(super) fn secret_field(ui: &mut Ui, value: &mut String, l: Language) {
    super::preferences::form_row(ui, "API Key", l, |ui| secret_input(ui, value, l));
}

fn secret_input(ui: &mut Ui, value: &mut String, l: Language) {
    let response = ui.add(
        design::text_edit(value)
            .password(true)
            .margin(egui::Margin {
                left: 12,
                right: 34,
                top: 7,
                bottom: 7,
            })
            .hint_text(l.text("输入 API Key", "Enter API key"))
            .desired_width(f32::INFINITY),
    );
    if !value.is_empty() {
        let rect = Rect::from_center_size(
            pos2(response.rect.max.x - 17.0, response.rect.center().y),
            vec2(26.0, 26.0),
        );
        let clear = ui
            .interact(rect, response.id.with("clear"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(l.text("清除密钥", "Clear API key"));
        draw_close(
            ui.painter(),
            rect.shrink(7.0),
            if clear.hovered() { design::TEXT } else { MUTED },
        );
        if clear.clicked() {
            value.clear();
            response.request_focus();
        }
    }
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
