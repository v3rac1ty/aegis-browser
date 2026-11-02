use std::cell::Cell;

pub const HTTPS_ONLY_LABEL: &str = "HTTPS-only";
pub const PREPARING_LINE: &str = "Preparing protection…";
pub const MISSING_LISTS_NOTICE: &str = "Tracker lists not found, blocking is off";
pub const TRACKER_SETTING_HINT: &str = "Open tabs reload to apply this.";
pub const HTTPS_FAILURE_TITLE: &str = "This site doesn't offer a secure connection";
pub const PASSWORDS_LIMIT: &str = "Software running as you can't be fully stopped by any browser.";

pub fn passwords_detail(exists: bool, os_protected: bool) -> &'static str {
    if !exists {
        return "No password vault yet";
    }
    if !os_protected {
        return "Passwords protected by your master password";
    }
    if cfg!(target_os = "windows") {
        "Passwords are encrypted with your Windows account"
    } else if cfg!(target_os = "macos") {
        "Passwords are encrypted with your macOS Keychain"
    } else {
        "Passwords are encrypted with your Linux keyring"
    }
}

pub fn passwords_title(exists: bool, os_protected: bool) -> &'static str {
    if !exists {
        "Password vault"
    } else if os_protected {
        "Passwords encrypted with your OS account"
    } else {
        "Passwords protected by your master password"
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ProtectionKind {
    TrackerBlocking,
    HttpsOnly,
    ThirdPartyCookies,
    IdleTabs,
    NoTelemetry,
    Passwords,
}

pub struct Protection {
    pub kind: ProtectionKind,
    pub title: &'static str,
    pub detail: &'static str,
    pub active: bool,
}

#[derive(Clone, Copy)]
pub struct ProtectionState {
    pub lists_loaded: bool,
    pub block_trackers: bool,
    pub https_only: bool,
    pub vault_exists: bool,
    pub vault_os_protected: bool,
}

thread_local! {
    static CURRENT: Cell<ProtectionState> = const { Cell::new(ProtectionState {
        lists_loaded: false,
        block_trackers: true,
        https_only: true,
        vault_exists: false,
        vault_os_protected: false,
    }) };
}

pub fn set_state(state: ProtectionState) {
    CURRENT.set(state);
}

pub fn state() -> ProtectionState {
    CURRENT.get()
}

pub fn protections(state: ProtectionState) -> Vec<Protection> {
    let cookie_detail = if cfg!(target_os = "windows") {
        "Known trackers' cookies blocked (Strict tracking prevention)"
    } else if cfg!(target_os = "macos") {
        "Third-party cookies blocked (WebKit default)"
    } else {
        "Third-party cookies rejected"
    };
    let https_detail = if cfg!(target_os = "macos") {
        "HTTP upgrades are on; the fallback page is unavailable on macOS"
    } else {
        "HTTP pages upgrade to HTTPS with a fallback choice"
    };
    vec![
        Protection {
            kind: ProtectionKind::TrackerBlocking,
            title: "Tracker blocking",
            detail: if !state.block_trackers {
                "Turned off in settings"
            } else if state.lists_loaded {
                "Network rules active; cosmetic filtering is not yet available"
            } else {
                "Tracker blocking is off"
            },
            active: state.lists_loaded && state.block_trackers,
        },
        Protection {
            kind: ProtectionKind::HttpsOnly,
            title: HTTPS_ONLY_LABEL,
            detail: if state.https_only {
                https_detail
            } else {
                "Turned off in settings"
            },
            active: state.https_only,
        },
        Protection {
            kind: ProtectionKind::ThirdPartyCookies,
            title: "Third-party cookies",
            detail: cookie_detail,
            active: true,
        },
        Protection {
            kind: ProtectionKind::IdleTabs,
            title: "Idle tabs sleep",
            detail: "Idle tabs remain active",
            active: false,
        },
        Protection {
            kind: ProtectionKind::NoTelemetry,
            title: "No telemetry",
            detail: "Aegis does not send usage telemetry",
            active: true,
        },
        Protection {
            kind: ProtectionKind::Passwords,
            title: passwords_title(state.vault_exists, state.vault_os_protected),
            detail: passwords_detail(state.vault_exists, state.vault_os_protected),
            active: state.vault_exists && state.vault_os_protected,
        },
    ]
}

pub fn tracker_blocking_active() -> bool {
    let state = state();
    state.lists_loaded && state.block_trackers
}

pub fn welcome_chip() -> &'static str {
    if protections(state())
        .iter()
        .any(|entry| entry.kind != ProtectionKind::NoTelemetry && entry.active)
    {
        "Secure"
    } else {
        "Private"
    }
}

pub fn welcome_line() -> &'static str {
    if !state().block_trackers {
        return if state().https_only {
            "Secure connection upgrades are ready. Tracker blocking is off."
        } else {
            "No telemetry is sent. Tracker blocking and HTTPS-only are off."
        };
    }
    match (tracker_blocking_active(), state().https_only) {
        (true, true) => "Network tracker blocking and secure connection upgrades are ready.",
        (true, false) => "Network tracker blocking is ready. Secure connection upgrades are off.",
        (false, true) => "Secure connection upgrades are ready. Tracker blocking is off.",
        (false, false) => "No telemetry is sent. Tracker blocking and HTTPS-only are off.",
    }
}

pub fn show(ui: &mut eframe::egui::Ui, theme: crate::theme::Theme, host: &str, blocked: u32) {
    let state = state();
    if tracker_blocking_active() && cfg!(target_os = "windows") {
        ui.label(format!("{blocked} trackers blocked on {host}"));
    } else if tracker_blocking_active() {
        ui.label(format!(
            "Tracker blocking active on {host}; count unavailable"
        ));
    } else {
        ui.label("Tracker blocking is off");
    }
    for protection in protections(state) {
        let color = if protection.active {
            theme.palette.text
        } else {
            theme.palette.tertiary
        };
        ui.horizontal(|ui| {
            let (status_rect, _) = ui
                .allocate_exact_size(eframe::egui::vec2(16.0, 16.0), eframe::egui::Sense::hover());
            if protection.active {
                crate::icons::check(ui.painter(), status_rect, color);
            } else {
                crate::icons::dash(ui.painter(), status_rect, color);
            }
            if protection.kind == ProtectionKind::HttpsOnly {
                let (rect, _) = ui.allocate_exact_size(
                    eframe::egui::vec2(16.0, 16.0),
                    eframe::egui::Sense::hover(),
                );
                crate::icons::lock(ui.painter(), rect, color);
            }
            ui.label(eframe::egui::RichText::new(protection.title).color(color));
        });
        ui.label(
            eframe::egui::RichText::new(protection.detail)
                .size(11.0)
                .color(color),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protections_follow_loaded_lists_and_https_setting() {
        let off = protections(ProtectionState {
            lists_loaded: false,
            block_trackers: true,
            https_only: false,
            vault_exists: false,
            vault_os_protected: false,
        });
        assert!(!off[0].active);
        assert!(!off[1].active);
        let on = protections(ProtectionState {
            lists_loaded: true,
            block_trackers: true,
            https_only: true,
            vault_exists: false,
            vault_os_protected: false,
        });
        assert!(on[0].active);
        assert!(on[1].active);
        assert!(on[2].active);
        assert!(!on[3].active);
        assert!(on[4].active);
        let disabled = protections(ProtectionState {
            lists_loaded: true,
            block_trackers: false,
            https_only: true,
            vault_exists: false,
            vault_os_protected: false,
        });
        assert!(!disabled[0].active);
    }
}
