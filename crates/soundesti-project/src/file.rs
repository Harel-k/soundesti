use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::Project;

pub const FORMAT_VERSION: u32 = 1;
pub const FILE_EXTENSION: &str = "soundesti";

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("could not read {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("could not write {path}: {source}")]
    Write { path: PathBuf, source: std::io::Error },
    #[error("{path} is not a valid Soundesti project: {source}")]
    Parse { path: PathBuf, source: serde_json::Error },
    #[error("{path} was saved by a newer Soundesti (format {found}, this build reads up to {supported})")]
    TooNew { path: PathBuf, found: u32, supported: u32 },
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Saves atomically: writes `<file>.tmp`, flushes it to disk, keeps the previous file as
/// `<file>.bak`, then renames the temp file over the target. If any step fails the
/// previous project file is left untouched.
pub fn save(project: &Project, path: &Path) -> Result<(), ProjectError> {
    let json = serde_json::to_vec_pretty(project).expect("project serialization cannot fail");
    let tmp = sibling(path, ".tmp");
    let write_err = |source| ProjectError::Write { path: path.to_path_buf(), source };
    {
        let mut f = fs::File::create(&tmp).map_err(write_err)?;
        f.write_all(&json).map_err(write_err)?;
        f.sync_all().map_err(write_err)?;
    }
    if path.exists() {
        // Best effort: a failed backup must not block saving.
        let _ = fs::copy(path, sibling(path, ".bak"));
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        write_err(e)
    })
}

pub fn load(path: &Path) -> Result<Project, ProjectError> {
    let bytes = fs::read(path).map_err(|source| ProjectError::Read { path: path.to_path_buf(), source })?;
    let parse = |source| ProjectError::Parse { path: path.to_path_buf(), source };
    let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(parse)?;
    let found = value.get("format_version").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    if found > FORMAT_VERSION {
        return Err(ProjectError::TooNew { path: path.to_path_buf(), found, supported: FORMAT_VERSION });
    }
    let value = migrate(value, found);
    let mut project: Project = serde_json::from_value(value).map_err(parse)?;
    project.format_version = FORMAT_VERSION;
    project.validate();
    Ok(project)
}

/// Upgrades older documents step by step. Format 1 is the first public format, so
/// there is nothing to migrate yet; future versions add arms here.
fn migrate(value: serde_json::Value, _from: u32) -> serde_json::Value {
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Note, Project};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("soundesti-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn save_load_round_trip() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("song.soundesti");
        let mut p = Project::new();
        p.bpm = 87.5;
        p.meta.title = "Round trip ♪".into();
        let ch = p.channels[0].id;
        p.current_mut().notes_mut(ch).push(Note::new(480, 960, 72));
        p.channels[0].instrument.params.insert("filter.cutoff".into(), 1234.0);
        save(&p, &path).unwrap();
        let back = load(&path).unwrap();
        assert_eq!(back, p);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn second_save_keeps_backup_and_leaves_no_temp() {
        let dir = temp_dir("backup");
        let path = dir.join("a.soundesti");
        let mut p = Project::new();
        save(&p, &path).unwrap();
        p.bpm = 99.0;
        save(&p, &path).unwrap();
        assert_eq!(load(&sibling(&path, ".bak")).unwrap().bpm, 130.0);
        assert_eq!(load(&path).unwrap().bpm, 99.0);
        assert!(!sibling(&path, ".tmp").exists());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn failed_save_does_not_touch_existing_file() {
        let dir = temp_dir("failsave");
        let path = dir.join("keep.soundesti");
        save(&Project::new(), &path).unwrap();
        let before = fs::read(&path).unwrap();
        // A directory where the temp file should go makes File::create fail.
        fs::create_dir_all(sibling(&path, ".tmp")).unwrap();
        let mut p = Project::new();
        p.bpm = 200.0;
        assert!(save(&p, &path).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn rejects_newer_format_and_garbage() {
        let dir = temp_dir("reject");
        let newer = dir.join("new.soundesti");
        let mut v = serde_json::to_value(Project::new()).unwrap();
        v["format_version"] = serde_json::json!(FORMAT_VERSION + 1);
        fs::write(&newer, serde_json::to_vec(&v).unwrap()).unwrap();
        assert!(matches!(load(&newer), Err(ProjectError::TooNew { .. })));
        let junk = dir.join("junk.soundesti");
        fs::write(&junk, b"\x00\x01 not json").unwrap();
        assert!(matches!(load(&junk), Err(ProjectError::Parse { .. })));
        assert!(matches!(load(&dir.join("missing.soundesti")), Err(ProjectError::Read { .. })));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let dir = temp_dir("unknown");
        let path = dir.join("x.soundesti");
        let mut v = serde_json::to_value(Project::new()).unwrap();
        v["future_feature"] = serde_json::json!({"anything": 1});
        fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
        assert!(load(&path).is_ok());
        fs::remove_dir_all(dir).ok();
    }
}
