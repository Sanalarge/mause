use crate::config::Config;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pref {
    ControlIn = 0,
    AudioOut = 1,
    MicOut = 2,
    FilesIn = 3,
    ClipIn = 4,
    ControlOut = 5,
    RelayIn = 6,
    MicIn = 7,
    ShareText = 8,
}

pub const SUPPORTS_PREFS: u32 = 1 << 15;
pub const SUPPORTS_SCREEN: u32 = 1 << 14;
pub const SUPPORTS_SCREEN2: u32 = 1 << 13;
pub const SUPPORTS_USB: u32 = 1 << 12;
pub const SUPPORTS_VERSION: u32 = 1 << 11;
pub const SUPPORTS_HARDWARE: u32 = 1 << 10;
pub const SCREEN_OUT: u16 = 1 << 9;

impl Pref {
    pub const ALL: [Pref; 9] = [
        Pref::ControlIn,
        Pref::AudioOut,
        Pref::MicOut,
        Pref::FilesIn,
        Pref::ClipIn,
        Pref::ControlOut,
        Pref::RelayIn,
        Pref::MicIn,
        Pref::ShareText,
    ];

    pub fn from_u8(v: u8) -> Option<Pref> {
        Pref::ALL.into_iter().find(|p| *p as u8 == v)
    }

    pub fn bit(self) -> u16 {
        1 << self as u16
    }

    pub fn tr(self) -> &'static str {
        match self {
            Pref::ControlIn => "karşının bu PC'yi yönetmesi",
            Pref::AudioOut => "sesini paylaşma",
            Pref::MicOut => "mikrofonunu paylaşma",
            Pref::FilesIn => "dosya erişimi",
            Pref::ClipIn => "pano alma",
            Pref::ControlOut => "karşıyı yönetme",
            Pref::RelayIn => "karşının sesini çalma",
            Pref::MicIn => "karşının mikrofonunu kullanma",
            Pref::ShareText => "pano paylaşımı",
        }
    }
}

pub fn get(cfg: &Config, p: Pref) -> bool {
    match p {
        Pref::ControlIn => cfg.allow_control_in,
        Pref::AudioOut => cfg.allow_audio_out,
        Pref::MicOut => cfg.allow_mic_out,
        Pref::FilesIn => cfg.allow_files_in,
        Pref::ClipIn => cfg.allow_clip_in,
        Pref::ControlOut => cfg.allow_control_out,
        Pref::RelayIn => cfg.relay_in,
        Pref::MicIn => cfg.mic_in,
        Pref::ShareText => cfg.share_text,
    }
}

pub fn set(cfg: &mut Config, p: Pref, on: bool) {
    let field = match p {
        Pref::ControlIn => &mut cfg.allow_control_in,
        Pref::AudioOut => &mut cfg.allow_audio_out,
        Pref::MicOut => &mut cfg.allow_mic_out,
        Pref::FilesIn => &mut cfg.allow_files_in,
        Pref::ClipIn => &mut cfg.allow_clip_in,
        Pref::ControlOut => &mut cfg.allow_control_out,
        Pref::RelayIn => &mut cfg.relay_in,
        Pref::MicIn => &mut cfg.mic_in,
        Pref::ShareText => &mut cfg.share_text,
    };
    *field = on;
}

pub fn bits(cfg: &Config) -> u16 {
    Pref::ALL
        .into_iter()
        .filter(|p| get(cfg, *p))
        .fold(0, |b, p| b | p.bit())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Control,
    Audio,
    Mic,
    Files,
    Clip,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    Out,
    In,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Where {
    Local,
    Remote,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Flow {
    pub dir: Dir,
    pub kind: Kind,
}

impl Flow {
    pub const KINDS: [Kind; 5] = [
        Kind::Control,
        Kind::Audio,
        Kind::Mic,
        Kind::Files,
        Kind::Clip,
    ];

    #[cfg(test)]
    pub fn all() -> impl Iterator<Item = Flow> {
        [Dir::Out, Dir::In]
            .into_iter()
            .flat_map(|dir| Self::KINDS.into_iter().map(move |kind| Flow { dir, kind }))
    }

    pub fn short(self) -> &'static str {
        match self.kind {
            Kind::Control => "Klavye",
            Kind::Audio => "Ses",
            Kind::Mic => "Mikrofon",
            Kind::Files => "Dosya",
            Kind::Clip => "Pano",
        }
    }

    pub fn sentence(self) -> &'static str {
        match (self.dir, self.kind) {
            (Dir::Out, Kind::Control) => "Bu PC'nin klavye ve mouse'u karşıyı yönetsin",
            (Dir::Out, Kind::Audio) => "Bu PC'nin sesi karşıda çalsın",
            (Dir::Out, Kind::Mic) => "Bu PC'nin mikrofonu karşıda kullanılsın",
            (Dir::Out, Kind::Files) => "Bu PC karşının klasörlerine erişip dosya gönderebilsin",
            (Dir::Out, Kind::Clip) => "Burada kopyalanan metin karşıya geçsin",
            (Dir::In, Kind::Control) => "Karşının klavye ve mouse'u bu PC'yi yönetsin",
            (Dir::In, Kind::Audio) => "Karşının sesi burada çalsın",
            (Dir::In, Kind::Mic) => "Karşının mikrofonu burada kullanılsın",
            (Dir::In, Kind::Files) => "Karşı PC bu PC'nin klasörlerine erişip dosya gönderebilsin",
            (Dir::In, Kind::Clip) => "Karşıda kopyalanan metin buraya geçsin",
        }
    }

    fn needs(self) -> (&'static [Pref], &'static [Pref]) {
        match self.kind {
            Kind::Control => (&[Pref::ControlOut], &[Pref::ControlIn]),
            Kind::Audio => (&[Pref::AudioOut], &[Pref::RelayIn]),
            Kind::Mic => (&[Pref::MicOut], &[Pref::MicIn]),
            Kind::Files => (&[], &[Pref::FilesIn]),
            Kind::Clip => (&[Pref::ShareText], &[Pref::ShareText, Pref::ClipIn]),
        }
    }

    fn off_switch(self) -> (Where, Pref) {
        match (self.dir, self.kind) {
            (Dir::Out, Kind::Control) => (Where::Local, Pref::ControlOut),
            (Dir::Out, Kind::Audio) => (Where::Local, Pref::AudioOut),
            (Dir::Out, Kind::Mic) => (Where::Local, Pref::MicOut),
            (Dir::Out, Kind::Files) => (Where::Remote, Pref::FilesIn),
            (Dir::Out, Kind::Clip) => (Where::Remote, Pref::ClipIn),
            (Dir::In, Kind::Control) => (Where::Local, Pref::ControlIn),
            (Dir::In, Kind::Audio) => (Where::Local, Pref::RelayIn),
            (Dir::In, Kind::Mic) => (Where::Local, Pref::MicIn),
            (Dir::In, Kind::Files) => (Where::Local, Pref::FilesIn),
            (Dir::In, Kind::Clip) => (Where::Local, Pref::ClipIn),
        }
    }

    fn side(self, giver: bool) -> Where {
        match (self.dir, giver) {
            (Dir::Out, true) | (Dir::In, false) => Where::Local,
            _ => Where::Remote,
        }
    }

    fn side_ok(self, at: Where, bits: Option<u16>) -> bool {
        let (giver, taker) = self.needs();
        [(giver, true), (taker, false)]
            .into_iter()
            .filter(|(_, g)| self.side(*g) == at)
            .all(|(prefs, _)| bits.is_none_or(|b| prefs.iter().all(|p| b & p.bit() != 0)))
    }

    pub fn active(self, mine: u16, peer: Option<u16>) -> bool {
        self.side_ok(Where::Local, Some(mine)) && self.side_ok(Where::Remote, peer)
    }

    pub fn look(self, mine: u16, peer: Option<u16>) -> Look {
        if self.active(mine, peer) {
            Look::On
        } else if peer.is_some() && self.side_ok(Where::Local, Some(mine)) {
            Look::PeerOff
        } else {
            Look::Off
        }
    }

    pub fn wanted(self, mine: u16, peer: Option<u16>) -> bool {
        match self.look(mine, peer) {
            Look::On => false,
            Look::Off => true,
            Look::PeerOff => self.off_switch().0 == Where::Remote,
        }
    }

    pub fn sync_changes(self, on: bool) -> Vec<(Where, Pref, bool)> {
        let (giver, taker) = self.needs();
        let mut out = Vec::new();
        for (prefs, is_giver) in [(giver, true), (taker, false)] {
            for pref in prefs {
                out.push((self.side(is_giver), *pref, on));
            }
        }
        out
    }

    pub fn needs_peer(self, turning_on: bool) -> bool {
        self.changes(turning_on)
            .iter()
            .any(|(w, _, _)| *w == Where::Remote)
    }

    pub fn changes(self, on: bool) -> Vec<(Where, Pref, bool)> {
        if on {
            let (giver, taker) = self.needs();
            let mut out: Vec<(Where, Pref, bool)> = Vec::new();
            for (prefs, giver) in [(giver, true), (taker, false)] {
                for p in prefs {
                    out.push((self.side(giver), *p, true));
                }
            }
            out
        } else {
            let (at, pref) = self.off_switch();
            vec![(at, pref, false)]
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    On,
    PeerOff,
    Off,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_on() -> u16 {
        Pref::ALL.into_iter().fold(0, |b, p| b | p.bit())
    }

    #[test]
    fn a_flow_is_the_mirror_of_the_same_flow_seen_from_the_other_pc() {
        let a = all_on() & !Pref::RelayIn.bit();
        let b = all_on() & !Pref::AudioOut.bit();
        for kind in Flow::KINDS {
            let out_a = Flow {
                dir: Dir::Out,
                kind,
            }
            .active(a, Some(b));
            let in_b = Flow { dir: Dir::In, kind }.active(b, Some(a));
            assert_eq!(out_a, in_b, "{kind:?}");
        }
    }

    #[test]
    fn turning_a_flow_on_sets_every_needed_flag_on_both_sides() {
        for flow in Flow::all() {
            let (mut mine, mut peer) = (0u16, 0u16);
            for (at, p, on) in flow.changes(true) {
                assert!(on);
                match at {
                    Where::Local => mine |= p.bit(),
                    Where::Remote => peer |= p.bit(),
                }
            }
            assert!(flow.active(mine, Some(peer)), "{flow:?}");
        }
    }

    #[test]
    fn synced_flow_changes_set_both_sides_together() {
        for flow in Flow::all() {
            for on in [true, false] {
                let start = if on { 0 } else { all_on() };
                let (mut mine, mut peer) = (start, start);
                for (at, pref, value) in flow.sync_changes(on) {
                    assert_eq!(value, on);
                    let target = if at == Where::Local {
                        &mut mine
                    } else {
                        &mut peer
                    };
                    if value {
                        *target |= pref.bit();
                    } else {
                        *target &= !pref.bit();
                    }
                }
                assert_eq!(flow.active(mine, Some(peer)), on, "{flow:?} {on}");
            }
        }
    }

    #[test]
    fn turning_a_flow_off_stops_it() {
        for flow in Flow::all() {
            let (mut mine, mut peer) = (all_on(), all_on());
            for (at, p, _) in flow.changes(false) {
                match at {
                    Where::Local => mine &= !p.bit(),
                    Where::Remote => peer &= !p.bit(),
                }
            }
            assert!(!flow.active(mine, Some(peer)), "{flow:?}");
        }
    }

    fn apply(changes: Vec<(Where, Pref, bool)>, mine: &mut u16, peer: &mut u16) {
        for (at, p, on) in changes {
            match (at, on) {
                (Where::Local, true) => *mine |= p.bit(),
                (Where::Local, false) => *mine &= !p.bit(),
                (Where::Remote, true) => {}
                (Where::Remote, false) => *peer &= !p.bit(),
            }
        }
    }

    #[test]
    fn a_flow_turned_off_here_can_be_turned_on_here_again() {
        for flow in Flow::all() {
            let (mut mine, mut peer) = (all_on(), all_on());
            apply(flow.changes(false), &mut mine, &mut peer);
            assert!(!flow.active(mine, Some(peer)), "{flow:?}");
            apply(flow.changes(true), &mut mine, &mut peer);
            let remote_only = flow.dir == Dir::Out && matches!(flow.kind, Kind::Files | Kind::Clip);
            assert_eq!(flow.active(mine, Some(peer)), !remote_only, "{flow:?}");
        }
    }

    #[test]
    fn closing_one_flow_leaves_the_others_running() {
        for flow in Flow::all() {
            let (mut mine, mut peer) = (all_on(), all_on());
            apply(flow.changes(false), &mut mine, &mut peer);
            for other in Flow::all().filter(|f| *f != flow) {
                assert!(other.active(mine, Some(peer)), "{flow:?} → {other:?}");
            }
        }
    }

    #[test]
    fn a_flow_closed_on_the_other_pc_is_shown_as_such_and_a_click_turns_it_off_here() {
        let audio_in = Flow {
            dir: Dir::In,
            kind: Kind::Audio,
        };
        let peer = all_on() & !Pref::AudioOut.bit();
        assert_eq!(audio_in.look(all_on(), Some(peer)), Look::PeerOff);
        assert!(!audio_in.wanted(all_on(), Some(peer)));
        let mine = all_on() & !Pref::RelayIn.bit();
        assert_eq!(audio_in.look(mine, Some(peer)), Look::Off);
        assert!(audio_in.wanted(mine, Some(peer)));
        assert_eq!(audio_in.look(all_on(), None), Look::On);
        assert_eq!(audio_in.look(mine, None), Look::Off);
        let files_out = Flow {
            dir: Dir::Out,
            kind: Kind::Files,
        };
        let peer = all_on() & !Pref::FilesIn.bit();
        assert_eq!(files_out.look(all_on(), Some(peer)), Look::PeerOff);
        assert!(files_out.wanted(all_on(), Some(peer)));
    }
}
