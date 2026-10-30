use super::Shortcut;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Key {
    L,
    T,
    W,
    R,
    S,
    Tab,
    F5,
    Left,
    Right,
    LeftBracket,
    RightBracket,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub command: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Platform {
    MacOS,
    Linux,
}

pub(super) fn match_shortcut(key: Key, mods: Mods, platform: Platform) -> Option<Shortcut> {
    use Key::*;
    use Platform::*;
    use Shortcut::*;

    match (platform, key, mods.ctrl, mods.shift, mods.alt, mods.command) {
        (MacOS, L, false, false, false, true) | (Linux, L, true, false, false, false) => {
            Some(CommandBar)
        }
        (MacOS, T, false, false, false, true) | (Linux, T, true, false, false, false) => {
            Some(NewTab)
        }
        (MacOS, W, false, false, false, true) | (Linux, W, true, false, false, false) => {
            Some(CloseTab)
        }
        (MacOS, S, false, true, false, true) | (Linux, S, true, true, false, false) => {
            Some(ToggleCompact)
        }
        (_, Tab, true, false, false, false) => Some(NextTab),
        (_, Tab, true, true, false, false) => Some(PrevTab),
        (MacOS, R, false, false, false, true)
        | (Linux, R, true, false, false, false)
        | (Linux, F5, false, false, false, false) => Some(Reload),
        (MacOS, LeftBracket, false, false, false, true)
        | (Linux, Left, false, false, true, false) => Some(Back),
        (MacOS, RightBracket, false, false, false, true)
        | (Linux, Right, false, false, true, false) => Some(Forward),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_shortcuts() {
        let command = Mods {
            command: true,
            ..Mods::default()
        };
        let control = Mods {
            ctrl: true,
            ..Mods::default()
        };
        let control_shift = Mods {
            ctrl: true,
            shift: true,
            ..Mods::default()
        };
        let command_shift = Mods {
            command: true,
            shift: true,
            ..Mods::default()
        };
        for (key, mods, expected) in [
            (Key::L, command, Shortcut::CommandBar),
            (Key::T, command, Shortcut::NewTab),
            (Key::W, command, Shortcut::CloseTab),
            (Key::S, command_shift, Shortcut::ToggleCompact),
            (Key::Tab, control, Shortcut::NextTab),
            (Key::Tab, control_shift, Shortcut::PrevTab),
            (Key::R, command, Shortcut::Reload),
            (Key::LeftBracket, command, Shortcut::Back),
            (Key::RightBracket, command, Shortcut::Forward),
        ] {
            assert_eq!(match_shortcut(key, mods, Platform::MacOS), Some(expected));
        }
        assert_eq!(match_shortcut(Key::S, command, Platform::MacOS), None);
    }

    #[test]
    fn linux_shortcuts() {
        let control = Mods {
            ctrl: true,
            ..Mods::default()
        };
        let control_shift = Mods {
            ctrl: true,
            shift: true,
            ..Mods::default()
        };
        let alt = Mods {
            alt: true,
            ..Mods::default()
        };
        for (key, mods, expected) in [
            (Key::L, control, Shortcut::CommandBar),
            (Key::T, control, Shortcut::NewTab),
            (Key::W, control, Shortcut::CloseTab),
            (Key::S, control_shift, Shortcut::ToggleCompact),
            (Key::Tab, control, Shortcut::NextTab),
            (Key::Tab, control_shift, Shortcut::PrevTab),
            (Key::R, control, Shortcut::Reload),
            (Key::F5, Mods::default(), Shortcut::Reload),
            (Key::Left, alt, Shortcut::Back),
            (Key::Right, alt, Shortcut::Forward),
        ] {
            assert_eq!(match_shortcut(key, mods, Platform::Linux), Some(expected));
        }
        assert_eq!(match_shortcut(Key::S, control, Platform::Linux), None);
    }

    #[test]
    fn unrelated_keys_pass_through() {
        let command_shift = Mods {
            command: true,
            shift: true,
            ..Mods::default()
        };
        let control_alt = Mods {
            ctrl: true,
            alt: true,
            ..Mods::default()
        };
        assert_eq!(match_shortcut(Key::T, command_shift, Platform::MacOS), None);
        assert_eq!(match_shortcut(Key::L, control_alt, Platform::Linux), None);
        assert_eq!(
            match_shortcut(Key::L, Mods::default(), Platform::Linux),
            None
        );
        assert_eq!(
            match_shortcut(Key::L, Mods::default(), Platform::MacOS),
            None
        );
        // caps lock alone leaves no shortcut modifiers after native lock bits are masked
        assert_eq!(
            match_shortcut(Key::L, Mods::default(), Platform::MacOS),
            None
        );
    }
}
