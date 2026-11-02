mod script;
pub mod state;

use std::cell::Cell;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use aegis_ffi::OwnerCheck;
use aegis_vault::{EntryId, VaultError};
use arboard::Clipboard;
use eframe::egui;
use zeroize::Zeroizing;

pub use script::{READ_FORM_SCRIPT, fill_script, https_origin, same_origin};
use state::VaultState;

thread_local! { static HAS_LOGINS: Cell<bool> = const { Cell::new(false) }; }
pub fn has_logins() -> bool {
    HAS_LOGINS.get()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VaultView {
    None,
    Locked,
    Open,
}

fn vault_view(exists: bool, locked: bool) -> VaultView {
    if !exists {
        VaultView::None
    } else if locked {
        VaultView::Locked
    } else {
        VaultView::Open
    }
}

#[cfg(test)]
mod view_tests {
    use super::{VaultView, vault_view};

    #[test]
    fn picks_view_from_vault_state() {
        assert_eq!(vault_view(false, true), VaultView::None);
        assert_eq!(vault_view(true, true), VaultView::Locked);
        assert_eq!(vault_view(true, false), VaultView::Open);
    }
}

#[derive(Clone, Copy)]
pub enum ProtectedAction {
    Reveal(EntryId),
    Copy(EntryId),
    Edit(EntryId),
    Fill(EntryId),
    Update(EntryId),
    Import,
}

pub enum Action {
    Gate(ProtectedAction),
    Save,
    Delete(EntryId),
    SetMaster,
    Lock,
    Create,
    Unlock,
    Cancel,
}

pub struct Passwords {
    pub state: VaultState,
    pub search: String,
    pub origin: String,
    pub username: String,
    pub password: Zeroizing<String>,
    pub show_password: bool,
    pub edit: Option<EntryId>,
    pub delete: Option<EntryId>,
    pub revealed: Option<(EntryId, Zeroizing<String>, Instant)>,
    copied: Option<(Zeroizing<String>, Instant)>,
    pub master_input: Zeroizing<String>,
    pub master_show: bool,
    pub master_prompt: bool,
    pub master_change: bool,
    pub master_confirm_old: bool,
    pub master_remove: bool,
    pub master_as_gate: bool,
    pub menu_open: bool,
    pub gate_pending: Option<ProtectedAction>,
    gate_rx: Option<Receiver<OwnerCheck>>,
    pub message: Option<String>,
    active_origin: Option<String>,
    last_url: Option<String>,
}

impl Passwords {
    pub fn unavailable(path: std::path::PathBuf, error: String) -> Self {
        let mut state = Self::new_empty(state::VaultState::unavailable(path));
        state.message = Some(error);
        state
    }

    pub fn new(path: std::path::PathBuf) -> Result<Self, VaultError> {
        Ok(Self::new_empty(VaultState::new(path)?))
    }

    fn new_empty(state: VaultState) -> Self {
        Self {
            state,
            search: String::new(),
            origin: String::new(),
            username: String::new(),
            password: Zeroizing::new(String::new()),
            show_password: false,
            edit: None,
            delete: None,
            revealed: None,
            copied: None,
            master_input: Zeroizing::new(String::new()),
            master_show: false,
            master_prompt: false,
            master_change: false,
            master_confirm_old: false,
            master_remove: false,
            master_as_gate: false,
            menu_open: false,
            gate_pending: None,
            gate_rx: None,
            message: None,
            active_origin: None,
            last_url: None,
        }
    }

    pub fn update_origin(&mut self, url: Option<&str>) {
        if self.last_url.as_deref() == url {
            return;
        }
        self.last_url = url.map(str::to_owned);
        let origin = url.and_then(https_origin);
        if self.active_origin == origin {
            return;
        }
        HAS_LOGINS.set(
            origin
                .as_deref()
                .is_some_and(|origin| !self.state.for_origin(origin).is_empty()),
        );
        self.active_origin = origin;
        self.menu_open = false;
    }

    pub fn refresh_origin(&mut self) {
        HAS_LOGINS.set(
            self.active_origin
                .as_deref()
                .is_some_and(|origin| !self.state.for_origin(origin).is_empty()),
        );
    }

    pub fn active_origin(&self) -> Option<&str> {
        self.active_origin.as_deref()
    }

    pub fn request_gate(
        &mut self,
        action: ProtectedAction,
        frame: &eframe::Frame,
        ctx: &egui::Context,
    ) {
        if !self.state.exists() {
            if !matches!(action, ProtectedAction::Import) {
                self.message = Some("Create a password vault first".to_owned());
                return;
            }
            match self.state.create(None) {
                Ok(()) => self.refresh_origin(),
                Err(VaultError::KeystoreUnavailable) => {
                    self.master_prompt = true;
                    self.master_as_gate = true;
                    self.gate_pending = Some(action);
                    return;
                }
                Err(error) => {
                    self.message = Some(error.to_string());
                    return;
                }
            }
        }
        if matches!(action, ProtectedAction::Fill(_)) && self.state.verified() {
            self.gate_pending = Some(action);
            return;
        }
        if self.state.locked() && self.state.has_master() {
            self.master_prompt = true;
            self.gate_pending = Some(action);
            return;
        }
        if !aegis_ffi::owner_check_available() {
            if self.state.has_master() {
                self.master_prompt = true;
                self.master_as_gate = true;
                self.gate_pending = Some(action);
            } else {
                self.message = Some("Set a master password to access saved passwords".into());
            }
            return;
        }
        if let Err(error) = self.state.open_without_master() {
            if !self.state.has_master() {
                self.message = Some(error.to_string());
            }
            return;
        }
        let Some(window) = frame.winit_window() else {
            return;
        };
        let (sender, receiver) = mpsc::channel();
        let wake = ctx.clone();
        match aegis_ffi::verify_owner(
            window.as_ref(),
            "Access saved passwords",
            Box::new(move |result| {
                let _ = sender.send(result);
                wake.request_repaint();
            }),
        ) {
            Ok(()) => {
                self.gate_rx = Some(receiver);
                self.gate_pending = Some(action);
            }
            Err(error) => {
                self.message = Some(error.to_string());
            }
        }
    }

    pub fn poll_gate(&mut self) -> Option<ProtectedAction> {
        if self.gate_rx.is_none() && !self.master_prompt {
            return self.gate_pending.take();
        }
        let result = self.gate_rx.as_ref()?.try_recv().ok()?;
        self.gate_rx = None;
        match result {
            OwnerCheck::Verified => {
                let action = self.gate_pending.take();
                if matches!(action, Some(ProtectedAction::Fill(_))) {
                    self.state.mark_verified();
                }
                action
            }
            OwnerCheck::Denied => {
                self.gate_pending = None;
                self.message = Some("Owner check was cancelled".into());
                None
            }
            OwnerCheck::Unavailable => {
                if self.state.has_master() {
                    self.master_prompt = true;
                    self.master_as_gate = true;
                } else {
                    self.gate_pending = None;
                    self.message = Some("Set a master password to access saved passwords".into());
                }
                None
            }
        }
    }

    pub fn submit_master(&mut self) -> Result<Option<ProtectedAction>, VaultError> {
        if self.master_remove {
            self.state.verify_master(&self.master_input)?;
            self.state.rewrap(None)?;
            self.master_remove = false;
        } else if self.master_confirm_old {
            self.state.verify_master(&self.master_input)?;
            self.master_input.clear();
            self.master_confirm_old = false;
            return Ok(None);
        } else if self.master_change {
            self.state.open_without_master()?;
            self.state.rewrap(Some(&self.master_input))?;
            self.master_change = false;
        } else if self.state.exists() {
            self.state.verify_master(&self.master_input)?;
        } else {
            self.state.create(Some(&self.master_input))?;
        }
        self.master_input.clear();
        self.master_prompt = false;
        if self.master_as_gate && matches!(self.gate_pending, Some(ProtectedAction::Fill(_))) {
            self.state.mark_verified();
        }
        self.refresh_origin();
        Ok(self.gate_pending.take())
    }

    pub fn tick(&mut self, ctx: &egui::Context) -> bool {
        let now = Instant::now();
        let locked = self.state.auto_lock(now);
        if locked {
            self.clear_copied_password();
            self.revealed = None;
            self.menu_open = false;
            self.clear_form();
            self.master_input.clear();
            self.gate_pending = None;
            HAS_LOGINS.set(false);
        }
        if let Some(delay) = self.state.until_lock(now) {
            ctx.request_repaint_after(delay);
        }
        if let Some((_, _, until)) = &self.revealed {
            if now >= *until {
                self.revealed = None;
            } else {
                ctx.request_repaint_after(until.saturating_duration_since(now));
            }
        }
        if let Some((_, until)) = &self.copied {
            let until = *until;
            if now >= until {
                self.clear_copied_password();
            } else {
                ctx.request_repaint_after(until.saturating_duration_since(now));
            }
        }
        locked
    }

    pub fn show_settings(&mut self, ui: &mut egui::Ui, theme: crate::theme::Theme) -> Vec<Action> {
        use crate::settings_page::ui as sui;

        let mut actions = Vec::new();
        if let Some(message) = &self.message {
            sui::section(ui, theme, "NOTICE", |ui| {
                sui::row(ui, theme, message, sui::NONE, |_| {});
            });
        }
        let view = vault_view(self.state.exists(), self.state.locked());
        sui::section(ui, theme, "SAVED LOGINS", |ui| match view {
            VaultView::None => self.create_rows(ui, theme, &mut actions),
            VaultView::Locked => self.locked_rows(ui, theme, &mut actions),
            VaultView::Open => self.login_rows(ui, theme, &mut actions),
        });
        if view == VaultView::Open {
            let title = if self.edit.is_some() {
                "EDIT LOGIN"
            } else {
                "ADD LOGIN"
            };
            sui::section(ui, theme, title, |ui| {
                self.form_rows(ui, theme, &mut actions);
            });
        }
        sui::section(ui, theme, "SECURITY", |ui| {
            let mut first = true;
            if view != VaultView::None {
                sui::divider(ui, theme, &mut first);
                sui::row(
                    ui,
                    theme,
                    crate::protections::passwords_detail(
                        self.state.exists(),
                        self.state.os_protected(),
                    ),
                    sui::NONE,
                    |_| {},
                );
            }
            sui::divider(ui, theme, &mut first);
            sui::row(
                ui,
                theme,
                crate::protections::PASSWORDS_LIMIT,
                sui::NONE,
                |_| {},
            );
            if view != VaultView::Open {
                return;
            }
            sui::divider(ui, theme, &mut first);
            let mut require = self.state.has_master();
            let enabled = self.state.os_protected() || !self.state.has_master();
            let toggled = ui
                .add_enabled_ui(enabled, |ui| {
                    sui::row(
                        ui,
                        theme,
                        "Require a master password too",
                        sui::SWITCH,
                        |ui| sui::switch(ui, &mut require, "Require a master password too", theme),
                    )
                })
                .inner
                .clicked();
            if toggled {
                actions.push(Action::SetMaster);
            }
            if self.state.has_master() {
                sui::divider(ui, theme, &mut first);
                sui::row(ui, theme, "Change master password", sui::BUTTON, |ui| {
                    if sui::button(ui, "Change master password", theme, false).clicked() {
                        self.master_change = true;
                        self.master_confirm_old = true;
                        self.master_prompt = true;
                    }
                });
            }
            sui::divider(ui, theme, &mut first);
            sui::row(ui, theme, "Lock the vault now", sui::BUTTON, |ui| {
                if sui::button(ui, "Lock now", theme, false).clicked() {
                    actions.push(Action::Lock);
                }
            });
        });
        actions
    }

    fn locked_rows(
        &mut self,
        ui: &mut egui::Ui,
        theme: crate::theme::Theme,
        actions: &mut Vec<Action>,
    ) {
        use crate::settings_page::ui as sui;

        if self.state.has_master() {
            sui::row(ui, theme, "Password vault is locked", sui::BUTTON, |ui| {
                if sui::button(ui, "Unlock", theme, true).clicked() {
                    self.master_prompt = true;
                }
            });
        } else if self.state.exists() {
            let wide = sui::Size { w: 230.0, h: 28.0 };
            sui::row(ui, theme, "Password vault is locked", wide, |ui| {
                if sui::button(ui, "Unlock with your OS account", theme, true).clicked() {
                    actions.push(Action::Unlock);
                }
            });
        }
    }

    fn create_rows(
        &mut self,
        ui: &mut egui::Ui,
        theme: crate::theme::Theme,
        actions: &mut Vec<Action>,
    ) {
        use crate::settings_page::ui as sui;

        sui::row_hint(
            ui,
            theme,
            crate::protections::passwords_detail(false, false),
            Some("Create a vault to save passwords"),
            sui::BUTTON,
            |ui| {
                if sui::button(ui, "Create vault", theme, true).clicked() {
                    actions.push(Action::Create);
                }
            },
        );
    }

    fn login_rows(
        &mut self,
        ui: &mut egui::Ui,
        theme: crate::theme::Theme,
        actions: &mut Vec<Action>,
    ) {
        use crate::settings_page::ui as sui;

        let mut first = true;
        sui::divider(ui, theme, &mut first);
        sui::row(ui, theme, "Search", sui::INPUT, |ui| {
            sui::text_input(ui, &mut self.search, theme);
        });
        let mut entries = self.state.list();
        entries.sort_by(|a, b| a.origin.cmp(&b.origin).then(a.username.cmp(&b.username)));
        let buttons = sui::Size { w: 450.0, h: 28.0 };
        for entry in entries {
            if !entry.origin.contains(&self.search) && !entry.username.contains(&self.search) {
                continue;
            }
            let host = entry
                .origin
                .strip_prefix("https://")
                .unwrap_or(&entry.origin);
            sui::divider(ui, theme, &mut first);
            sui::row_hint(ui, theme, host, Some(&entry.username), buttons, |ui| {
                // right to left, so Delete is added first
                if sui::button(ui, "Delete", theme, false).clicked() {
                    self.delete = Some(entry.id);
                }
                if sui::button(ui, "Edit", theme, false).clicked() {
                    actions.push(Action::Gate(ProtectedAction::Edit(entry.id)));
                }
                if sui::button(ui, "Copy username", theme, false).clicked() {
                    self.clear_copied_password();
                    ui.ctx().copy_text(entry.username.clone());
                }
                if sui::button(ui, "Copy password", theme, false).clicked() {
                    actions.push(Action::Gate(ProtectedAction::Copy(entry.id)));
                }
                if sui::button(ui, "Reveal", theme, false).clicked() {
                    actions.push(Action::Gate(ProtectedAction::Reveal(entry.id)));
                }
            });
            if let Some((id, value, _)) = &self.revealed
                && *id == entry.id
            {
                sui::hairline(ui, theme);
                sui::row(ui, theme, value.as_str(), sui::NONE, |_| {});
            }
        }
        if let Some(id) = self.delete {
            sui::divider(ui, theme, &mut first);
            let pair = sui::Size { w: 182.0, h: 28.0 };
            sui::row(ui, theme, "Delete this saved login?", pair, |ui| {
                if sui::button(ui, "Delete login", theme, true).clicked() {
                    actions.push(Action::Delete(id));
                    self.delete = None;
                }
                if sui::button(ui, "Cancel", theme, false).clicked() {
                    self.delete = None;
                }
            });
        }
    }

    fn form_rows(
        &mut self,
        ui: &mut egui::Ui,
        theme: crate::theme::Theme,
        actions: &mut Vec<Action>,
    ) {
        use crate::settings_page::ui as sui;

        let editing = self.edit.is_some();
        sui::row(ui, theme, "Origin", sui::INPUT, |ui| {
            ui.add_enabled_ui(!editing, |ui| {
                sui::text_input(ui, &mut self.origin, theme);
            });
        });
        sui::hairline(ui, theme);
        sui::row(ui, theme, "Username", sui::INPUT, |ui| {
            ui.add_enabled_ui(!editing, |ui| {
                sui::text_input(ui, &mut self.username, theme);
            });
        });
        sui::hairline(ui, theme);
        sui::row(ui, theme, "Password", sui::INPUT, |ui| {
            sui::password_input(ui, &mut self.password, !self.show_password, theme);
        });
        sui::hairline(ui, theme);
        sui::row(ui, theme, "Show password", sui::SWITCH, |ui| {
            sui::switch(ui, &mut self.show_password, "Show password", theme);
        });
        sui::hairline(ui, theme);
        let pair = sui::Size { w: 203.0, h: 28.0 };
        let label = if editing {
            "Editing a saved login"
        } else {
            "New login"
        };
        sui::row(ui, theme, label, pair, |ui| {
            if sui::button(ui, "Save login", theme, true).clicked() {
                actions.push(Action::Save);
            }
            if editing && sui::button(ui, "Cancel edit", theme, false).clicked() {
                actions.push(Action::Cancel);
            }
        });
    }

    pub fn show_master_prompt(
        &mut self,
        ctx: &egui::Context,
        settings: &aegis_core::Settings,
    ) -> bool {
        if !self.master_prompt {
            return false;
        }
        let mut submit = false;
        let title = if self.master_confirm_old || self.master_remove {
            "Current master password"
        } else if self.master_change || !self.state.exists() {
            "New master password"
        } else {
            "Master password"
        };
        let builder = egui::ViewportBuilder::default()
            .with_icon(crate::logo::icon())
            .with_inner_size([360.0, 145.0])
            .with_transparent(cfg!(any(target_os = "windows", target_os = "macos")))
            .with_has_shadow(false)
            .with_decorations(false)
            .with_resizable(false)
            .with_taskbar(false)
            .with_always_on_top()
            .with_active(true);
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("master_password_prompt"),
            builder,
            |ctx, _| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let theme =
                        crate::theme::resolve(settings, ui.ctx().theme() == egui::Theme::Dark);
                    ui.label(
                        egui::RichText::new(title)
                            .size(15.0)
                            .strong()
                            .color(theme.palette.text),
                    );
                    crate::settings_page::ui::password_input(
                        ui,
                        &mut self.master_input,
                        !self.master_show,
                        theme,
                    );
                    crate::settings_page::ui::switch(
                        ui,
                        &mut self.master_show,
                        "Show password",
                        theme,
                    );
                    if crate::settings_page::ui::button(ui, "Continue", theme, true).clicked() {
                        submit = true;
                    }
                    if crate::settings_page::ui::button(ui, "Cancel", theme, false).clicked() {
                        self.master_input.clear();
                        self.master_prompt = false;
                        self.master_change = false;
                        self.master_confirm_old = false;
                        self.master_remove = false;
                        self.master_as_gate = false;
                        self.gate_pending = None;
                    }
                });
            },
        );
        submit
    }

    pub fn clear_form(&mut self) {
        self.origin.clear();
        self.username.clear();
        self.password.clear();
        self.edit = None;
    }

    pub fn copy_password(&mut self, password: &str) -> Result<(), arboard::Error> {
        Clipboard::new()?.set_text(password)?;
        self.copied = Some((
            Zeroizing::new(password.to_owned()),
            Instant::now() + Duration::from_secs(30),
        ));
        Ok(())
    }

    pub fn clear_copied_password(&mut self) {
        let Some((copied, _)) = self.copied.take() else {
            return;
        };
        let Ok(mut clipboard) = Clipboard::new() else {
            return;
        };
        let Ok(current) = clipboard.get_text() else {
            return;
        };
        let current = Zeroizing::new(current);
        if *current == *copied {
            let _ = clipboard.clear();
        }
    }
}
