//! The installer's embedded file archive. Shared by `build.rs` (packing) and the
//! installer (unpacking), so it depends on nothing but std.
//!
//! Layout: `MAGIC` (8 bytes), file count (u32 LE), then per file: name length (u16 LE),
//! UTF-8 relative path with `/` separators, data length (u64 LE), data.

#![allow(dead_code)]

use std::path::Path;

pub const MAGIC: &[u8; 8] = b"SDPK1\0\0\0";

pub struct Entry<'a> {
    pub path: String,
    pub data: &'a [u8],
}

/// Packs every file under `dir` (recursively, sorted for reproducible output).
pub fn pack_dir(dir: &Path) -> std::io::Result<Vec<u8>> {
    let mut files = Vec::new();
    collect(dir, dir, &mut files)?;
    files.sort();
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(files.len() as u32).to_le_bytes());
    for rel in files {
        let data = std::fs::read(dir.join(&rel))?;
        let name = rel.replace('\\', "/");
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&(data.len() as u64).to_le_bytes());
        out.extend_from_slice(&data);
    }
    Ok(out)
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let p = e.path();
        if p.is_dir() {
            collect(root, &p, out)?;
        } else {
            out.push(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
        }
    }
    Ok(())
}

/// Parses an archive with full bounds checking; rejects absolute or `..` paths.
pub fn parse(bytes: &[u8]) -> Result<Vec<Entry<'_>>, String> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let mut pos = 0usize;
    let mut take = |n: usize| -> Result<&[u8], String> {
        let end = pos.checked_add(n).filter(|&e| e <= bytes.len()).ok_or("archive truncated")?;
        let s = &bytes[pos..end];
        pos = end;
        Ok(s)
    };
    if take(8)? != MAGIC {
        return Err("not a Soundesti payload".into());
    }
    let count = u32::from_le_bytes(take(4)?.try_into().unwrap()) as usize;
    let mut entries = Vec::with_capacity(count.min(10_000));
    for _ in 0..count {
        let nlen = u16::from_le_bytes(take(2)?.try_into().unwrap()) as usize;
        let name = std::str::from_utf8(take(nlen)?).map_err(|_| "bad file name")?.to_string();
        if name.is_empty() || name.starts_with('/') || name.contains(':') || name.split('/').any(|c| c == "..") {
            return Err(format!("unsafe path in archive: {name}"));
        }
        let dlen = u64::from_le_bytes(take(8)?.try_into().unwrap()) as usize;
        let data = take(dlen)?;
        entries.push(Entry { path: name, data });
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_safety() {
        let dir = std::env::temp_dir().join(format!("sdpk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.txt"), b"hello").unwrap();
        std::fs::write(dir.join("sub").join("b.bin"), [0u8, 1, 2]).unwrap();
        let bytes = pack_dir(&dir).unwrap();
        let e = parse(&bytes).unwrap();
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].path, "a.txt");
        assert_eq!(e[0].data, b"hello");
        assert_eq!(e[1].path, "sub/b.bin");
        assert!(parse(&bytes[..bytes.len() - 1]).is_err(), "truncation detected");
        assert!(parse(b"garbage!").is_err());
        assert!(parse(&[]).unwrap().is_empty());
        // Path traversal is refused.
        let mut evil = MAGIC.to_vec();
        evil.extend_from_slice(&1u32.to_le_bytes());
        let name = b"../x";
        evil.extend_from_slice(&(name.len() as u16).to_le_bytes());
        evil.extend_from_slice(name);
        evil.extend_from_slice(&0u64.to_le_bytes());
        assert!(parse(&evil).is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}
