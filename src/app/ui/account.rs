use super::super::{AccountContext, AccountErrorKind, AccountUiState, RockCastApp};
use crate::session::{AccountClient, OsCredentialStore};
use eframe::egui::{self, Context, RichText};

impl RockCastApp {
    fn begin_account_load(&mut self) {
        if self.account_load_started {
            return;
        }
        self.account_load_started = true;
        self.account_state = AccountUiState::Starting {
            device_name: default_device_name(),
            loading_account: true,
        };
        self.spawn_account_load();
    }

    fn refresh_account(&mut self) {
        if self.account_refreshing {
            return;
        }
        self.account_refreshing = true;
        if let AccountUiState::Connected { banner, .. } = &mut self.account_state {
            *banner = Some("Refreshing account…".into());
        }
        self.spawn_account_load();
    }

    fn spawn_account_load(&mut self) {
        let tx = self.ui_tx.clone();
        let config = self.rockserver.clone();
        if self
            .playback
            .spawn_job(move |_| {
                let client = AccountClient::new(config, OsCredentialStore);
                let result = match client.has_credentials() {
                    Ok(false) => Ok(None),
                    Ok(true) => client.refresh().and_then(|_| {
                        client.profile().and_then(|profile| {
                            client.devices().map(|devices| Some((profile, devices)))
                        })
                    }),
                    Err(error) => Err(error),
                };
                let _ = tx.send(super::super::messages::UiMsg::AccountLoaded(result));
            })
            .is_err()
        {
            self.account_refreshing = false;
            self.account_state = AccountUiState::Error {
                kind: AccountErrorKind::Recoverable,
                cached: None,
            };
        }
    }

    fn start_pairing(&mut self, name: String) {
        let fallback_name = name.clone();
        self.account_state = AccountUiState::Starting {
            device_name: name.clone(),
            loading_account: false,
        };
        let tx = self.ui_tx.clone();
        let config = self.rockserver.clone();
        if self
            .playback
            .spawn_job(move |_| {
                let result = AccountClient::new(config, OsCredentialStore).create_pairing(&name);
                let _ = tx.send(super::super::messages::UiMsg::PairingStarted { name, result });
            })
            .is_err()
        {
            self.account_state = AccountUiState::Disconnected {
                device_name: fallback_name,
                message: Some(
                    "Could not start account connection. Local radio is unchanged.".into(),
                ),
            };
        }
    }

    fn cancel_pairing(&mut self) {
        if let Some(cancel) = &self.pairing_cancel {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.pairing_cancel = None;
        self.account_state = AccountUiState::Disconnected {
            device_name: default_device_name(),
            message: Some("Connection cancelled. Local radio remains available.".into()),
        };
    }

    pub(in crate::app) fn draw_account_window(&mut self, ctx: &Context) {
        if !self.account_open {
            return;
        }
        self.begin_account_load();
        let mut open = self.account_open;
        egui::Window::new(self.lang.t().account_title)
            .open(&mut open)
            .resizable(true)
            .show(ctx, |ui| self.draw_account_state(ui, ctx));
        if self.account_open
            && !open
            && matches!(self.account_state, AccountUiState::Waiting { .. })
        {
            self.cancel_pairing();
        }
        if !open {
            self.account_load_started = false;
            self.account_refreshing = false;
        }
        self.account_open = open;
    }

    fn draw_account_state(&mut self, ui: &mut egui::Ui, ctx: &Context) {
        enum Action {
            StartPairing(String),
            Refresh,
            Logout,
            Cancel,
        }
        let mut action = None;
        match &mut self.account_state {
            AccountUiState::Disconnected {
                device_name,
                message,
            } => {
                ui.label(self.lang.t().account_offline_note);
                if let Some(message) = message {
                    ui.label(message.as_str());
                }
                ui.separator();
                ui.label(RichText::new(self.lang.t().account_connect_title).strong());
                ui.horizontal(|ui| {
                    ui.label("PC name");
                    ui.add(
                        egui::TextEdit::singleline(device_name)
                            .desired_width(260.0)
                            .char_limit(128),
                    );
                });
                let start = ui
                    .add_enabled(
                        !device_name.trim().is_empty(),
                        egui::Button::new(self.lang.t().account_connect),
                    )
                    .clicked();
                if start {
                    action = Some(Action::StartPairing(device_name.trim().to_owned()));
                }
            }
            AccountUiState::Starting {
                device_name,
                loading_account,
            } => {
                ui.spinner();
                ui.label(if *loading_account {
                    "Checking account…"
                } else {
                    self.lang.t().account_starting
                });
                if !*loading_account {
                    ui.label(format!("Device: {}", presentation_device_name(device_name)));
                }
                ui.label(self.lang.t().account_offline_note);
            }
            AccountUiState::Waiting { request, status } => {
                ui.label(RichText::new("Connect this PC using your phone").strong());
                ui.label(format!("Status: {status}"));
                ui.label(format!(
                    "Device: {}",
                    presentation_device_name(&request.device_display_name)
                ));
                ui.label(format!(
                    "Verification phrase: {}",
                    request.verification_phrase
                ));
                ui.label(format!("Short code: {}", request.short_code));
                let link = request.deep_link(self.rockserver.base_url());
                draw_qr(ui, &link);
                if ui.button("Open secure pairing link").clicked() {
                    ctx.open_url(egui::OpenUrl::new_tab(link));
                }
                if ui.button("Cancel connection").clicked() {
                    action = Some(Action::Cancel);
                }
            }
            AccountUiState::ConnectedFirstTime { context } => {
                draw_connected(ui, context, Some("Account connected."), false)
            }
            AccountUiState::Connected { context, banner } => {
                draw_connected(ui, context, banner.as_deref(), true);
                if ui
                    .add_enabled(
                        !self.account_refreshing,
                        egui::Button::new("Refresh account & devices"),
                    )
                    .clicked()
                {
                    action = Some(Action::Refresh);
                }
                if ui.button("Log out on this PC").clicked() {
                    action = Some(Action::Logout);
                }
            }
            AccountUiState::Error { kind, cached } => {
                ui.label(match kind {
                    AccountErrorKind::AuthRequired => {
                        "Your account session needs reconnecting. Local radio remains available."
                    }
                    AccountErrorKind::Recoverable => {
                        "Account service is temporarily unavailable. Local radio remains available."
                    }
                    AccountErrorKind::SecureStorage => {
                        "Secure credential storage is unavailable. Local radio remains available."
                    }
                });
                if let Some(context) = cached {
                    draw_connected(ui, context, Some("Try refresh again later."), false);
                }
            }
        }
        match action {
            Some(Action::StartPairing(name)) => self.start_pairing(name),
            Some(Action::Refresh) => self.refresh_account(),
            Some(Action::Logout) => {
                let _ = AccountClient::new(self.rockserver.clone(), OsCredentialStore).logout();
                self.account_state = AccountUiState::Disconnected {
                    device_name: default_device_name(),
                    message: Some("Local credentials were removed.".into()),
                };
            }
            Some(Action::Cancel) => self.cancel_pairing(),
            None => {}
        }
    }
}

fn draw_connected(
    ui: &mut egui::Ui,
    context: &AccountContext,
    banner: Option<&str>,
    show_devices: bool,
) {
    if let Some(banner) = banner {
        ui.label(banner);
    }
    ui.label(format!(
        "This PC is connected to account «{}».",
        context.profile.account_display_name
    ));
    ui.label(format!(
        "Current device: {}",
        presentation_device_name(&context.profile.device_display_name)
    ));
    if show_devices {
        for device in &context.devices {
            ui.label(presentation_device_name(&device.device_display_name));
        }
    }
}

fn default_device_name() -> String {
    super::super::default_pairing_device_name()
}

pub(super) fn presentation_device_name(value: &str) -> String {
    let raw = value.trim();
    format!(
        "RockCast — {}",
        raw.strip_prefix("RockCast — ")
            .or_else(|| raw.strip_prefix("RockCast - "))
            .unwrap_or(raw)
    )
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

#[cfg(test)]
mod tests {
    use super::{AccountContext, AccountUiState, presentation_device_name};
    use crate::session::AccountProfile;

    #[derive(Debug, PartialEq, Eq)]
    struct ScreenFlags {
        connect: bool,
        qr: bool,
        expired_error: bool,
    }

    fn flags(state: &AccountUiState) -> ScreenFlags {
        match state {
            AccountUiState::Disconnected { .. } => ScreenFlags {
                connect: true,
                qr: false,
                expired_error: false,
            },
            AccountUiState::Starting { .. }
            | AccountUiState::ConnectedFirstTime { .. }
            | AccountUiState::Connected { .. } => ScreenFlags {
                connect: false,
                qr: false,
                expired_error: false,
            },
            AccountUiState::Waiting { .. } => ScreenFlags {
                connect: false,
                qr: true,
                expired_error: false,
            },
            AccountUiState::Error { .. } => ScreenFlags {
                connect: false,
                qr: false,
                expired_error: true,
            },
        }
    }
    #[test]
    fn device_presentation_adds_one_rockcast_prefix() {
        assert_eq!(
            presentation_device_name("DESKTOP-685GRAQ"),
            "RockCast — DESKTOP-685GRAQ"
        );
        assert_eq!(
            presentation_device_name("RockCast — DESKTOP-685GRAQ"),
            "RockCast — DESKTOP-685GRAQ"
        );
    }

    #[test]
    fn connected_and_loading_screens_never_offer_pairing() {
        let loading = AccountUiState::Starting {
            device_name: "DESKTOP-685GRAQ".into(),
            loading_account: true,
        };
        assert_eq!(
            flags(&loading),
            ScreenFlags {
                connect: false,
                qr: false,
                expired_error: false,
            }
        );
        let connected = AccountUiState::Connected {
            context: AccountContext {
                profile: AccountProfile {
                    device_id: "device".into(),
                    account_display_name: "account".into(),
                    device_display_name: "DESKTOP-685GRAQ".into(),
                    device_type: "windows".into(),
                },
                devices: Vec::new(),
            },
            banner: None,
        };
        assert_eq!(flags(&connected), flags(&loading));
    }
}
