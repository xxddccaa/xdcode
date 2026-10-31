//! Per-worktree model preferences, separate from the user's provider configuration.
//!
//! Callers write only after the app server accepts an atomic selection. Explicit launch overrides
//! and historical session restoration never call `remember`, so they cannot replace preferences.
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use xdcode_protocol::config_types::ModeKind;
use xdcode_protocol::model_types::ReasoningEffort;
use xdcode_protocol::protocol::CatalogModelSelection;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct ProjectModelMemory {
    pub normal: Option<CatalogModelSelection>,
    pub plan: Option<CatalogModelSelection>,
    pub last_mode: Option<ModeKind>,
    #[serde(default)]
    pub last_efforts: BTreeMap<String, Option<ReasoningEffort>>,
}

pub(crate) fn project_root(cwd: &Path) -> PathBuf {
    let root = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|root| PathBuf::from(root.trim_end()));
    let root = root.unwrap_or_else(|| cwd.to_path_buf());
    dunce::canonicalize(&root).unwrap_or(root)
}

pub(crate) fn memory_path(home: &Path, source: &Path, cwd: &Path) -> PathBuf {
    let source = dunce::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
    let mut hash = sha2::Sha256::new();
    hash.update(source.as_os_str().as_encoded_bytes());
    hash.update([0]);
    hash.update(project_root(cwd).as_os_str().as_encoded_bytes());
    home.join("state")
        .join("model-selections")
        .join(format!("{:x}.json", hash.finalize()))
}

impl ProjectModelMemory {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        match std::fs::read(path) {
            Ok(contents) => Ok(serde_json::from_slice(&contents)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    pub fn selection(&self, mode: ModeKind) -> Option<&CatalogModelSelection> {
        match mode {
            ModeKind::Default => self.normal.as_ref(),
            ModeKind::Plan => self.plan.as_ref().or(self.normal.as_ref()),
        }
    }

    pub fn remember(&mut self, selection: CatalogModelSelection) {
        self.last_mode = Some(selection.mode);
        self.last_efforts
            .insert(selection.catalog_id.clone(), selection.effort.clone());
        match selection.mode {
            ModeKind::Default => self.normal = Some(selection),
            ModeKind::Plan => self.plan = Some(selection),
        }
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let parent = path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("invalid model memory path"))?;
        std::fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(&serde_json::to_vec_pretty(self)?)?;
        temporary.as_file().sync_all()?;
        temporary.persist(path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(id: &str, mode: ModeKind, effort: Option<ReasoningEffort>) -> CatalogModelSelection {
        CatalogModelSelection {
            provider: "provider".into(),
            catalog_id: id.into(),
            model: "same-model-name".into(),
            effort,
            mode,
        }
    }

    #[test]
    fn modes_are_independent_and_first_plan_inherits_normal() {
        let mut memory = ProjectModelMemory::default();
        memory.remember(choice(
            "first",
            ModeKind::Default,
            Some(ReasoningEffort::High),
        ));
        assert_eq!(
            memory.selection(ModeKind::Plan).unwrap().catalog_id,
            "first"
        );
        memory.remember(choice("second", ModeKind::Plan, None));
        assert_eq!(
            memory.selection(ModeKind::Default).unwrap().catalog_id,
            "first"
        );
        assert_eq!(
            memory.selection(ModeKind::Plan).unwrap().catalog_id,
            "second"
        );
        assert_eq!(memory.last_mode, Some(ModeKind::Plan));
        assert_eq!(memory.last_efforts["second"], None);
    }

    #[test]
    fn config_files_and_non_git_projects_have_separate_memory() {
        let temp = tempfile::tempdir().unwrap();
        let project_a = temp.path().join("a");
        let project_b = temp.path().join("b");
        std::fs::create_dir(&project_a).unwrap();
        std::fs::create_dir(&project_b).unwrap();
        let a = memory_path(temp.path(), Path::new("/config/a"), &project_a);
        assert_ne!(
            a,
            memory_path(temp.path(), Path::new("/config/b"), &project_a)
        );
        assert_ne!(
            a,
            memory_path(temp.path(), Path::new("/config/a"), &project_b)
        );
    }

    #[test]
    fn git_worktrees_and_nested_directories_keep_correct_project_identity() {
        let temp = tempfile::tempdir().unwrap();
        let main = temp.path().join("main");
        let worktree = temp.path().join("linked");
        std::fs::create_dir(&main).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(&main)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .args([
                    "-c",
                    "user.name=Test",
                    "-c",
                    "user.email=test@example.invalid",
                    "commit",
                    "--allow-empty",
                    "-qm",
                    "initial"
                ])
                .current_dir(&main)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("git")
                .args(["worktree", "add", "-qb", "linked"])
                .arg(&worktree)
                .current_dir(&main)
                .status()
                .unwrap()
                .success()
        );
        let nested = main.join("subdir");
        std::fs::create_dir(&nested).unwrap();
        let source = temp.path().join("config.toml");
        assert_eq!(
            memory_path(temp.path(), &source, &main),
            memory_path(temp.path(), &source, &nested)
        );
        assert_ne!(
            memory_path(temp.path(), &source, &main),
            memory_path(temp.path(), &source, &worktree)
        );
    }

    #[test]
    fn atomic_round_trip_preserves_custom_effort_and_service_default() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("memory.json");
        let mut memory = ProjectModelMemory::default();
        memory.remember(choice(
            "first",
            ModeKind::Default,
            Some(ReasoningEffort::Custom("vendor-deep".into())),
        ));
        memory.remember(choice("second", ModeKind::Plan, None));
        memory.save(&path).unwrap();
        let loaded = ProjectModelMemory::load(&path).unwrap();
        assert_eq!(
            loaded.normal.unwrap().effort,
            Some(ReasoningEffort::Custom("vendor-deep".into()))
        );
        assert_eq!(loaded.plan.unwrap().effort, None);
    }
}
