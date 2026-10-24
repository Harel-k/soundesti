//! egui key events → Soundesti chords.

use soundesti_shortcuts::{Chord, Key};

pub fn chord_from_egui(key: egui::Key, mods: egui::Modifiers) -> Option<Chord> {
    let k = map_key(key)?;
    Some(Chord::with(k, mods.ctrl || mods.command, mods.alt, mods.shift))
}

fn map_key(key: egui::Key) -> Option<Key> {
    use egui::Key as E;
    let name = key.name();
    // Letters and F-keys map by name; everything else explicitly.
    if name.len() == 1 && name.as_bytes()[0].is_ascii_uppercase() {
        return Some(Key::Letter(name.as_bytes()[0] as char));
    }
    let k = match key {
        E::Num0 => Key::Digit(0),
        E::Num1 => Key::Digit(1),
        E::Num2 => Key::Digit(2),
        E::Num3 => Key::Digit(3),
        E::Num4 => Key::Digit(4),
        E::Num5 => Key::Digit(5),
        E::Num6 => Key::Digit(6),
        E::Num7 => Key::Digit(7),
        E::Num8 => Key::Digit(8),
        E::Num9 => Key::Digit(9),
        E::Space => Key::Space,
        E::Enter => Key::Enter,
        E::Escape => Key::Escape,
        E::Tab => Key::Tab,
        E::Backspace => Key::Backspace,
        E::Delete => Key::Delete,
        E::Insert => Key::Insert,
        E::Home => Key::Home,
        E::End => Key::End,
        E::PageUp => Key::PageUp,
        E::PageDown => Key::PageDown,
        E::ArrowUp => Key::Up,
        E::ArrowDown => Key::Down,
        E::ArrowLeft => Key::Left,
        E::ArrowRight => Key::Right,
        E::Comma => Key::Comma,
        E::Period => Key::Period,
        E::Slash => Key::Slash,
        E::Semicolon => Key::Semicolon,
        E::OpenBracket => Key::OpenBracket,
        E::CloseBracket => Key::CloseBracket,
        E::Equals => Key::Equals,
        E::Minus => Key::Minus,
        _ => {
            if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()) {
                return (1..=24).contains(&n).then_some(Key::F(n));
            }
            return None;
        }
    };
    Some(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_keys() {
        let m = egui::Modifiers { ctrl: true, shift: true, ..Default::default() };
        assert_eq!(chord_from_egui(egui::Key::S, m), Some("Ctrl+Shift+S".parse().unwrap()));
        assert_eq!(chord_from_egui(egui::Key::F12, egui::Modifiers::NONE), Some("F12".parse().unwrap()));
        assert_eq!(chord_from_egui(egui::Key::Num0, egui::Modifiers::NONE), Some("0".parse().unwrap()));
        assert_eq!(chord_from_egui(egui::Key::Space, egui::Modifiers::NONE), Some("Space".parse().unwrap()));
        assert!(chord_from_egui(egui::Key::ShiftLeft, egui::Modifiers::NONE).is_none());
    }
}
