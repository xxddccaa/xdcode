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

    #[test]
    fn delegate_resources_are_installed_parsed_and_fingerprinted() {
        let temp = tempfile::tempdir().unwrap();
        let home = AbsolutePathBuf::try_from(temp.path()).unwrap();
        install_system_skills(&home).unwrap();
        let installed = system_cache_root_dir(&home).join("xdcode-delegate");
        let mut fingerprint_items = Vec::new();
        collect_fingerprint_items(&SYSTEM_SKILLS_DIR, &mut fingerprint_items);
        for resource in [
            "SKILL.md",
            "agents/xdcode.yaml",
            "references/native.md",
            "references/exec.md",
        ] {
            let path = format!("xdcode-delegate/{resource}");
            let embedded = SYSTEM_SKILLS_DIR.get_file(&path).unwrap();
            assert_eq!(
                fs::read(installed.join(resource).as_path()).unwrap(),
                embedded.contents()
            );
            assert!(
                fingerprint_items
                    .iter()
                    .any(|(entry, hash)| { entry == &path && hash.is_some() })
            );
        }
        // Exercise the production parsers on installed files, not wording checks.
        // Filesystem root discovery lives in xdcode-skills-extension.
        let instructions = fs::read_to_string(installed.join("SKILL.md").as_path()).unwrap();
        let frontmatter =
            parse_skill_frontmatter_metadata(&instructions, || "unused".into()).unwrap();
        assert_eq!(frontmatter.name, "xdcode-delegate");
        let metadata = parse_skill_metadata(
            &fs::read_to_string(installed.join("agents/xdcode.yaml").as_path()).unwrap(),
        )
        .unwrap();
        assert!(metadata.interface.is_some());
        assert!(
            metadata
                .policy
                .as_ref()
                .and_then(|policy| policy.allow_implicit_invocation)
                .unwrap_or(true)
        );
    }

    #[test]
    fn stale_system_cache_restores_delegate_references() {
        let temp = tempfile::tempdir().unwrap();
        let home = AbsolutePathBuf::try_from(temp.path()).unwrap();
        install_system_skills(&home).unwrap();
        let system = system_cache_root_dir(&home);
        let reference = system.join("xdcode-delegate/references/native.md");
        fs::write(reference.as_path(), "local cache sentinel").unwrap();
        // Matching content markers avoid reinstalling on every startup.
        install_system_skills(&home).unwrap();
        assert_eq!(
            fs::read_to_string(reference.as_path()).unwrap(),
            "local cache sentinel"
        );
        fs::write(
            system.join(super::SYSTEM_SKILLS_MARKER_FILENAME).as_path(),
            "old\n",
        )
        .unwrap();
        install_system_skills(&home).unwrap();
        assert_eq!(
            fs::read(reference.as_path()).unwrap(),
            SYSTEM_SKILLS_DIR
                .get_file("xdcode-delegate/references/native.md")
                .unwrap()
                .contents()
        );
    }

    #[test]
    #[ignore = "requires XDCODE_SKILLS_TEST_BIN pointing to a newly built xdcode; starts an isolated app-server"]
    fn delegate_is_discovered_by_real_runtime_loader() {
        use std::io::BufRead;
        use std::io::Write;
        use std::process::Command;
        use std::process::Stdio;
        use std::sync::mpsc;
        use std::time::Duration;
        use std::time::Instant;

        // The skills extension depends on this crate, so use its real RPC loader
        // through a supplied build rather than introducing a dependency cycle.
        // Use a temporary application home and cwd; no model turn or installed
        // service is started. Normal read-only host skill roots may also be scanned.
        struct OwnedServer(std::process::Child);
        impl Drop for OwnedServer {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let binary = std::env::var_os("XDCODE_SKILLS_TEST_BIN")
            .expect("supply the newly built xdcode binary explicitly");
        let temp = tempfile::tempdir().unwrap();
        fs::write(
            temp.path().join("config.toml"),
            "model = \"loader-fixture\"\nmodel_provider = \"fixture\"\n\
             [model_providers.fixture]\nname = \"Loader fixture\"\n\
             base_url = \"http://127.0.0.1:9/v1\"\nwire_api = \"responses\"\n",
        )
        .unwrap();
        let home = AbsolutePathBuf::try_from(temp.path()).unwrap();
        // Only the supplied binary may populate this fresh system cache.
        assert!(!system_cache_root_dir(&home).exists());
        let mut server = OwnedServer(
            Command::new(binary)
                .args(["app-server", "--listen", "stdio://"])
                .env("XDCODE_HOME", temp.path())
                .current_dir(temp.path())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let stdout = server.0.stdout.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines() {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let deadline = Instant::now() + Duration::from_secs(30);
        let response = |id: i64| loop {
            let line = receiver
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("runtime loader RPC did not finish before the deadline")
                .unwrap();
            let value: serde_yaml::Value = serde_yaml::from_str(&line).unwrap();
            if value["id"].as_i64() == Some(id) {
                assert!(value["error"].is_null(), "RPC error: {value:?}");
                break value;
            }
        };
        let mut stdin = server.0.stdin.take().unwrap();
        writeln!(stdin, "{}", r#"{"id":1,"method":"initialize","params":{"clientInfo":{"name":"xdcode_skills_test","version":"0.1.0"},"capabilities":{"experimentalApi":true}}}"#).unwrap();
        stdin.flush().unwrap();
        response(1);
        writeln!(stdin, "{}", r#"{"method":"initialized","params":{}}"#).unwrap();
        writeln!(
            stdin,
            "{}",
            r#"{"id":2,"method":"skills/list","params":{"forceReload":true}}"#
        )
        .unwrap();
        stdin.flush().unwrap();
        let loaded = response(2);
        let skill = loaded["result"]["data"]
            .as_sequence()
            .unwrap()
            .iter()
            .flat_map(|entry| entry["skills"].as_sequence().unwrap())
            .find(|skill| skill["name"].as_str() == Some("xdcode-delegate"))
            .expect("real runtime loader must discover the embedded delegate skill");
        assert_eq!(skill["scope"].as_str(), Some("system"));
        assert_eq!(skill["enabled"].as_bool(), Some(true));
        let installed_path = std::path::Path::new(skill["path"].as_str().unwrap());
        assert_eq!(
            installed_path,
            system_cache_root_dir(&home)
                .join("xdcode-delegate/SKILL.md")
                .as_path()
        );
        for reference in ["references/native.md", "references/exec.md"] {
            let embedded = SYSTEM_SKILLS_DIR
                .get_file(format!("xdcode-delegate/{reference}"))
                .unwrap();
            assert_eq!(
                fs::read(installed_path.parent().unwrap().join(reference)).unwrap(),
                embedded.contents()
            );
        }
        drop(stdin);
        drop(server);
        reader.join().unwrap();
    }
}
