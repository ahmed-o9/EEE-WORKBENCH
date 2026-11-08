//! Portable `.eeproj` manifests with validation, atomic saves and autosaves.
//! Large result arrays should be stored as separate compressed binary assets;
//! this manifest keeps project metadata and references compact.

pub mod minijson;

use minijson::{parse, Json};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const PROJECT_FORMAT: &str = "eeproj";
pub const SCHEMA_VERSION: u64 = 1;
const MAX_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    pub name: String,
    pub created_at_ms: u64,
    pub modified_at_ms: u64,
    pub active_studio: String,
    /// Studio-specific content. Large arrays should be external binary assets.
    pub studio_data: Json,
    pub simulation_config: Json,
    pub results: Json,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoverySource {
    Primary,
    Autosave,
    Backup,
    AutosaveBackup,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RecoveryResult {
    pub project: Project,
    pub source: RecoverySource,
}

impl Project {
    pub fn new(name: impl Into<String>) -> Result<Self, String> {
        let now = now_ms();
        let project = Self {
            name: name.into().trim().to_string(),
            created_at_ms: now,
            modified_at_ms: now,
            active_studio: "circuit".into(),
            studio_data: Json::Obj(Vec::new()),
            simulation_config: Json::Obj(Vec::new()),
            results: Json::Null,
        };
        project.validate()?;
        Ok(project)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("project name cannot be empty".into());
        }
        if self.created_at_ms == 0 || self.modified_at_ms == 0 {
            return Err("project timestamps must be non-zero Unix milliseconds".into());
        }
        if self.modified_at_ms < self.created_at_ms {
            return Err("project modified timestamp precedes creation timestamp".into());
        }
        if self.active_studio.trim().is_empty() {
            return Err("active studio cannot be empty".into());
        }
        // Ensure every nested number can be encoded as valid JSON.
        self.to_json().write_checked()?;
        Ok(())
    }

    pub fn to_json(&self) -> Json {
        Json::Obj(vec![
            ("format".into(), Json::Str(PROJECT_FORMAT.into())),
            ("schema_version".into(), Json::Num(SCHEMA_VERSION as f64)),
            (
                "metadata".into(),
                Json::Obj(vec![
                    ("name".into(), Json::Str(self.name.clone())),
                    ("created_at_ms".into(), Json::Num(self.created_at_ms as f64)),
                    (
                        "modified_at_ms".into(),
                        Json::Num(self.modified_at_ms as f64),
                    ),
                ]),
            ),
            (
                "active_studio".into(),
                Json::Str(self.active_studio.clone()),
            ),
            ("studio_data".into(), self.studio_data.clone()),
            ("simulation_config".into(), self.simulation_config.clone()),
            ("results".into(), self.results.clone()),
        ])
    }

    pub fn from_json(value: &Json) -> Result<Self, String> {
        let format = value
            .get("format")
            .and_then(Json::as_str)
            .ok_or("project format field is missing")?;
        if format != PROJECT_FORMAT {
            return Err(format!("unsupported project format '{format}'"));
        }
        let schema = value
            .get("schema_version")
            .and_then(Json::as_u64)
            .ok_or("project schema_version is missing or invalid")?;
        if schema != SCHEMA_VERSION {
            return Err(format!("unsupported .eeproj schema version {schema}; supported version is {SCHEMA_VERSION}"));
        }
        let metadata = value.get("metadata").ok_or("project metadata is missing")?;
        let name = metadata
            .get("name")
            .and_then(Json::as_str)
            .ok_or("project metadata.name is missing")?
            .to_string();
        let created_at_ms = metadata
            .get("created_at_ms")
            .and_then(Json::as_u64)
            .ok_or("project metadata.created_at_ms is missing or invalid")?;
        let modified_at_ms = metadata
            .get("modified_at_ms")
            .and_then(Json::as_u64)
            .ok_or("project metadata.modified_at_ms is missing or invalid")?;
        let active_studio = value
            .get("active_studio")
            .and_then(Json::as_str)
            .ok_or("project active_studio is missing")?
            .to_string();

        let project = Self {
            name,
            created_at_ms,
            modified_at_ms,
            active_studio,
            studio_data: value
                .get("studio_data")
                .cloned()
                .unwrap_or(Json::Obj(Vec::new())),
            simulation_config: value
                .get("simulation_config")
                .cloned()
                .unwrap_or(Json::Obj(Vec::new())),
            results: value.get("results").cloned().unwrap_or(Json::Null),
        };
        project.validate()?;
        Ok(project)
    }

    /// Save to a single portable JSON manifest. The previous file is retained
    /// as `.bak` until a subsequent successful save.
    pub fn save(&mut self, path: impl AsRef<Path>) -> Result<(), String> {
        self.modified_at_ms = now_ms()
            .max(self.created_at_ms)
            .max(self.modified_at_ms.saturating_add(1));
        self.validate()?;
        let data = self.to_json().write_checked()?.into_bytes();
        ensure_manifest_size(data.len())?;
        atomic_write(path.as_ref(), &data)
    }

    /// Write a crash-recovery copy next to the main project.
    pub fn autosave(&mut self, path: impl AsRef<Path>) -> Result<PathBuf, String> {
        self.modified_at_ms = now_ms()
            .max(self.created_at_ms)
            .max(self.modified_at_ms.saturating_add(1));
        self.validate()?;
        let autosave_path = autosave_path(path.as_ref());
        let data = self.to_json().write_checked()?.into_bytes();
        ensure_manifest_size(data.len())?;
        atomic_write(&autosave_path, &data)?;
        Ok(autosave_path)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        read_project(path.as_ref())
    }

    /// Load the newest valid primary/autosave/backup copy. Corrupt candidates
    /// are skipped and never silently replace a valid newer copy. The previous
    /// autosave is also considered if a crash interrupted its replacement.
    pub fn load_with_recovery(path: impl AsRef<Path>) -> Result<RecoveryResult, String> {
        let path = path.as_ref();
        let candidates = [
            (path.to_path_buf(), RecoverySource::Primary),
            (autosave_path(path), RecoverySource::Autosave),
            (backup_path(path), RecoverySource::Backup),
            (autosave_backup_path(path), RecoverySource::AutosaveBackup),
        ];
        let mut valid: Vec<(Project, RecoverySource)> = Vec::new();
        let mut errors = Vec::new();
        for (candidate, source) in candidates {
            if !candidate.exists() {
                continue;
            }
            match read_project(&candidate) {
                Ok(project) => valid.push((project, source)),
                Err(error) => errors.push(format!("{}: {error}", candidate.display())),
            }
        }
        valid.sort_by_key(|(project, source)| {
            let priority = match source {
                RecoverySource::Primary => 0_u8,
                RecoverySource::Backup => 1,
                RecoverySource::AutosaveBackup => 2,
                RecoverySource::Autosave => 3,
            };
            (project.modified_at_ms, priority)
        });
        if let Some((project, source)) = valid.pop() {
            return Ok(RecoveryResult { project, source });
        }
        if errors.is_empty() {
            Err(format!(
                "no project, autosave or backup file exists for {}",
                path.display()
            ))
        } else {
            Err(format!(
                "no valid project copy found: {}",
                errors.join("; ")
            ))
        }
    }
}

fn ensure_manifest_size(size: usize) -> Result<(), String> {
    if size as u64 > MAX_MANIFEST_BYTES {
        Err(format!(
            "project manifest is larger than the {} MiB safety limit; reduce stored result data",
            MAX_MANIFEST_BYTES / 1024 / 1024
        ))
    } else {
        Ok(())
    }
}

fn read_project(path: &Path) -> Result<Project, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    if metadata.len() > MAX_MANIFEST_BYTES {
        return Err(format!(
            "manifest exceeds the {} MiB safety limit",
            MAX_MANIFEST_BYTES / 1024 / 1024
        ));
    }
    let mut file =
        File::open(path).map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    let mut contents = String::with_capacity(metadata.len() as usize);
    file.read_to_string(&mut contents)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let json = parse(&contents).map_err(|error| format!("invalid project JSON: {error}"))?;
    Project::from_json(&json)
}

fn atomic_write(path: &Path, data: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| format!("cannot create project directory: {error}"))?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let mut temporary_name = path.as_os_str().to_os_string();
    temporary_name.push(format!(".tmp-{}-{nonce}", std::process::id()));
    let temporary = PathBuf::from(temporary_name);

    let write_result = (|| -> Result<(), String> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|error| format!("cannot create unique temporary project: {error}"))?;
        file.write_all(data)
            .map_err(|error| format!("cannot write temporary project: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("cannot flush temporary project: {error}"))?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }

    let backup = backup_path(path);
    let had_original = path.exists();
    if had_original {
        if backup.exists() {
            fs::remove_file(&backup)
                .map_err(|error| format!("cannot rotate project backup: {error}"))?;
        }
        fs::rename(path, &backup)
            .map_err(|error| format!("cannot preserve previous project: {error}"))?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if had_original {
            let _ = fs::rename(&backup, path);
        }
        let _ = fs::remove_file(&temporary);
        return Err(format!("cannot replace project file: {error}"));
    }
    Ok(())
}

fn autosave_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".autosave");
    PathBuf::from(name)
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".bak");
    PathBuf::from(name)
}

fn autosave_backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".autosave.bak");
    PathBuf::from(name)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

    fn temporary_directory() -> PathBuf {
        let test_id = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let directory =
            env::temp_dir().join(format!("eee-project-test-{}-{test_id}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn project_round_trip_persists_studio_data() {
        let directory = temporary_directory();
        let path = directory.join("divider.eeproj");
        let mut project = Project::new("Divider study").unwrap();
        project.studio_data = Json::Obj(vec![(
            "elements".into(),
            Json::Arr(vec![Json::Str("R1".into()), Json::Str("V1".into())]),
        )]);
        project.save(&path).unwrap();
        let loaded = Project::load(&path).unwrap();
        assert_eq!(loaded.name, "Divider study");
        assert_eq!(loaded.studio_data, project.studio_data);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn project_write_timestamps_increase_even_within_one_millisecond() {
        let directory = temporary_directory();
        let path = directory.join("monotonic.eeproj");
        let mut project = Project::new("Monotonic timestamp test").unwrap();
        project.save(&path).unwrap();
        let after_save = project.modified_at_ms;
        project.autosave(&path).unwrap();
        let after_autosave = project.modified_at_ms;
        project.save(&path).unwrap();
        assert!(after_autosave > after_save);
        assert!(project.modified_at_ms > after_autosave);
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn recovery_prefers_newer_autosave() {
        let directory = temporary_directory();
        let path = directory.join("test.eeproj");
        let mut original = Project::new("Recovery test").unwrap();
        original.save(&path).unwrap();
        let mut newer = original.clone();
        newer.modified_at_ms += 10_000;
        newer.name = "Recovered".into();
        newer.autosave(&path).unwrap();
        let recovered = Project::load_with_recovery(&path).unwrap();
        assert_eq!(recovered.source, RecoverySource::Autosave);
        assert_eq!(recovered.project.name, "Recovered");
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn recovery_can_use_previous_autosave_if_replacement_was_interrupted() {
        let directory = temporary_directory();
        let path = directory.join("autosave-crash.eeproj");
        let mut primary = Project::new("Primary").unwrap();
        primary.save(&path).unwrap();

        let mut autosaved = primary.clone();
        autosaved.name = "Autosave copy".into();
        let autosave_file = autosaved.autosave(&path).unwrap();
        let interrupted_backup = autosave_backup_path(&path);
        fs::rename(&autosave_file, &interrupted_backup).unwrap();

        let recovered = Project::load_with_recovery(&path).unwrap();
        assert_eq!(recovered.source, RecoverySource::AutosaveBackup);
        assert_eq!(recovered.project.name, "Autosave copy");
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn manifest_write_limit_is_checked_before_disk_write() {
        assert!(ensure_manifest_size((MAX_MANIFEST_BYTES + 1) as usize).is_err());
        assert!(ensure_manifest_size(MAX_MANIFEST_BYTES as usize).is_ok());
    }

    #[test]
    fn rejects_future_schema_versions() {
        let mut project = Project::new("schema").unwrap().to_json();
        if let Json::Obj(entries) = &mut project {
            entries
                .iter_mut()
                .find(|(key, _)| key == "schema_version")
                .unwrap()
                .1 = Json::Num(99.0);
        }
        assert!(Project::from_json(&project).is_err());
    }
}
