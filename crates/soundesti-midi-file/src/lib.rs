//! Standard MIDI File import and export (formats 0 and 1, independent implementation).
//!
//! Import produces one part per (track, MIDI channel) with its notes in Soundesti ticks,
//! plus tempo and time signature. Export writes a format-1 file: a tempo track and one
//! track per Channel Rack channel, from the current pattern or the whole song.

use std::collections::HashMap;

use soundesti_project::{ChannelId, Project};
use soundesti_timeline::{PPQ, Tick, TimeSignature};

#[derive(Debug, PartialEq)]
pub enum MidiError {
    NotMidi,
    Truncated,
    Unsupported(String),
}

impl std::fmt::Display for MidiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MidiError::NotMidi => write!(f, "not a MIDI file"),
            MidiError::Truncated => write!(f, "the MIDI file is cut short"),
            MidiError::Unsupported(m) => write!(f, "unsupported MIDI file: {m}"),
        }
    }
}

impl std::error::Error for MidiError {}

#[derive(Clone, Debug, PartialEq)]
pub struct MidiNote {
    pub start: Tick,
    pub length: Tick,
    pub key: u8,
    /// 0..1
    pub velocity: f32,
}

/// Notes from one track on one MIDI channel.
#[derive(Clone, Debug, PartialEq)]
pub struct MidiPart {
    pub name: String,
    /// 0-based MIDI channel (9 = General MIDI drums).
    pub channel: u8,
    pub program: Option<u8>,
    pub notes: Vec<MidiNote>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MidiSong {
    pub bpm: Option<f64>,
    pub time_signature: Option<TimeSignature>,
    pub parts: Vec<MidiPart>,
}

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], MidiError> {
        let end = self.p.checked_add(n).filter(|&e| e <= self.d.len()).ok_or(MidiError::Truncated)?;
        let s = &self.d[self.p..end];
        self.p = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, MidiError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, MidiError> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, MidiError> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn var(&mut self) -> Result<u32, MidiError> {
        let mut v = 0u32;
        for _ in 0..4 {
            let b = self.u8()?;
            v = (v << 7) | (b & 0x7F) as u32;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(MidiError::Unsupported("variable-length number too long".into()))
    }
}

/// Parses a .mid file.
pub fn read(data: &[u8]) -> Result<MidiSong, MidiError> {
    let mut r = Reader { d: data, p: 0 };
    if r.take(4).map_err(|_| MidiError::NotMidi)? != b"MThd" {
        return Err(MidiError::NotMidi);
    }
    let hlen = r.u32()? as usize;
    let format = r.u16()?;
    let ntracks = r.u16()?;
    let division = r.u16()?;
    r.take(hlen.saturating_sub(6))?;
    if format > 1 {
        return Err(MidiError::Unsupported(format!("format {format} (only 0 and 1)")));
    }
    if division & 0x8000 != 0 {
        return Err(MidiError::Unsupported("SMPTE time division".into()));
    }
    let ppq = division.max(1) as i64;
    let conv = |t: u64| -> Tick { (t as i64).saturating_mul(PPQ) / ppq };

    let mut song = MidiSong { bpm: None, time_signature: None, parts: Vec::new() };
    for _ in 0..ntracks {
        if r.p + 8 > data.len() {
            break;
        }
        let tag = r.take(4)?;
        let len = r.u32()? as usize;
        if tag != b"MTrk" {
            r.take(len)?;
            continue;
        }
        let body = r.take(len.min(data.len() - r.p))?;
        let mut t = Reader { d: body, p: 0 };
        let mut now: u64 = 0;
        let mut status = 0u8;
        let mut name = String::new();
        // (channel) -> part index; (channel, key) -> open note (start, velocity)
        let mut parts: HashMap<u8, MidiPart> = HashMap::new();
        let mut open: HashMap<(u8, u8), Vec<(u64, u8)>> = HashMap::new();
        let mut programs: HashMap<u8, u8> = HashMap::new();
        while t.p < body.len() {
            now += t.var()? as u64;
            let mut b = t.u8()?;
            if b < 0x80 {
                // Running status.
                if status == 0 {
                    return Err(MidiError::Unsupported("data byte without status".into()));
                }
                t.p -= 1;
                b = status;
            }
            match b {
                0xFF => {
                    let kind = t.u8()?;
                    let n = t.var()? as usize;
                    let m = t.take(n)?;
                    match kind {
                        0x03 if name.is_empty() => name = String::from_utf8_lossy(m).trim().to_string(),
                        0x51 if m.len() == 3 && song.bpm.is_none() => {
                            let us = (m[0] as u32) << 16 | (m[1] as u32) << 8 | m[2] as u32;
                            if us > 0 {
                                song.bpm = Some((60_000_000.0 / us as f64 * 1000.0).round() / 1000.0);
                            }
                        }
                        0x58 if m.len() >= 2 && song.time_signature.is_none() => {
                            let den = 1u8.checked_shl(m[1] as u32).unwrap_or(4);
                            if m[0] > 0 && [2, 4, 8, 16].contains(&den) {
                                song.time_signature = Some(TimeSignature::new(m[0].min(16), den));
                            }
                        }
                        0x2F => break,
                        _ => {}
                    }
                }
                0xF0 | 0xF7 => {
                    let n = t.var()? as usize;
                    t.take(n)?;
                }
                _ => {
                    status = b;
                    let ch = b & 0x0F;
                    match b & 0xF0 {
                        0x90 | 0x80 => {
                            let key = t.u8()? & 0x7F;
                            let vel = t.u8()? & 0x7F;
                            if b & 0xF0 == 0x90 && vel > 0 {
                                open.entry((ch, key)).or_default().push((now, vel));
                            } else if let Some(list) = open.get_mut(&(ch, key))
                                && !list.is_empty()
                            {
                                let (start, v) = list.remove(0);
                                parts
                                    .entry(ch)
                                    .or_insert_with(|| MidiPart {
                                        name: String::new(),
                                        channel: ch,
                                        program: None,
                                        notes: Vec::new(),
                                    })
                                    .notes
                                    .push(MidiNote {
                                        start: conv(start),
                                        length: (conv(now) - conv(start)).max(1),
                                        key,
                                        velocity: v as f32 / 127.0,
                                    });
                            }
                        }
                        0xA0 | 0xB0 | 0xE0 => {
                            t.take(2)?;
                        }
                        0xC0 => {
                            programs.insert(ch, t.u8()? & 0x7F);
                        }
                        0xD0 => {
                            t.take(1)?;
                        }
                        _ => return Err(MidiError::Unsupported(format!("status byte {b:#04X}"))),
                    }
                }
            }
        }
        // Notes still held at the end of the track end there.
        for ((ch, key), list) in open {
            for (start, v) in list {
                parts
                    .entry(ch)
                    .or_insert_with(|| MidiPart { name: String::new(), channel: ch, program: None, notes: Vec::new() })
                    .notes
                    .push(MidiNote {
                        start: conv(start),
                        length: (conv(now) - conv(start)).max(1),
                        key,
                        velocity: v as f32 / 127.0,
                    });
            }
        }
        let many = parts.len() > 1;
        let mut chans: Vec<u8> = parts.keys().copied().collect();
        chans.sort_unstable();
        for ch in chans {
            let mut part = parts.remove(&ch).unwrap();
            part.notes.sort_by_key(|n| (n.start, n.key));
            part.program = programs.get(&ch).copied();
            part.name = match (name.is_empty(), many) {
                (false, false) => name.clone(),
                (false, true) => format!("{name} ch{}", ch + 1),
                (true, _) if ch == 9 => "Drums".into(),
                (true, _) => format!("MIDI ch{}", ch + 1),
            };
            song.parts.push(part);
        }
    }
    Ok(song)
}

fn var(out: &mut Vec<u8>, mut v: u32) {
    let mut bytes = vec![(v & 0x7F) as u8];
    v >>= 7;
    while v > 0 {
        bytes.push((v & 0x7F) as u8 | 0x80);
        v >>= 7;
    }
    out.extend(bytes.iter().rev());
}

fn chunk(out: &mut Vec<u8>, tag: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(tag);
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
}

/// Writes the current pattern (or the whole song in song mode) as a format-1 MIDI file
/// at Soundesti's resolution (960 PPQ). Muted channels are left out.
pub fn write(project: &Project, song: bool) -> Vec<u8> {
    let mut p = project.clone();
    p.song_mode = song;
    let ids: Vec<ChannelId> = p.channels.iter().filter(|c| !c.muted).map(|c| c.id).collect();
    let slots: HashMap<ChannelId, usize> = ids.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    let snap = soundesti_session::build_snapshot(&p, &slots);

    let mut out = Vec::new();
    let mut hdr = Vec::new();
    hdr.extend_from_slice(&1u16.to_be_bytes());
    hdr.extend_from_slice(&((ids.len() + 1) as u16).to_be_bytes());
    hdr.extend_from_slice(&(PPQ as u16).to_be_bytes());
    chunk(&mut out, b"MThd", &hdr);

    // Tempo track.
    let mut t = Vec::new();
    let us = (60_000_000.0 / p.bpm.max(1.0)).round() as u32;
    t.extend_from_slice(&[0x00, 0xFF, 0x51, 0x03, (us >> 16) as u8, (us >> 8) as u8, us as u8]);
    let ts = p.time_signature;
    t.extend_from_slice(&[0x00, 0xFF, 0x58, 0x04, ts.numerator, ts.denominator.trailing_zeros() as u8, 24, 8]);
    if !p.meta.title.is_empty() {
        t.extend_from_slice(&[0x00, 0xFF, 0x03]);
        var(&mut t, p.meta.title.len() as u32);
        t.extend_from_slice(p.meta.title.as_bytes());
    }
    t.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
    chunk(&mut out, b"MTrk", &t);

    for (i, id) in ids.iter().enumerate() {
        let ch = p.channel(*id).unwrap();
        // GM drums for drum-like instruments; otherwise one channel per track (skip 10).
        let midi_ch = match ch.instrument.type_id.as_str() {
            "soundesti.kickr" | "soundesti.clapper" | "soundesti.hatz" => 9,
            _ => [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15][i % 15],
        };
        let mut events: Vec<(Tick, u8, u8, u8)> = Vec::new(); // (tick, order, key, velocity)
        for n in snap.notes.iter().filter(|n| n.channel as usize == i) {
            let vel = ((n.velocity * 127.0).round() as u8).clamp(1, 127);
            events.push((n.start, 1, n.key.min(127), vel));
            events.push((n.end, 0, n.key.min(127), 0));
        }
        events.sort();
        let mut t = Vec::new();
        t.push(0x00);
        t.extend_from_slice(&[0xFF, 0x03]);
        var(&mut t, ch.name.len() as u32);
        t.extend_from_slice(ch.name.as_bytes());
        let mut last: Tick = 0;
        for (tick, on, key, vel) in events {
            var(&mut t, (tick - last).max(0) as u32);
            last = tick;
            t.extend_from_slice(&[if on == 1 { 0x90 } else { 0x80 } | midi_ch, key, if on == 1 { vel } else { 64 }]);
        }
        t.extend_from_slice(&[0x00, 0xFF, 0x2F, 0x00]);
        chunk(&mut out, b"MTrk", &t);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use soundesti_project::Note;

    #[test]
    fn export_then_import_round_trips() {
        let mut p = Project::new();
        p.bpm = 128.0;
        p.meta.title = "Test".into();
        let (kick, hat) = (p.channels[0].id, p.channels[2].id);
        p.current_mut().notes_mut(kick).push(Note::new(0, 240, 60));
        p.current_mut().notes_mut(kick).push(Note::new(1920, 240, 60));
        let mut n = Note::new(480, 480, 66);
        n.velocity = 0.5;
        p.current_mut().notes_mut(hat).push(n);
        let bytes = write(&p, false);
        assert_eq!(&bytes[..4], b"MThd");
        let song = read(&bytes).unwrap();
        assert_eq!(song.bpm, Some(128.0));
        assert_eq!(song.time_signature, Some(TimeSignature::new(4, 4)));
        let k = song.parts.iter().find(|x| x.name == "Kick").unwrap();
        assert_eq!(k.channel, 9, "drums on GM channel 10");
        assert_eq!(
            k.notes.iter().map(|n| (n.start, n.length, n.key)).collect::<Vec<_>>(),
            [(0, 240, 60), (1920, 240, 60)]
        );
        let h = song.parts.iter().find(|x| x.name == "Hat").unwrap();
        assert_eq!((h.notes[0].start, h.notes[0].length, h.notes[0].key), (480, 480, 66));
        assert!((h.notes[0].velocity - 64.0 / 127.0).abs() < 0.01);
    }

    #[test]
    fn reads_running_status_other_resolutions_and_note_on_zero() {
        // Format 0, 96 PPQ, one track: tempo 100 BPM, program 5, note C4 with running
        // status and a velocity-0 note-off, then a note left hanging at the end.
        let mut trk = vec![0x00, 0xFF, 0x51, 0x03, 0x09, 0x27, 0xC0, 0x00, 0xC0, 0x05];
        trk.extend_from_slice(&[0x00, 0x90, 60, 100, 0x60, 60, 0]); // on, +96 off (running status, vel 0)
        trk.extend_from_slice(&[0x00, 64, 90, 0x30, 0xFF, 0x2F, 0x00]); // E4 on (running), end after 48
        let mut d = b"MThd".to_vec();
        d.extend_from_slice(&[0, 0, 0, 6, 0, 0, 0, 1, 0, 96]);
        d.extend_from_slice(b"MTrk");
        d.extend_from_slice(&(trk.len() as u32).to_be_bytes());
        d.extend_from_slice(&trk);
        let song = read(&d).unwrap();
        assert_eq!(song.bpm, Some(100.0));
        let part = &song.parts[0];
        assert_eq!(part.program, Some(5));
        assert_eq!((part.notes[0].start, part.notes[0].length, part.notes[0].key), (0, 960, 60));
        assert_eq!((part.notes[1].start, part.notes[1].length, part.notes[1].key), (960, 480, 64));
    }

    #[test]
    fn rejects_bad_data_without_panicking() {
        assert_eq!(read(b"RIFF1234"), Err(MidiError::NotMidi));
        let good = write(&Project::new(), false);
        for cut in 0..good.len() {
            let _ = read(&good[..cut]);
        }
        let mut smpte = good.clone();
        smpte[12] = 0xE7;
        assert!(matches!(read(&smpte), Err(MidiError::Unsupported(_))));
    }
}
