//! Low-level .flp reader: the `FLhd` header and the `FLdt` event stream.
//!
//! Format facts (as documented by the open-source PyFLP project; this is an independent
//! implementation): every event is a one-byte ID followed by a payload whose size depends
//! on the ID range: < 64 one byte, < 128 two bytes, < 192 four bytes (little-endian), and
//! >= 192 a LEB128 length followed by that many bytes.

use crate::FlpError;

pub const WORD: u8 = 64;
pub const DWORD: u8 = 128;
pub const TEXT: u8 = 192;
pub const DATA: u8 = 208;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value<'a> {
    Int(u32),
    Bytes(&'a [u8]),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Event<'a> {
    pub id: u8,
    pub value: Value<'a>,
}

impl<'a> Event<'a> {
    pub fn int(&self) -> u32 {
        match self.value {
            Value::Int(v) => v,
            Value::Bytes(b) => b.iter().take(4).enumerate().fold(0, |acc, (i, x)| acc | (*x as u32) << (8 * i)),
        }
    }

    pub fn bytes(&self) -> &'a [u8] {
        match self.value {
            Value::Bytes(b) => b,
            Value::Int(_) => &[],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Header {
    pub ppq: u16,
}

const MAX_EVENTS: usize = 5_000_000;

pub fn parse(data: &[u8]) -> Result<(Header, Vec<Event<'_>>), FlpError> {
    let mut r = Reader { data, pos: 0 };
    if r.take(4)? != b"FLhd" {
        return Err(FlpError::NotFlp);
    }
    let header_len = r.u32()? as usize;
    if header_len < 6 {
        return Err(FlpError::Corrupt("header is too short".into()));
    }
    let h = r.take(header_len)?;
    // Bytes 0-3 are the file format and channel count; only the PPQ matters here.
    let header = Header { ppq: u16::from_le_bytes([h[4], h[5]]) };
    if header.ppq == 0 {
        return Err(FlpError::Corrupt("PPQ is zero".into()));
    }
    if r.take(4)? != b"FLdt" {
        return Err(FlpError::Corrupt("missing FLdt chunk".into()));
    }
    let len = r.u32()? as usize;
    // Some writers leave a wrong size; read what is actually there.
    let end = (r.pos + len).min(data.len());
    let mut events = Vec::new();
    while r.pos < end {
        if events.len() >= MAX_EVENTS {
            return Err(FlpError::Corrupt("too many events".into()));
        }
        let id = r.u8()?;
        let value = if id < WORD {
            Value::Int(r.u8()? as u32)
        } else if id < DWORD {
            Value::Int(u16::from_le_bytes(r.take(2)?.try_into().unwrap()) as u32)
        } else if id < TEXT {
            Value::Int(r.u32()?)
        } else {
            let n = r.varint()?;
            Value::Bytes(r.take(n)?)
        };
        events.push(Event { id, value });
    }
    Ok((header, events))
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], FlpError> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.data.len()).ok_or(FlpError::Truncated)?;
        let s = &self.data[self.pos..end];
        self.pos = end;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, FlpError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, FlpError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn varint(&mut self) -> Result<usize, FlpError> {
        let mut v: usize = 0;
        for shift in (0..35).step_by(7) {
            let b = self.u8()?;
            v |= ((b & 0x7f) as usize) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(FlpError::Corrupt("event size is too large".into()))
    }
}

/// Decodes a text event: UTF-16LE for FL 11.5 and newer, otherwise single-byte text.
pub fn text(bytes: &[u8], utf16: bool) -> String {
    let s = if utf16 && bytes.len() >= 2 {
        let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        String::from_utf16_lossy(&units)
    } else {
        bytes.iter().map(|&b| b as char).collect()
    };
    s.trim_end_matches('\0').to_string()
}

#[cfg(test)]
pub mod writer {
    //! Builds .flp files for tests.
    use super::*;

    pub struct Writer {
        pub ppq: u16,
        pub events: Vec<u8>,
    }

    impl Writer {
        pub fn new(ppq: u16) -> Self {
            Self { ppq, events: Vec::new() }
        }

        pub fn int(&mut self, id: u8, v: u32) -> &mut Self {
            self.events.push(id);
            if id < WORD {
                self.events.push(v as u8);
            } else if id < DWORD {
                self.events.extend_from_slice(&(v as u16).to_le_bytes());
            } else {
                self.events.extend_from_slice(&v.to_le_bytes());
            }
            self
        }

        pub fn bytes(&mut self, id: u8, b: &[u8]) -> &mut Self {
            self.events.push(id);
            let mut n = b.len();
            loop {
                let byte = (n & 0x7f) as u8;
                n >>= 7;
                if n == 0 {
                    self.events.push(byte);
                    break;
                }
                self.events.push(byte | 0x80);
            }
            self.events.extend_from_slice(b);
            self
        }

        pub fn ascii(&mut self, id: u8, s: &str) -> &mut Self {
            let mut b = s.as_bytes().to_vec();
            b.push(0);
            self.bytes(id, &b)
        }

        pub fn utf16(&mut self, id: u8, s: &str) -> &mut Self {
            let mut b: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
            b.extend_from_slice(&[0, 0]);
            self.bytes(id, &b)
        }

        pub fn finish(&self) -> Vec<u8> {
            let mut out = b"FLhd".to_vec();
            out.extend_from_slice(&6u32.to_le_bytes());
            out.extend_from_slice(&0i16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(&self.ppq.to_le_bytes());
            out.extend_from_slice(b"FLdt");
            out.extend_from_slice(&(self.events.len() as u32).to_le_bytes());
            out.extend_from_slice(&self.events);
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::writer::Writer;
    use super::*;

    #[test]
    fn reads_every_size_class() {
        let mut w = Writer::new(96);
        w.int(9, 1).int(66, 140).int(156, 128_000).bytes(224, &[1; 300]);
        let data = w.finish();
        let (h, ev) = parse(&data).unwrap();
        assert_eq!(h.ppq, 96);
        assert_eq!(ev.len(), 4);
        assert_eq!(ev[1].int(), 140);
        assert_eq!(ev[2].int(), 128_000);
        assert_eq!(ev[3].bytes().len(), 300, "two-byte LEB128 length");
    }

    #[test]
    fn rejects_bad_files() {
        assert!(matches!(parse(b"RIFF...."), Err(FlpError::NotFlp)));
        let mut data = Writer::new(96).ascii(199, "21.0.0").finish();
        data.truncate(data.len() - 3);
        // The FLdt size claims more than is there; the last event is cut short.
        assert!(matches!(parse(&data), Err(FlpError::Truncated)));
    }

    #[test]
    fn decodes_text() {
        let u: Vec<u8> = "Kick\0".encode_utf16().flat_map(|x| x.to_le_bytes()).collect();
        assert_eq!(text(&u, true), "Kick");
        assert_eq!(text(b"Snare\0", false), "Snare");
    }
}
