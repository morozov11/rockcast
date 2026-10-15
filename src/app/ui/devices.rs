//! egui panel widgets.

use eframe::egui::{self, RichText, Ui, Vec2};
use egui_taffy::{
    TuiBuilderLogic,
    taffy::{
        self,
        prelude::{AlignItems, FlexDirection, Size, auto, length, percent},
    },
    tui,
};

use super::super::RockCastApp;
use super::super::theme::*;

impl RockCastApp {
    pub(in crate::app) fn draw_device_row(&mut self, ui: &mut Ui) {
        let t = self.lang.t();
        let labels: Vec<String> = self.devices.iter().map(|d| d.label(self.lang)).collect();

        let selected_text = match self.selected_device.and_then(|i| labels.get(i)) {
            Some(s) => s.clone(),
            None if labels.is_empty() => {
                if self.loading_devices {
                    t.searching.into()
                } else {
                    t.device_none.into()
                }
            }
            None => t.device_none.into(),
        };

        let selected_idx = self.selected_device;
        let loading_devices = self.loading_devices;

        let mut new_selection: Option<usize> = None;
        let mut refresh_clicked = false;

        tui(ui, ui.id().with("device_row"))
            .reserve_available_width()
            .style(taffy::Style {
                flex_direction: FlexDirection::Row,
                align_items: Some(AlignItems::Center),
                gap: length(8.0_f32),
                size: Size {
                    width: percent(1.0_f32),
                    height: length(28.0_f32),
                },
                ..Default::default()
            })
            .show(|tui| {
                // Device label
                tui.style(taffy::Style {
                    flex_shrink: 0.0,
                    size: Size {
                        width: auto(),
                        height: length(28.0_f32),
                    },
                    ..Default::default()
                })
                .ui(|ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                    ui.horizontal_centered(|ui| {
                        ui.label(RichText::new(t.device).color(MUTED));
                    });
                });

                // Device selector ComboBox: flexes responsively between 160px and 480px
                tui.style(taffy::Style {
                    flex_grow: 1.0,
                    flex_shrink: 1.0,
                    min_size: Size {
                        width: length(160.0_f32),
                        height: length(28.0_f32),
                    },
                    max_size: Size {
                        width: length(480.0_f32),
                        height: length(28.0_f32),
                    },
                    ..Default::default()
                })
                .ui(|ui| {
                    ui.horizontal_centered(|ui| {
                        let combo_w = ui.available_width();
                        egui::ComboBox::from_id_salt("device")
                            .selected_text(RichText::new(&selected_text).color(FG))
                            .width(combo_w)
                            .height(28.0)
                            .show_ui(ui, |ui| {
                                ui.set_min_width(combo_w);
                                if labels.is_empty() {
                                    ui.label(RichText::new(t.nothing_found).color(MUTED));
                                    return;
                                }
                                for (i, label) in labels.iter().enumerate() {
                                    let selected = selected_idx == Some(i);
                                    if ui
                                        .selectable_label(selected, RichText::new(label).color(FG))
                                        .clicked()
                                    {
                                        new_selection = Some(i);
                                    }
                                }
                            });
                    });
                });

                // Find devices button
                tui.style(taffy::Style {
                    flex_shrink: 0.0,
                    size: Size {
                        width: length(125.0_f32),
                        height: length(26.0_f32),
                    },
                    ..Default::default()
                })
                .ui(|ui| {
                    ui.horizontal_centered(|ui| {
                        let find = egui::Button::new(RichText::new(t.find).color(FG))
                            .min_size(Vec2::new(125.0, 26.0))
                            .fill(PANEL_2);
                        if ui.add_enabled(!loading_devices, find).clicked() {
                            refresh_clicked = true;
                        }
                    });
                });
            });

        if let Some(i) = new_selection {
            let prev = self.selected_device;
            self.selected_device = Some(i);
            if let Some(d) = self.devices.get(i) {
                let kind = if d.is_local() { "local" } else { "cast" };
                log::info!(
                    "output device chosen: idx={i} (was {prev:?}) kind={kind} id={} name='{}'",
                    d.id(),
                    d.name()
                );
            }
            self.mark_settings_dirty();
        }

        if refresh_clicked {
            self.refresh_devices();
        }
    }
}
