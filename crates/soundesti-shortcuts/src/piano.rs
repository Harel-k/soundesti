use crate::Key;

/// Semitone offset (from the typing keyboard's base C) for a key, using the common
/// two-row "tracker" layout: Z-row = lower octave, Q-row = upper octave, black keys on
/// the row above. FL parity of this exact layout is UNVERIFIED (SHORTCUT_MATRIX SM-5).
pub fn typing_piano_offset(key: Key) -> Option<i32> {
    let off = match key {
        // Lower octave
        Key::Letter('Z') => 0,
        Key::Letter('S') => 1,
        Key::Letter('X') => 2,
        Key::Letter('D') => 3,
        Key::Letter('C') => 4,
        Key::Letter('V') => 5,
        Key::Letter('G') => 6,
        Key::Letter('B') => 7,
        Key::Letter('H') => 8,
        Key::Letter('N') => 9,
        Key::Letter('J') => 10,
        Key::Letter('M') => 11,
        Key::Comma => 12,
        Key::Letter('L') => 13,
        Key::Period => 14,
        Key::Semicolon => 15,
        Key::Slash => 16,
        // Upper octave
        Key::Letter('Q') => 12,
        Key::Digit(2) => 13,
        Key::Letter('W') => 14,
        Key::Digit(3) => 15,
        Key::Letter('E') => 16,
        Key::Letter('R') => 17,
        Key::Digit(5) => 18,
        Key::Letter('T') => 19,
        Key::Digit(6) => 20,
        Key::Letter('Y') => 21,
        Key::Digit(7) => 22,
        Key::Letter('U') => 23,
        Key::Letter('I') => 24,
        Key::Digit(9) => 25,
        Key::Letter('O') => 26,
        Key::Digit(0) => 27,
        Key::Letter('P') => 28,
        Key::OpenBracket => 29,
        Key::Equals => 30,
        Key::CloseBracket => 31,
        _ => return None,
    };
    Some(off)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_is_chromatic_without_gaps_in_each_row() {
        let lower = ['Z', 'S', 'X', 'D', 'C', 'V', 'G', 'B', 'H', 'N', 'J', 'M'];
        for (i, c) in lower.iter().enumerate() {
            assert_eq!(typing_piano_offset(Key::Letter(*c)), Some(i as i32));
        }
        assert_eq!(typing_piano_offset(Key::Letter('Q')), Some(12));
        assert_eq!(typing_piano_offset(Key::Letter('U')), Some(23));
        assert_eq!(typing_piano_offset(Key::Space), None);
        assert_eq!(typing_piano_offset(Key::F(1)), None);
    }
}
