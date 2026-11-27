mod interface;
mod invocation;
mod loading;
mod mentions;
mod metadata;
mod model;
mod name_counts;
mod parser;
mod selection;

pub use interface::SkillInterfaceAssetPolicy;
pub use interface::SkillInterfaceFile;
pub use interface::resolve_skill_interface;
pub use invocation::ImplicitSkillAccess;
pub use invocation::ImplicitSkillLookup;
pub use invocation::detect_implicit_skill_invocation_for_command;
pub use invocation::implicit_skill_accesses_for_command;
pub use loading::LoadedSkillRoot;
pub use loading::LoadedSkills;
pub use loading::SkillError;
pub use loading::SkillLoadFuture;
pub use loading::SkillRootLoadRequest;
pub use loading::SkillRootLoader;
pub use loading::SkillRootSnapshotCache;
pub use loading::SkillRootSnapshots;
pub use mentions::ToolMentionKind;
pub use mentions::ToolMentions;
pub use mentions::app_id_from_path;
pub use mentions::extract_tool_mentions;
pub use mentions::extract_tool_mentions_with_sigil;
pub use mentions::normalize_skill_path;
pub use mentions::plugin_config_name_from_path;
pub use mentions::tool_kind_for_path;
pub use metadata::ParsedSkillMetadata;
pub use metadata::parse_skill_metadata;
pub use model::EnvironmentSkillMetadata;
pub use model::SkillDependencies;
pub use model::SkillInterface;
pub use model::SkillMetadata;
pub use model::SkillPolicy;
pub use model::SkillToolDependency;
pub use name_counts::build_skill_name_counts;
pub use parser::ParsedSkillFrontmatter;
pub use parser::SkillParseError;
pub use parser::parse_skill_frontmatter_metadata;
pub use selection::ExplicitSkillLookup;
pub use selection::collect_explicit_skill_mentions;

use include_dir::Dir;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::Hash;
use std::hash::Hasher;
use xdcode_utils_absolute_path::AbsolutePathBuf;

use thiserror::Error;

const SYSTEM_SKILLS_DIR: Dir = include_dir::include_dir!("$CARGO_MANIFEST_DIR/src/assets/samples");

const SYSTEM_SKILLS_DIR_NAME: &str = ".system";
const SKILLS_DIR_NAME: &str = "skills";
const SYSTEM_SKILLS_MARKER_FILENAME: &str = ".xdcode-system-skills.marker";
const SYSTEM_SKILLS_MARKER_SALT: &str = "v1";

/// Returns the on-disk cache location for embedded system skills from an absolute XDCODE_HOME.
pub fn system_cache_root_dir(xdcode_home: &AbsolutePathBuf) -> AbsolutePathBuf {
    xdcode_home
        .join(SKILLS_DIR_NAME)
        .join(SYSTEM_SKILLS_DIR_NAME)
}

/// Installs embedded system skills into `XDCODE_HOME/skills/.system`.
///
/// Clears any existing system skills directory first and then writes the embedded
/// skills directory into place.
///
/// To avoid doing unnecessary work on every startup, a marker file is written
/// with a fingerprint of the embedded directory. When the marker matches, the
/// install is skipped.
pub fn install_system_skills(xdcode_home: &AbsolutePathBuf) -> Result<(), SystemSkillsError> {
    let skills_root_dir = xdcode_home.join(SKILLS_DIR_NAME);
    fs::create_dir_all(skills_root_dir.as_path())
        .map_err(|source| SystemSkillsError::io("create skills root dir", source))?;

    let dest_system = system_cache_root_dir(xdcode_home);

    let marker_path = dest_system.join(SYSTEM_SKILLS_MARKER_FILENAME);
    let expected_fingerprint = embedded_system_skills_fingerprint();
    if dest_system.as_path().is_dir()
        && read_marker(&marker_path).is_ok_and(|marker| marker == expected_fingerprint)
    {
        return Ok(());
    }

    if dest_system.as_path().exists() {
        fs::remove_dir_all(dest_system.as_path())
            .map_err(|source| SystemSkillsError::io("remove existing system skills dir", source))?;
    }

    write_embedded_dir(&SYSTEM_SKILLS_DIR, &dest_system)?;
    fs::write(marker_path.as_path(), format!("{expected_fingerprint}\n"))
        .map_err(|source| SystemSkillsError::io("write system skills marker", source))?;
    Ok(())
}

fn read_marker(path: &AbsolutePathBuf) -> Result<String, SystemSkillsError> {
    Ok(fs::read_to_string(path.as_path())
        .map_err(|source| SystemSkillsError::io("read system skills marker", source))?
        .trim()
        .to_string())
}

fn embedded_system_skills_fingerprint() -> String {
    let mut items = Vec::new();
    collect_fingerprint_items(&SYSTEM_SKILLS_DIR, &mut items);
    items.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));

    let mut hasher = DefaultHasher::new();
    SYSTEM_SKILLS_MARKER_SALT.hash(&mut hasher);
    for (path, contents_hash) in items {
        path.hash(&mut hasher);
        contents_hash.hash(&mut hasher);
    }
    format!("{:x}", hasher.finish())
}

fn collect_fingerprint_items(dir: &Dir<'_>, items: &mut Vec<(String, Option<u64>)>) {
    for entry in dir.entries() {
        match entry {
            include_dir::DirEntry::Dir(subdir) => {
                items.push((subdir.path().to_string_lossy().to_string(), None));
                collect_fingerprint_items(subdir, items);
            }
            include_dir::DirEntry::File(file) => {
                let mut file_hasher = DefaultHasher::new();
                file.contents().hash(&mut file_hasher);
                items.push((
                    file.path().to_string_lossy().to_string(),
                    Some(file_hasher.finish()),
                ));
            }
        }
    }
}

/// Writes the embedded `include_dir::Dir` to disk under `dest`.
///
/// Preserves the embedded directory structure.
fn write_embedded_dir(dir: &Dir<'_>, dest: &AbsolutePathBuf) -> Result<(), SystemSkillsError> {
    fs::create_dir_all(dest.as_path())
        .map_err(|source| SystemSkillsError::io("create system skills dir", source))?;

    for entry in dir.entries() {
        match entry {
            include_dir::DirEntry::Dir(subdir) => {
                let subdir_dest = dest.join(subdir.path());
                fs::create_dir_all(subdir_dest.as_path()).map_err(|source| {
                    SystemSkillsError::io("create system skills subdir", source)
                })?;
                write_embedded_dir(subdir, dest)?;
            }
            include_dir::DirEntry::File(file) => {
                let path = dest.join(file.path());
                if let Some(parent) = path.as_path().parent() {
                    fs::create_dir_all(parent).map_err(|source| {
                        SystemSkillsError::io("create system skills file parent", source)
                    })?;
                }
                fs::write(path.as_path(), file.contents())
                    .map_err(|source| SystemSkillsError::io("write system skill file", source))?;
            }
        }
    }

    Ok(())
}

#[derive(Debug, Error)]
pub enum SystemSkillsError {
    #[error("io error while {action}: {source}")]
    Io {
        action: &'static str,
        #[source]
        source: std::io::Error,
    },
}

impl SystemSkillsError {
    fn io(action: &'static str, source: std::io::Error) -> Self {
        Self::Io { action, source }
    }
}

#[cfg(test)]
mod tests {
    use super::SYSTEM_SKILLS_DIR;
    use super::collect_fingerprint_items;
    use super::install_system_skills;
    use super::system_cache_root_dir;
    use crate::parse_skill_frontmatter_metadata;
    use crate::parse_skill_metadata;
    use std::fs;
    use xdcode_utils_absolute_path::AbsolutePathBuf;

    #[test]
    fn fingerprint_traverses_nested_entries() {
        let mut items = Vec::new();
        collect_fingerprint_items(&SYSTEM_SKILLS_DIR, &mut items);
        let mut paths: Vec<String> = items.into_iter().map(|(path, _)| path).collect();
        paths.sort_unstable();

        assert!(
            paths
                .binary_search_by(|probe| probe.as_str().cmp("skill-creator/SKILL.md"))
                .is_ok()
        );
        assert!(
            paths
                .binary_search_by(|probe| probe.as_str().cmp("skill-creator/scripts/init_skill.py"))
                .is_ok()
        );
    }

    #[test]
    fn model_config_skill_and_its_references_are_embedded_and_fingerprinted() {
        let mut items = Vec::new();
        collect_fingerprint_items(&SYSTEM_SKILLS_DIR, &mut items);
        for path in [
            "xdcode-model-config/SKILL.md",
            "xdcode-model-config/agents/xdcode.yaml",
            "xdcode-model-config/references/configuration.md",
            "xdcode-model-config/references/openrouter.md",
            "xdcode-model-config/scripts/openrouter_model_id.py",
        ] {
            let file = SYSTEM_SKILLS_DIR
                .get_file(path)
                .expect("embedded skill resource");
            assert!(
                items
                    .iter()
                    .any(|(entry, hash)| entry == path && hash.is_some())
            );
            assert!(!file.contents().is_empty());
        }
        let skill = SYSTEM_SKILLS_DIR
            .get_file("xdcode-model-config/SKILL.md")
            .unwrap()
            .contents_utf8()
            .unwrap();
        let metadata = parse_skill_frontmatter_metadata(skill, || "unused".into()).unwrap();
        assert_eq!(metadata.name, "xdcode-model-config");
    }

    #[test]
    fn installs_model_config_resources_independently_of_the_project_directory() {
        // A temporary runtime home exercises the real installer without touching
        // the user's installed skills or requiring a source checkout at runtime.
        let temp = tempfile::tempdir().unwrap();
        let home = AbsolutePathBuf::try_from(temp.path()).unwrap();
        install_system_skills(&home).unwrap();
        let installed = system_cache_root_dir(&home).join("xdcode-model-config");
        for path in [
            "SKILL.md",
            "agents/xdcode.yaml",
            "references/configuration.md",
            "references/openrouter.md",
            "scripts/openrouter_model_id.py",
        ] {
            let embedded = SYSTEM_SKILLS_DIR
                .get_file(format!("xdcode-model-config/{path}"))
                .unwrap();
            assert_eq!(
                fs::read(installed.join(path).as_path()).unwrap(),
                embedded.contents()
            );
        }
        // A matching content marker preserves the installed cache.
        let sentinel = installed.join("local-marker");
        fs::write(sentinel.as_path(), "keep while fingerprint is unchanged").unwrap();
        install_system_skills(&home).unwrap();
        assert!(sentinel.as_path().exists());
        // A stale marker triggers replacement, bringing resources back in sync.
        fs::write(
            system_cache_root_dir(&home)
                .join(".xdcode-system-skills.marker")
                .as_path(),
            "obsolete\n",
        )
        .unwrap();
        install_system_skills(&home).unwrap();
        assert!(!sentinel.as_path().exists());
        assert!(
            installed
                .join("references/configuration.md")
                .as_path()
                .is_file()
        );
    }
}
