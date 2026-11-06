use eframe::egui::Key;

pub const KEY_LEFTCTRL: u16 = 29;
pub const KEY_LEFTSHIFT: u16 = 42;
pub const KEY_LEFTALT: u16 = 56;
pub const KEY_LEFTMETA: u16 = 125;

pub fn evdev(k: Key) -> Option<u16> {
    use Key::*;
    Some(match k {
        Escape => 1,
        Num1 => 2,
        Num2 => 3,
        Num3 => 4,
        Num4 => 5,
        Num5 => 6,
        Num6 => 7,
        Num7 => 8,
        Num8 => 9,
        Num9 => 10,
        Num0 => 11,
        Minus => 12,
        Equals | Plus => 13,
        Backspace => 14,
        Tab => 15,
        Q => 16,
        W => 17,
        E => 18,
        R => 19,
        T => 20,
        Y => 21,
        U => 22,
        I => 23,
        O => 24,
        P => 25,
        OpenBracket => 26,
        CloseBracket => 27,
        Enter => 28,
        A => 30,
        S => 31,
        D => 32,
        F => 33,
        G => 34,
        H => 35,
        J => 36,
        K => 37,
        L => 38,
        Semicolon | Colon => 39,
        Quote => 40,
        Backtick => 41,
        Backslash | Pipe => 43,
        Z => 44,
        X => 45,
        C => 46,
        V => 47,
        B => 48,
        N => 49,
        M => 50,
        Comma => 51,
        Period => 52,
        Slash | Questionmark => 53,
        Space => 57,
        F1 => 59,
        F2 => 60,
        F3 => 61,
        F4 => 62,
        F5 => 63,
        F6 => 64,
        F7 => 65,
        F8 => 66,
        F9 => 67,
        F10 => 68,
        F11 => 87,
        F12 => 88,
        Home => 102,
        ArrowUp => 103,
        PageUp => 104,
        ArrowLeft => 105,
        ArrowRight => 106,
        End => 107,
        ArrowDown => 108,
        PageDown => 109,
        Insert => 110,
        Delete => 111,
        AudioVolumeMute => 113,
        AudioVolumeDown => 114,
        MediaPlayPause => 164,
        MediaTrackNext => 163,
        MediaTrackPrevious => 165,
        _ => return None,
    })
}

pub fn modifier_changes(old: [bool; 4], new: [bool; 4]) -> Vec<(u16, u16, i32)> {
    [KEY_LEFTCTRL, KEY_LEFTSHIFT, KEY_LEFTALT, KEY_LEFTMETA]
        .into_iter()
        .zip(old.into_iter().zip(new))
        .filter(|(_, (o, n))| o != n)
        .map(|(code, (_, n))| (crate::platform::ev::EV_KEY, code, n as i32))
        .collect()
}

pub fn track_down(down: &mut Vec<u16>, events: &[(u16, u16, i32)]) {
    for &(kind, code, value) in events {
        if kind != crate::platform::ev::EV_KEY {
            continue;
        }
        if value != 0 {
            if !down.contains(&code) {
                down.push(code);
            }
        } else {
            down.retain(|c| *c != code);
        }
    }
}

pub fn release_down(down: &mut Vec<u16>) -> Vec<(u16, u16, i32)> {
    down.drain(..)
        .map(|code| (crate::platform::ev::EV_KEY, code, 0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harfler_ve_rakamlar_evdev_koduna_doner() {
        assert_eq!(evdev(Key::A), Some(30));
        assert_eq!(evdev(Key::Z), Some(44));
        assert_eq!(evdev(Key::Num0), Some(11));
        assert_eq!(evdev(Key::Enter), Some(28));
        assert_eq!(evdev(Key::F12), Some(88));
    }

    #[test]
    fn yalnizca_degisen_degistiriciler_gonderilir() {
        let evs = modifier_changes([false, true, false, false], [true, false, false, false]);
        assert_eq!(evs, vec![(1, KEY_LEFTCTRL, 1), (1, KEY_LEFTSHIFT, 0)]);
        assert!(modifier_changes([true; 4], [true; 4]).is_empty());
    }

    #[test]
    fn basili_kalan_tus_ve_dugmeler_odak_kaybinda_birakilir() {
        use crate::platform::ev;
        let mut down = Vec::new();
        track_down(
            &mut down,
            &[
                (ev::EV_KEY, ev::BTN_LEFT, 1),
                (ev::EV_KEY, 30, 1),
                (ev::EV_KEY, 30, 1),
                (ev::EV_REL, ev::REL_WHEEL, 1),
                (ev::EV_KEY, 44, 1),
                (ev::EV_KEY, 44, 0),
            ],
        );
        assert_eq!(down, vec![ev::BTN_LEFT, 30]);
        assert_eq!(
            release_down(&mut down),
            vec![(ev::EV_KEY, ev::BTN_LEFT, 0), (ev::EV_KEY, 30, 0)]
        );
        assert!(down.is_empty());
    }
}
