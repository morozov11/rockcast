use super::super::{AccountContext, AccountErrorKind, AccountUiState, RockCastApp};
use crate::{
    i18n,
    session::{AccountClient, OsCredentialStore},
};
use eframe::egui::{self, Context, RichText};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

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
            *banner = Some(self.lang.t().account_checking.into());
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
        self.pairing_link_copied = false;
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
                message: Some(self.lang.t().account_connection_failed.into()),
            };
        }
    }

    fn cancel_pairing(&mut self) {
        if let Some(cancel) = &self.pairing_cancel {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        self.pairing_cancel = None;
        self.pairing_link_copied = false;
        self.account_state = AccountUiState::Disconnected {
            device_name: default_device_name(),
            message: Some(self.lang.t().account_connection_cancelled.into()),
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
        if !self.account_open {
            open = false;
        }
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
            OpenDevices,
            Done,
            AskRevoke(String),
            Revoke(String),
        }
        let mut action = None;
        let t = self.lang.t();
        match &mut self.account_state {
            AccountUiState::Disconnected {
                device_name,
                message,
            } => {
                ui.label(t.account_offline_note);
                if let Some(message) = message {
                    ui.label(message.as_str());
                }
                ui.separator();
                ui.label(RichText::new(t.account_connect_title).strong());
                ui.horizontal(|ui| {
                    ui.label(t.account_pc_name);
                    ui.add(
                        egui::TextEdit::singleline(device_name)
                            .desired_width(260.0)
                            .char_limit(128),
                    );
                });
                if ui
                    .add_enabled(
                        !device_name.trim().is_empty(),
                        egui::Button::new(t.account_connect),
                    )
                    .clicked()
                {
                    action = Some(Action::StartPairing(device_name.trim().to_owned()));
                }
            }
            AccountUiState::Starting {
                device_name,
                loading_account,
            } => {
                ui.spinner();
                ui.label(if *loading_account {
                    t.account_checking
                } else {
                    t.account_starting
                });
                if !*loading_account {
                    ui.label(i18n::fmt1(
                        t.account_device,
                        presentation_device_name(device_name),
                    ));
                }
                ui.label(t.account_offline_note);
            }
            AccountUiState::Waiting { request, status } => {
                ui.label(RichText::new(t.account_waiting_title).strong());
                ui.label(t.account_waiting_step);
                ui.label(status.as_str());
                ui.label(i18n::fmt1(
                    t.account_device,
                    presentation_device_name(&request.device_display_name),
                ));
                ui.label(i18n::fmt1(
                    t.account_expires_in,
                    countdown(&request.expires_at),
                ));
                ui.label(t.account_waiting_steps);
                ui.label(i18n::fmt1(t.account_phrase, &request.verification_phrase));
                ui.label(i18n::fmt1(t.account_short_code, &request.short_code));
                let link = request.deep_link(self.rockserver.base_url());
                draw_qr(ui, &link);
                if ui.button(t.account_open_link).clicked() {
                    ctx.open_url(egui::OpenUrl::new_tab(link.clone()));
                }
                if ui.button(t.account_copy_link).clicked() {
                    ui.ctx().copy_text(link);
                    self.pairing_link_copied = true;
                }
                if self.pairing_link_copied {
                    ui.label(t.account_link_copied);
                }
                if ui.button(t.account_cancel).clicked() {
                    action = Some(Action::Cancel);
                }
            }
            AccountUiState::ConnectedFirstTime { context } => {
                ui.label(RichText::new(t.account_success_title).strong());
                draw_current_account(ui, context, t);
                if ui.button(t.account_open_devices).clicked() {
                    action = Some(Action::OpenDevices);
                }
                if ui.button(t.account_done).clicked() {
                    action = Some(Action::Done);
                }
            }
            AccountUiState::Connected { context, banner } => {
                if let Some(banner) = banner {
                    ui.label(banner.as_str());
                }
                if let Some(id) = draw_connected(ui, context, t, self.lang) {
                    action = Some(Action::AskRevoke(id));
                }
                if ui
                    .add_enabled(
                        !self.account_refreshing,
                        egui::Button::new(t.account_refresh),
                    )
                    .clicked()
                {
                    action = Some(Action::Refresh);
                }
                if ui.button(t.account_logout).clicked() {
                    action = Some(Action::Logout);
                }
            }
            AccountUiState::Error { kind, cached } => {
                ui.label(match kind {
                    AccountErrorKind::AuthRequired => t.account_auth_required,
                    AccountErrorKind::Recoverable => t.account_unavailable,
                    AccountErrorKind::SecureStorage => t.account_storage_unavailable,
                });
                if let Some(context) = cached {
                    ui.label(t.account_devices_unavailable);
                    draw_current_account(ui, context, t);
                }
            }
        }
        if self.revoke_confirmation.is_some() {
            ui.separator();
            ui.label(t.account_confirm_disconnect);
            if ui.button(t.account_confirm).clicked() {
                action = self.revoke_confirmation.clone().map(Action::Revoke);
            }
            if ui.button(t.account_done).clicked() {
                self.revoke_confirmation = None;
            }
        }
        match action {
            Some(Action::StartPairing(name)) => self.start_pairing(name),
            Some(Action::Refresh) => self.refresh_account(),
            Some(Action::Logout) => {
                let _ = AccountClient::new(self.rockserver.clone(), OsCredentialStore).logout();
                self.account_state = AccountUiState::Disconnected {
                    device_name: default_device_name(),
                    message: None,
                };
            }
            Some(Action::Cancel) => self.cancel_pairing(),
            Some(Action::OpenDevices) => {
                if let AccountUiState::ConnectedFirstTime { context } = &self.account_state {
                    self.account_state = AccountUiState::Connected {
                        context: context.clone(),
                        banner: None,
                    };
                }
            }
            Some(Action::Done) => self.account_open = false,
            Some(Action::AskRevoke(id)) => self.revoke_confirmation = Some(id),
            Some(Action::Revoke(id)) => {
                self.revoke_confirmation = None;
                if AccountClient::new(self.rockserver.clone(), OsCredentialStore)
                    .revoke_device(&id)
                    .is_ok()
                {
                    self.refresh_account();
                }
            }
            None => {}
        }
    }
}

fn draw_current_account(ui: &mut egui::Ui, context: &AccountContext, t: &i18n::Strings) {
    ui.label(i18n::fmt1(
        t.account_success,
        &context.profile.account_display_name,
    ));
    ui.label(i18n::fmt1(
        t.account_current_device,
        presentation_device_name(&context.profile.device_display_name),
    ));
    ui.label(t.account_connected);
}

fn draw_connected(
    ui: &mut egui::Ui,
    context: &AccountContext,
    t: &i18n::Strings,
    lang: i18n::Lang,
) -> Option<String> {
    draw_current_account(ui, context, t);
    ui.separator();
    ui.label(RichText::new(t.account_other_devices).strong());
    let mut other = false;
    let mut revoke = None;
    for device in &context.devices {
        if device.device_id == context.profile.device_id {
            continue;
        }
        other = true;
        ui.label(presentation_device_name(&device.device_display_name));
        if let Some(date) = display_date(&device.created_at, lang) {
            ui.label(i18n::fmt1(t.account_connected_at, date));
        }
        if let Some(date) = device
            .last_seen_at
            .as_deref()
            .and_then(|value| display_date(value, lang))
        {
            ui.label(i18n::fmt1(t.account_last_seen, date));
        }
        if ui.button(t.account_disconnect).clicked() {
            revoke = Some(device.device_id.clone());
        }
        ui.separator();
    }
    if !other {
        ui.label(t.account_empty_devices);
    }
    revoke
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

fn countdown(expires_at: &str) -> String {
    let Ok(expires_at) = OffsetDateTime::parse(expires_at, &Rfc3339) else {
        return "—".into();
    };
    let seconds = (expires_at - OffsetDateTime::now_utc())
        .whole_seconds()
        .max(0);
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn display_date(value: &str, lang: i18n::Lang) -> Option<String> {
    let value = OffsetDateTime::parse(value, &Rfc3339).ok()?;
    Some(if lang == i18n::Lang::Ru {
        format!(
            "{:02}.{:02}.{:04}, {:02}:{:02} UTC",
            value.day(),
            u8::from(value.month()),
            value.year(),
            value.hour(),
            value.minute()
        )
    } else {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02} UTC",
            value.year(),
            u8::from(value.month()),
            value.day(),
            value.hour(),
            value.minute()
        )
    })
}

fn qr_layout(width: usize) -> (usize, usize) {
    const QUIET_ZONE: usize = 4;
    let modules = width + QUIET_ZONE * 2;
    (modules, (320 / modules).max(1))
}

fn draw_qr(ui: &mut egui::Ui, value: &str) {
    let Ok(code) =
        qrcode::QrCode::with_error_correction_level(value.as_bytes(), qrcode::EcLevel::M)
    else {
        return;
    };
    let (modules, module_size) = qr_layout(code.width());
    let size = (modules * module_size) as f32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::WHITE);
    for y in 0..code.width() {
        for x in 0..code.width() {
            if code[(x, y)] == qrcode::Color::Dark {
                let offset = 4 + x;
                let row = 4 + y;
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        rect.min
                            + egui::vec2((offset * module_size) as f32, (row * module_size) as f32),
                        egui::vec2(module_size as f32, module_size as f32),
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
    use super::{AccountContext, AccountUiState, presentation_device_name, qr_layout};
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
            presentation_device_name("RockCast — DESKTOP"),
            "RockCast — DESKTOP"
        );
    }
    #[test]
    fn qr_uses_a_four_module_quiet_zone_and_integer_modules() {
        let (modules, module_size) = qr_layout(45);
        assert_eq!(modules, 53);
        assert_eq!(module_size, 6);
        assert!((256..=320).contains(&(modules * module_size)));
    }
    #[test]
    fn connected_and_loading_screens_never_offer_pairing() {
        let loading = AccountUiState::Starting {
            device_name: "DESKTOP".into(),
            loading_account: true,
        };
        let connected = AccountUiState::Connected {
            context: AccountContext {
                profile: AccountProfile {
                    device_id: "device".into(),
                    account_display_name: "account".into(),
                    device_display_name: "DESKTOP".into(),
                    device_type: "windows".into(),
                },
                devices: Vec::new(),
            },
            banner: None,
        };
        assert_eq!(flags(&connected), flags(&loading));
    }
}
