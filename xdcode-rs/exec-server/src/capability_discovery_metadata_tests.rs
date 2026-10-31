use super::*;
use crate::LOCAL_FS;

#[tokio::test]
async fn present_native_metadata_does_not_fall_back_on_read_errors_or_limits() {
    for failure in ["oversized", "invalid-utf8", "directory"] {
        let root = tempfile::tempdir().expect("temporary root");
        let skill = root.path().join("demo");
        std::fs::create_dir_all(skill.join("agents")).expect("metadata directory");
        std::fs::write(
            skill.join(SKILL_FILE_NAME),
            "---\nname: demo\ndescription: Demo.\n---\n",
        )
        .expect("skill instructions");
        let native = skill.join(SKILL_METADATA_PATH);
        let portable = skill.join("agents").join(SKILLS_PORTABLE_METADATA_FILENAME);
        assert_ne!(native, portable);
        std::fs::write(&portable, "policy:\n  allow_implicit_invocation: false\n")
            .expect("portable metadata");
        match failure {
            "oversized" => {
                std::fs::write(&native, vec![b'x'; MAX_FILE_BYTES + 1]).expect("large native")
            }
            "invalid-utf8" => std::fs::write(&native, [0xff]).expect("invalid native"),
            "directory" => std::fs::create_dir(&native).expect("native directory"),
            _ => unreachable!(),
        }
        let discovered = discover_root(
            LOCAL_FS.as_ref(),
            CapabilityRootDiscoverRequest {
                id: "test".into(),
                path: PathUri::from_host_native_path(root.path()).expect("root URI"),
                sandbox: None,
            },
        )
        .await;
        assert!(discovered.error.is_none());
        assert_eq!(discovered.skills.len(), 1);
        assert!(discovered.skills[0].metadata.is_none(), "{failure}");
        if failure == "directory" {
            assert!(discovered.warnings.is_empty());
        } else {
            assert!(
                discovered
                    .warnings
                    .iter()
                    .any(|warning| warning.contains(SKILL_METADATA_PATH)),
                "{failure}"
            );
        }
    }
}

#[tokio::test]
async fn portable_metadata_obeys_the_same_file_and_bundle_limits() {
    let root = tempfile::tempdir().expect("temporary root");
    let native =
        PathUri::from_host_native_path(root.path().join(SKILL_METADATA_PATH)).expect("native path");
    let portable_path = root
        .path()
        .join("agents")
        .join(SKILLS_PORTABLE_METADATA_FILENAME);
    std::fs::create_dir_all(portable_path.parent().expect("metadata parent"))
        .expect("metadata directory");
    let portable = PathUri::from_host_native_path(&portable_path).expect("portable path");
    for (contents, bytes) in [
        (vec![b'x'; MAX_FILE_BYTES + 1], 0),
        (vec![b'x'], MAX_BUNDLE_BYTES_PER_ROOT),
    ] {
        std::fs::write(&portable_path, contents).expect("portable metadata");
        let mut budget = BundleBudget { bytes };
        let mut warnings = Vec::new();
        assert!(
            read_optional_text_file_with_fallback(
                LOCAL_FS.as_ref(),
                native.clone(),
                Some(portable.clone()),
                None,
                &mut budget,
                &mut warnings,
            )
            .await
            .is_none()
        );
        assert_eq!(budget.bytes, bytes);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("limit"));
    }
}
