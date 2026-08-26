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
        egui::Window::new("Account & devices").open(&mut open).resizable(true).show(ctx, |ui| {
            ui.label("Optional account connection. Radio and voice remain available without it.");
            if self.pairing.is_none() && ui.button("Connect this PC").clicked() {
                match self.account_client().create_pairing(&format!("RockCast on {}", std::env::var("COMPUTERNAME").unwrap_or_else(|_| "this PC".into()))) {
                    Ok(pairing) => { self.pairing = Some(pairing); self.account_message.clear(); }
                    Err(error) => self.account_message = error.to_string(),
                }
            }
            if let Some(pairing) = &self.pairing {
                ui.separator();
                ui.label(RichText::new("Pair this PC in your browser").strong());
                ui.label(format!("Short code: {}", pairing.short_code));
                ui.label(format!("Verification phrase: {}", pairing.verification_phrase));
                let link = pairing.deep_link(self.rockserver.base_url());
                draw_qr(ui, &link);
                if ui.button("Open secure pairing link").clicked() { ctx.open_url(egui::OpenUrl::new_tab(link)); }
                ui.small("The one-time pairing secret is held only in memory and is never written to settings or logs.");
                ui.label("After browser approval, RockCast finishes automatically.");
                if ui.button("Cancel pairing").clicked() {
                    if let Some(cancel) = &self.pairing_cancel { cancel.store(true, std::sync::atomic::Ordering::Relaxed); }
                    self.pairing = None;
                    self.pairing_cancel = None;
                    self.account_message = "Pairing cancelled.".into();
                }
            }
            ui.separator();
            if ui.button("Refresh account & devices").clicked() {
                let client = self.account_client();
                let _ = client.refresh();
                match (client.profile(), client.devices()) { (Ok(profile), Ok(devices)) => { self.account_profile = Some(profile); self.account_devices = devices; self.account_message.clear(); }, _ => self.account_message = "Account is unavailable or the session has expired. Local radio is unchanged.".into() }
            }
            if let Some(profile) = &self.account_profile { ui.label(format!("Connected device: {}", profile.device_id)); }
            let mut revoke = None;
            for device in &self.account_devices { ui.horizontal(|ui| { ui.label(format!("{} ({})", device.name, device.platform)); if ui.button("Revoke").clicked() { revoke = Some(device.device_id.clone()); } }); }
            if let Some(device_id) = revoke { match self.account_client().revoke_device(&device_id) { Ok(()) => { self.account_devices.retain(|d| d.device_id != device_id); self.account_message = "Device revoked.".into(); }, Err(error) => self.account_message = error.to_string() } }
            if self.account_profile.is_some() && ui.button("Log out on this PC").clicked() { let _ = self.account_client().logout(); self.account_profile = None; self.account_devices.clear(); self.account_message = "Local credentials were removed.".into(); }
            if !self.account_message.is_empty() { ui.label(&self.account_message); }
        });
        self.account_open = open;
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
