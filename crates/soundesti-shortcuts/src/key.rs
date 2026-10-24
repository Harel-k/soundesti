use std::fmt;
use std::str::FromStr;

/// A physical-ish key. Numpad keys are distinct from the main row, as in FL.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    /// Uppercase A-Z.
    Letter(char),
    /// Main-row digit 0-9.
    Digit(u8),
    /// F1-F24.
    F(u8),
    NumpadDigit(u8),
    NumpadAdd,
    NumpadSubtract,
    NumpadMultiply,
    NumpadDivide,
    Space,
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    Comma,
    Period,
    Slash,
    Semicolon,
    OpenBracket,
    CloseBracket,
    Equals,
    Minus,
}

/// A key plus modifiers, e.g. `Ctrl+Shift+S`. Left/right modifier sides are not
/// distinguished yet (see KNOWN_ISSUES KI-002).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Chord {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: Key,
}

impl Chord {
    pub const fn new(key: Key) -> Self {
        Self { ctrl: false, alt: false, shift: false, key }
    }

    pub const fn with(key: Key, ctrl: bool, alt: bool, shift: bool) -> Self {
        Self { ctrl, alt, shift, key }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("cannot parse key `{0}`")]
pub struct ParseKeyError(pub String);

impl FromStr for Key {
    type Err = ParseKeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseKeyError(s.to_string());
        let lower = s.to_ascii_lowercase();
        let key = match lower.as_str() {
            "space" => Key::Space,
            "enter" | "return" => Key::Enter,
            "esc" | "escape" => Key::Escape,
            "tab" => Key::Tab,
            "backspace" => Key::Backspace,
            "del" | "delete" => Key::Delete,
            "ins" | "insert" => Key::Insert,
            "home" => Key::Home,
            "end" => Key::End,
            "pgup" | "pageup" => Key::PageUp,
            "pgdown" | "pagedown" | "pgdn" => Key::PageDown,
            "up" => Key::Up,
            "down" => Key::Down,
            "left" => Key::Left,
            "right" => Key::Right,
            "," | "comma" => Key::Comma,
            "." | "period" => Key::Period,
            "/" | "slash" => Key::Slash,
            ";" | "semicolon" => Key::Semicolon,
            "[" => Key::OpenBracket,
            "]" => Key::CloseBracket,
            "=" | "equals" => Key::Equals,
            "-" | "minus" => Key::Minus,
            "num+" => Key::NumpadAdd,
            "num-" => Key::NumpadSubtract,
            "num*" => Key::NumpadMultiply,
            "num/" => Key::NumpadDivide,
            _ => {
                if let Some(d) = lower.strip_prefix("num") {
                    let n: u8 = d.parse().map_err(|_| err())?;
                    if n > 9 {
                        return Err(err());
                    }
                    Key::NumpadDigit(n)
                } else if let Some(f) = lower.strip_prefix('f').filter(|r| !r.is_empty()) {
                    let n: u8 = f.parse().map_err(|_| err())?;
                    if !(1..=24).contains(&n) {
                        return Err(err());
                    }
                    Key::F(n)
                } else if lower.len() == 1 {
                    let c = lower.chars().next().unwrap();
                    if c.is_ascii_lowercase() {
                        Key::Letter(c.to_ascii_uppercase())
                    } else if let Some(d) = c.to_digit(10) {
                        Key::Digit(d as u8)
                    } else {
                        return Err(err());
                    }
                } else {
                    return Err(err());
                }
            }
        };
        Ok(key)
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Letter(c) => write!(f, "{c}"),
            Key::Digit(d) => write!(f, "{d}"),
            Key::F(n) => write!(f, "F{n}"),
            Key::NumpadDigit(d) => write!(f, "Num{d}"),
            Key::NumpadAdd => f.write_str("Num+"),
            Key::NumpadSubtract => f.write_str("Num-"),
            Key::NumpadMultiply => f.write_str("Num*"),
            Key::NumpadDivide => f.write_str("Num/"),
            Key::Space => f.write_str("Space"),
            Key::Enter => f.write_str("Enter"),
            Key::Escape => f.write_str("Esc"),
            Key::Tab => f.write_str("Tab"),
            Key::Backspace => f.write_str("Backspace"),
            Key::Delete => f.write_str("Del"),
            Key::Insert => f.write_str("Ins"),
            Key::Home => f.write_str("Home"),
            Key::End => f.write_str("End"),
            Key::PageUp => f.write_str("PgUp"),
            Key::PageDown => f.write_str("PgDown"),
            Key::Up => f.write_str("Up"),
            Key::Down => f.write_str("Down"),
            Key::Left => f.write_str("Left"),
            Key::Right => f.write_str("Right"),
            Key::Comma => f.write_str(","),
            Key::Period => f.write_str("."),
            Key::Slash => f.write_str("/"),
            Key::Semicolon => f.write_str(";"),
            Key::OpenBracket => f.write_str("["),
            Key::CloseBracket => f.write_str("]"),
            Key::Equals => f.write_str("="),
            Key::Minus => f.write_str("-"),
        }
    }
}

impl FromStr for Chord {
    type Err = ParseKeyError;

    /// Parses `Ctrl+Alt+Shift+Key` in any modifier order (case-insensitive).
    /// `Num+` is handled specially because its name contains the separator.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let (mods, key_part) = if s.len() >= 4 && s[s.len() - 4..].eq_ignore_ascii_case("num+") {
            (s[..s.len() - 4].trim_end_matches('+'), &s[s.len() - 4..])
        } else {
            match s.rfind('+') {
                Some(i) if i + 1 < s.len() => (&s[..i], &s[i + 1..]),
                _ => ("", s),
            }
        };
        let mut chord = Chord::new(key_part.trim().parse()?);
        for m in mods.split('+').map(str::trim).filter(|m| !m.is_empty()) {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" => chord.ctrl = true,
                "alt" | "opt" | "option" => chord.alt = true,
                "shift" => chord.shift = true,
                _ => return Err(ParseKeyError(s.to_string())),
            }
        }
        Ok(chord)
    }
}

impl fmt::Display for Chord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.alt {
            f.write_str("Alt+")?;
        }
        if self.shift {
            f.write_str("Shift+")?;
        }
        write!(f, "{}", self.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_normalize() {
        let c: Chord = "shift+ctrl+s".parse().unwrap();
        assert_eq!(c, Chord::with(Key::Letter('S'), true, false, true));
        assert_eq!(c.to_string(), "Ctrl+Shift+S");
        assert_eq!("Alt+Num+".parse::<Chord>().unwrap(), Chord::with(Key::NumpadAdd, false, true, false));
        assert_eq!("Num+".parse::<Chord>().unwrap(), Chord::new(Key::NumpadAdd));
        assert_eq!("Alt+Num*".parse::<Chord>().unwrap().key, Key::NumpadMultiply);
        assert_eq!("F12".parse::<Chord>().unwrap().key, Key::F(12));
        assert_eq!("Num0".parse::<Chord>().unwrap().key, Key::NumpadDigit(0));
        assert_eq!("0".parse::<Chord>().unwrap().key, Key::Digit(0));
        assert_eq!("Ctrl+Opt+Q".parse::<Chord>().unwrap(), "Ctrl+Alt+Q".parse::<Chord>().unwrap());
    }

    #[test]
    fn rejects_garbage() {
        for bad in ["", "Ctrl+", "Hyper+A", "F0", "F99", "Num10", "AB", "Ctrl+Banana"] {
            assert!(bad.parse::<Chord>().is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn display_round_trips() {
        for s in ["Space", "Ctrl+Alt+Shift+C", "Alt+F8", "Shift+0", "Num/", "Ctrl+Del", "PgDown"] {
            let c: Chord = s.parse().unwrap();
            assert_eq!(c.to_string().parse::<Chord>().unwrap(), c, "{s}");
        }
    }
}
