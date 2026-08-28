use super::super::RockCastApp;
use crate::session::{AccountClient, OsCredentialStore};
use eframe::egui::{self, Context, RichText};

impl RockCastApp {
    fn account_client(&self) -> AccountClient<OsCredentialStore> {
        AccountClient::new(self.rockserver.clone(), OsCredentialStore)
    }

    pub(in crate::app) fn draw_account_window(&mut self, ctx: &Context) {
        if !self.account_open {
            return;
        }
        let mut open = self.account_open;
        egui::Window::new("Account & devices")
            .open(&mut open)
            .resizable(true)
            .show(ctx, |ui| {
                ui.label("Optional account connection. Radio and voice remain available without an account.");

                if self.pairing.is_none() {
                    ui.separator();
                    ui.label(RichText::new("Connect this PC to an account").strong());
                    ui.label("This connects RockCast to an existing Rock account; it does not create a separate RockCast account.");
                    ui.horizontal(|ui| {
                        ui.label("PC name");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.pairing_device_name)
                                .desired_width(260.0)
                                .char_limit(128),
                        );
                    });
                    let can_start = !self.pairing_device_name.trim().is_empty();
                    if ui
                        .add_enabled(can_start, egui::Button::new("Connect this PC to an account"))
                        .clicked()
                    {
                        let name = self.pairing_device_name.trim().to_owned();
                        match self.account_client().create_pairing(&name) {
                            Ok(pairing) => {
                                self.pairing_device_name = pairing.device_display_name.clone();
                                self.pairing_status = "Waiting for browser approval…".into();
                                self.pairing = Some(pairing);
                                self.account_message.clear();
                            }
                            Err(error) => {
                                self.account_message = format!(
                                    "Could not start account connection: {error} Local radio is unchanged."
                                );
                            }
                        }
                    }
                }

                if let Some(pairing) = self.pairing.clone() {
                    ui.separator();
                    ui.label(RichText::new("Connect this PC using your phone").strong());
                    if pairing.status == "pending" {
                        ui.label(format!("Status: {}", self.pairing_status));
                    } else {
                        ui.label("Status: This pairing request is no longer pending.");
                    }
                    ui.label(format!(
                        "Device: {} — {}",
                        product_name(&pairing.device_type),
                        pairing.device_display_name
                    ));
                    ui.label(format!("Verification phrase: {}", pairing.verification_phrase));
                    ui.label(format!("Short code: {}", pairing.short_code));
                    ui.label(format!("Expires: {}", pairing.expires_at));
                    let link = pairing.deep_link(self.rockserver.base_url());
                    draw_qr(ui, &link);
                    if ui.button("Open secure pairing link").clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab(link.clone()));
                    }
                    ui.small("The QR and link belong to this one pairing request. Secrets stay in memory and are never shown or saved.");
                    if ui.button("Cancel connection").clicked() {
                        if let Some(cancel) = &self.pairing_cancel {
                            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                        self.pairing = None;
                        self.pairing_cancel = None;
                        self.pairing_status.clear();
                        self.account_message = "Connection cancelled. Local radio remains available.".into();
                    }
                }

                ui.separator();
                if ui.button("Refresh account & devices").clicked() {
                    let client = self.account_client();
                    let _ = client.refresh();
                    match (client.profile(), client.devices()) {
                        (Ok(profile), Ok(devices)) => {
                            self.account_profile = Some(profile);
                            self.account_devices = devices;
                            self.account_message.clear();
                        }
                        _ => {
                            self.account_message = "Account is unavailable or the session has expired. Local radio is unchanged.".into();
                        }
                    }
                }
                if let Some(profile) = &self.account_profile {
                    ui.label(format!(
                        "This PC is connected to account «{}».",
                        profile.account_display_name
                    ));
                    ui.label(format!(
                        "Current device: {} — {}",
                        product_name(&profile.device_type),
                        profile.device_display_name
                    ));
                }

                let current_device_id = self
                    .account_profile
                    .as_ref()
                    .map(|profile| profile.device_id.as_str());
                let mut revoke = None;
                for device in &self.account_devices {
                    let current = current_device_id == Some(device.device_id.as_str());
                    ui.horizontal(|ui| {
                        ui.label(format!(
                            "{} — {}",
                            product_name(&device.device_type),
                            device.device_display_name
                        ));
                        if current {
                            ui.small("This PC");
                        } else if ui.button("Revoke").clicked() {
                            revoke = Some((device.device_id.clone(), device.device_display_name.clone()));
                        }
                    });
                    if !device.created_at.is_empty() {
                        ui.small(format!("Connected: {}", device.created_at));
                    }
                    if let Some(last_seen_at) = &device.last_seen_at {
                        ui.small(format!("Last activity: {last_seen_at}"));
                    }
                }
                if let Some((device_id, device_name)) = revoke {
                    match self.account_client().revoke_device(&device_id) {
                        Ok(()) => {
                            self.account_devices.retain(|d| d.device_id != device_id);
                            self.account_message = format!("Device «{device_name}» was revoked.");
                        }
                        Err(error) => self.account_message = error.to_string(),
                    }
                }
                if self.account_profile.is_some() && ui.button("Log out on this PC").clicked() {
                    let _ = self.account_client().logout();
                    self.account_profile = None;
                    self.account_devices.clear();
                    self.account_message = "Local credentials were removed.".into();
                }
                if !self.account_message.is_empty() {
                    ui.label(&self.account_message);
                }
            });
        self.account_open = open;
    }
}

fn product_name(device_type: &str) -> &'static str {
    if device_type.to_ascii_lowercase().contains("mobile") {
        "RockMobile"
    } else {
        "RockCast"
    }
}

fn draw_qr(ui: &mut egui::Ui, value: &str) {
    let Ok(code) = qrcode::QrCode::new(value.as_bytes()) else {
        return;
    };
    let size = 180.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let module = size / code.width() as f32;
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::WHITE);
    for y in 0..code.width() {
        for x in 0..code.width() {
            if code[(x, y)] == qrcode::Color::Dark {
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        rect.min + egui::vec2(x as f32 * module, y as f32 * module),
                        egui::vec2(module + 0.5, module + 0.5),
                    ),
                    0.0,
                    egui::Color32::BLACK,
                );
            }
        }
    }
}
