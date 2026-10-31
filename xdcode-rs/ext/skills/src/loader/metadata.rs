use std::io;

use xdcode_exec_server::ExecutorFileSystem;
use xdcode_exec_server::GetMetadataOptions;
use xdcode_exec_server::ReadFileOptions;
use xdcode_skills::ParsedSkillMetadata;
use xdcode_skills::SkillDependencies;
use xdcode_skills::SkillInterface;
use xdcode_skills::SkillInterfaceAssetPolicy;
use xdcode_skills::SkillParseError;
use xdcode_skills::SkillPolicy;
use xdcode_skills::parse_skill_metadata;
use xdcode_skills::resolve_skill_interface;
use xdcode_utils_absolute_path::AbsolutePathBuf;
use xdcode_utils_path_uri::PathUri;

use super::SKILLS_METADATA_FILENAME;
use super::SKILLS_PORTABLE_METADATA_FILENAME;
use super::discovery::SkillMetadataDiscovery;

#[derive(Default)]
pub(super) struct LoadedSkillMetadata {
    pub(super) interface: Option<SkillInterface>,
    pub(super) dependencies: Option<SkillDependencies>,
    pub(super) policy: Option<SkillPolicy>,
}

/// Probe portable metadata only after the native path is absent, including through aliases.
pub(super) fn metadata_probe_paths(path: &PathUri) -> impl Iterator<Item = PathUri> {
    let portable = path
        .parent()
        .and_then(|directory| directory.join(SKILLS_PORTABLE_METADATA_FILENAME).ok())
        .filter(|portable| portable != path);
    std::iter::once(path.clone()).chain(portable)
}

pub(super) async fn load_host_skill_metadata(
    file_system: &dyn ExecutorFileSystem,
    skill_path: &AbsolutePathBuf,
    metadata: &SkillMetadataDiscovery,
    plugin_root: Option<&AbsolutePathBuf>,
) -> LoadedSkillMetadata {
    // Fail open: optional metadata should not block loading SKILL.md.
    let Some(skill_dir) = skill_path.parent() else {
        return LoadedSkillMetadata::default();
    };
    let metadata_path = match metadata {
        SkillMetadataDiscovery::Present(path) => path.clone(),
        SkillMetadataDiscovery::Absent => return LoadedSkillMetadata::default(),
        SkillMetadataDiscovery::Probe(path) => {
            let mut selected = None;
            for candidate in metadata_probe_paths(path) {
                match file_system
                    .get_metadata(
                        &candidate,
                        GetMetadataOptions::default(),
                        /*sandbox*/ None,
                    )
                    .await
                {
                    Ok(metadata) if metadata.is_file => {
                        selected = Some(candidate);
                        break;
                    }
                    Ok(_) => return LoadedSkillMetadata::default(),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(error) => {
                        tracing::warn!("ignoring {candidate}: failed to stat metadata: {error}");
                        return LoadedSkillMetadata::default();
                    }
                }
            }
            let Some(selected) = selected else {
                return LoadedSkillMetadata::default();
            };
            selected
        }
    };

    let contents = match file_system
        .read_file_text(
            &metadata_path,
            ReadFileOptions::default(),
            /*sandbox*/ None,
        )
        .await
    {
        Ok(contents) => contents,
        Err(error) => {
            tracing::warn!(
                "ignoring {metadata_path}: failed to read {label}: {error}",
                label = SKILLS_METADATA_FILENAME
            );
            return LoadedSkillMetadata::default();
        }
    };

    let parsed = match parse_skill_metadata(&contents) {
        Ok(parsed) => parsed,
        Err(error) => {
            tracing::warn!(
                "ignoring {metadata_path}: invalid {label}: {error}",
                label = SKILLS_METADATA_FILENAME
            );
            return LoadedSkillMetadata::default();
        }
    };

    let ParsedSkillMetadata {
        interface,
        dependencies,
        policy,
    } = parsed;
    let asset_policy = match plugin_root {
        Some(plugin_root) => SkillInterfaceAssetPolicy::PluginShared { plugin_root },
        None => SkillInterfaceAssetPolicy::LocalOnly,
    };
    LoadedSkillMetadata {
        interface: resolve_skill_interface(interface, &skill_dir, asset_policy),
        dependencies,
        policy,
    }
}
pub(super) fn sanitize_single_line(raw: &str) -> String {
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn validate_len(
    value: &str,
    max_len: usize,
    field_name: &'static str,
) -> Result<(), SkillParseError> {
    if value.is_empty() {
        return Err(SkillParseError::MissingField(field_name));
    }
    if value.chars().count() > max_len {
        return Err(SkillParseError::InvalidField {
            field: field_name,
            reason: format!("exceeds maximum length of {max_len} characters"),
        });
    }
    Ok(())
}
