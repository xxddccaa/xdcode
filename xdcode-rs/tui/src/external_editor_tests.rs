use super::editor_directory;
#[cfg(unix)]
use super::run_editor;
use pretty_assertions::assert_eq;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use tempfile::TempDir;
use xdcode_protocol::permissions::FileSystemAccessMode;
use xdcode_protocol::permissions::FileSystemSandboxEntry;
use xdcode_protocol::permissions::FileSystemSandboxPolicy;
use xdcode_utils_absolute_path::AbsolutePathBuf;

struct EditorPaths {
    _root: TempDir,
    xdcode_home: PathBuf,
    cwd: PathBuf,
}

impl EditorPaths {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("create editor test root");
        let canonical_root =
            dunce::canonicalize(root.path()).expect("canonicalize editor test root");
        let xdcode_home = canonical_root.join("xdcode-home");
        let cwd = canonical_root.join("workspace");
        fs::create_dir(&xdcode_home).expect("create Xdcode home");
        fs::create_dir(&cwd).expect("create workspace");

        Self {
            _root: root,
            xdcode_home,
            cwd,
        }
    }
}

fn workspace_write_policy(writable_roots: &[&Path]) -> FileSystemSandboxPolicy {
    let writable_roots = writable_roots
        .iter()
        .map(|root| AbsolutePathBuf::from_absolute_path(root).expect("absolute writable root"))
        .collect::<Vec<_>>();

    FileSystemSandboxPolicy::workspace_write(
        &writable_roots,
        /*exclude_tmpdir_env_var*/ true,
        /*exclude_slash_tmp*/ true,
    )
}

#[test]
fn editor_directory_is_inside_isolated_xdcode_home() {
    let paths = EditorPaths::new();
    let policy = workspace_write_policy(&[]);

    let directory = editor_directory(&[&paths.xdcode_home], &policy, &paths.cwd)
        .expect("create isolated editor directory");

    assert_eq!(directory, paths.xdcode_home.join("editor"));
    assert!(directory.is_dir());
}

#[test]
fn editor_directory_rejects_writable_home_editor_and_parent() {
    let paths = EditorPaths::new();
    let editor = paths.xdcode_home.join("editor");
    fs::create_dir(&editor).expect("create editor directory");
    let parent = paths.xdcode_home.parent().expect("Xdcode home parent");

    for writable_root in [paths.xdcode_home.as_path(), editor.as_path(), parent] {
        let policy = workspace_write_policy(&[writable_root]);

        assert!(
            editor_directory(&[&paths.xdcode_home], &policy, &paths.cwd).is_err(),
            "writable root {} must not expose editor buffers",
            writable_root.display()
        );
    }
}

#[test]
fn editor_directory_rejects_writable_descendant() {
    let paths = EditorPaths::new();
    let writable_descendant = paths.xdcode_home.join("editor").join("nested");
    fs::create_dir_all(&writable_descendant).expect("create writable editor descendant");
    let policy = workspace_write_policy(&[&writable_descendant]);

    assert!(editor_directory(&[&paths.xdcode_home], &policy, &paths.cwd).is_err());
}

#[test]
fn editor_directory_rejects_read_only_carveout_with_writable_parent() {
    let paths = EditorPaths::new();
    let editor = paths.xdcode_home.join("editor");
    fs::create_dir(&editor).expect("create editor directory");
    let policy = FileSystemSandboxPolicy::restricted(vec![
        FileSystemSandboxEntry::new(
            AbsolutePathBuf::from_absolute_path(&paths.xdcode_home)
                .expect("absolute Xdcode home")
                .into(),
            FileSystemAccessMode::Write,
        ),
        FileSystemSandboxEntry::new(
            AbsolutePathBuf::from_absolute_path(&editor)
                .expect("absolute editor directory")
                .into(),
            FileSystemAccessMode::Read,
        ),
    ]);

    assert!(editor_directory(&[&paths.xdcode_home], &policy, &paths.cwd).is_err());
}

#[test]
#[cfg(unix)]
fn editor_directory_rejects_preexisting_symlink() {
    use std::os::unix::fs::symlink;

    let paths = EditorPaths::new();
    let outside = paths.cwd.join("outside");
    fs::create_dir(&outside).expect("create editor symlink target");
    symlink(&outside, paths.xdcode_home.join("editor")).expect("create editor directory symlink");
    let policy = FileSystemSandboxPolicy::read_only();

    assert!(editor_directory(&[&paths.xdcode_home], &policy, &paths.cwd).is_err());
}

#[test]
#[cfg(unix)]
fn editor_directory_rejects_writable_xdcode_home_alias() {
    use std::os::unix::fs::symlink;

    let paths = EditorPaths::new();
    let aliased_home = paths.cwd.join("xdcode-home-link");
    symlink(&paths.xdcode_home, &aliased_home).expect("create Xdcode home symlink");
    let policy = workspace_write_policy(&[]);

    assert!(policy.can_write_local_path_with_cwd(&aliased_home.join("editor"), &paths.cwd));
    assert!(!policy.can_write_local_path_with_cwd(&paths.xdcode_home.join("editor"), &paths.cwd));
    assert!(editor_directory(&[&aliased_home], &policy, &paths.cwd).is_err());
}

#[test]
#[cfg(unix)]
fn editor_directory_rejects_writable_xdcode_home_alias_target() {
    use std::os::unix::fs::symlink;

    let paths = EditorPaths::new();
    let alias_parent = paths
        .xdcode_home
        .parent()
        .expect("Xdcode home parent")
        .join("aliases");
    fs::create_dir(&alias_parent).expect("create protected alias parent");
    let aliased_home = alias_parent.join("xdcode-home-link");
    symlink(&paths.xdcode_home, &aliased_home).expect("create Xdcode home symlink");
    let policy = workspace_write_policy(&[&paths.xdcode_home]);

    assert!(!policy.can_write_local_path_with_cwd(&aliased_home.join("editor"), &paths.cwd));
    assert!(policy.can_write_local_path_with_cwd(&paths.xdcode_home.join("editor"), &paths.cwd));
    assert!(editor_directory(&[&aliased_home], &policy, &paths.cwd).is_err());
}

#[test]
#[cfg(unix)]
fn editor_directory_uses_protected_workspace_fallback_with_default_temporary_grants() {
    let root = tempfile::tempdir().expect("create editor test root");
    let xdcode_home = root.path().join("xdcode-home");
    let cwd = root.path().join("workspace");
    fs::create_dir(&xdcode_home).expect("create Xdcode home");
    fs::create_dir(&cwd).expect("create workspace");
    let workspace_xdcode_home = cwd.join(".xdcode");
    let policy = FileSystemSandboxPolicy::workspace_write(
        &[],
        /*exclude_tmpdir_env_var*/ false,
        /*exclude_slash_tmp*/ false,
    );

    assert!(!workspace_xdcode_home.exists());
    assert!(policy.can_write_local_path_with_cwd(&xdcode_home, &cwd));
    assert!(!policy.can_write_local_path_with_cwd(&workspace_xdcode_home, &cwd));
    assert!(!policy.can_write_local_path_with_cwd(&workspace_xdcode_home.join("editor"), &cwd));

    let directory = editor_directory(&[&xdcode_home, &workspace_xdcode_home], &policy, &cwd)
        .expect("use protected workspace metadata directory");

    assert_eq!(
        directory,
        dunce::canonicalize(&workspace_xdcode_home)
            .expect("canonicalize workspace metadata directory")
            .join("editor")
    );
    assert!(directory.is_dir());
}

#[test]
#[cfg(unix)]
fn editor_directory_rejects_explicitly_writable_workspace_fallback() {
    let root = tempfile::tempdir().expect("create editor test root");
    let xdcode_home = root.path().join("xdcode-home");
    let cwd = root.path().join("workspace");
    fs::create_dir(&xdcode_home).expect("create Xdcode home");
    fs::create_dir(&cwd).expect("create workspace");
    let workspace_xdcode_home = cwd.join(".xdcode");
    let writable_workspace_xdcode_home =
        AbsolutePathBuf::from_absolute_path(&workspace_xdcode_home)
            .expect("absolute workspace metadata directory");
    let policy = FileSystemSandboxPolicy::workspace_write(
        &[writable_workspace_xdcode_home],
        /*exclude_tmpdir_env_var*/ false,
        /*exclude_slash_tmp*/ false,
    );

    assert!(policy.can_write_local_path_with_cwd(&xdcode_home, &cwd));
    assert!(policy.can_write_local_path_with_cwd(&workspace_xdcode_home, &cwd));
    assert!(
        editor_directory(&[&xdcode_home, &workspace_xdcode_home], &policy, &cwd).is_err(),
        "explicitly writable metadata must not be used for editor buffers"
    );
    assert!(!workspace_xdcode_home.exists());
}

#[test]
#[cfg(unix)]
fn editor_directory_rejects_workspace_fallback_symlink_to_writable_target() {
    use std::os::unix::fs::symlink;

    let paths = EditorPaths::new();
    let workspace_xdcode_home = paths.cwd.join(".xdcode");
    symlink(&paths.xdcode_home, &workspace_xdcode_home).expect("create workspace metadata symlink");
    let policy = workspace_write_policy(&[&paths.xdcode_home]);

    assert!(!policy.can_write_local_path_with_cwd(&workspace_xdcode_home, &paths.cwd));
    assert!(policy.can_write_local_path_with_cwd(&paths.xdcode_home, &paths.cwd));
    assert!(
        editor_directory(
            &[&paths.xdcode_home, &workspace_xdcode_home],
            &policy,
            &paths.cwd,
        )
        .is_err()
    );
}

#[test]
fn editor_directory_uses_next_protected_candidate_after_creation_error() {
    let paths = EditorPaths::new();
    let unavailable_home = paths
        .xdcode_home
        .parent()
        .expect("Xdcode home parent")
        .join("unavailable-home");
    fs::write(&unavailable_home, "not a directory").expect("create unavailable Xdcode home");
    let policy = workspace_write_policy(&[]);

    let directory = editor_directory(
        &[&unavailable_home, &paths.xdcode_home],
        &policy,
        &paths.cwd,
    )
    .expect("use next protected candidate after directory creation fails");

    assert_eq!(directory, paths.xdcode_home.join("editor"));
}

#[test]
#[cfg(windows)]
fn editor_directory_rejects_windows_temporary_directory_outside_tmpdir_policy_root() {
    let paths = EditorPaths::new();
    let policy = FileSystemSandboxPolicy::workspace_write(
        &[],
        /*exclude_tmpdir_env_var*/ false,
        /*exclude_slash_tmp*/ true,
    );

    assert!(
        editor_directory(&[&paths.xdcode_home], &policy, &paths.cwd).is_err(),
        "effective Windows temporary directories must not contain editor buffers"
    );
    assert!(!paths.xdcode_home.join("editor").exists());
}

#[test]
fn editor_directory_allows_full_disk_write_policies() {
    let paths = EditorPaths::new();

    for policy in [
        FileSystemSandboxPolicy::unrestricted(),
        FileSystemSandboxPolicy::external_sandbox(),
    ] {
        let directory = editor_directory(&[&paths.xdcode_home], &policy, &paths.cwd)
            .expect("full-disk-write policies should preserve external editor support");

        assert_eq!(directory, paths.xdcode_home.join("editor"));
    }
}

#[tokio::test]
#[cfg(unix)]
async fn editor_process_receives_buffer_in_isolated_xdcode_home() {
    let paths = EditorPaths::new();
    let policy = workspace_write_policy(&[]);
    let editor_directory = paths.xdcode_home.join("editor");
    let editor_command = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "case \"$2\" in \"$1\"/*) printf edited > \"$2\" ;; *) exit 88 ;; esac".to_string(),
        "editor".to_string(),
        editor_directory.to_string_lossy().into_owned(),
    ];

    let content = run_editor(
        "seed",
        &editor_command,
        &paths.xdcode_home,
        &policy,
        &paths.cwd,
    )
    .await
    .expect("run editor with isolated buffer");

    assert_eq!(content, "edited");
}

#[tokio::test]
#[cfg(any(target_os = "macos", target_os = "linux"))]
async fn editor_process_uses_protected_workspace_fallback_with_default_temporary_grants() {
    let root = tempfile::tempdir().expect("create editor test root");
    let xdcode_home = root.path().join("xdcode-home");
    let cwd = root.path().join("workspace");
    fs::create_dir(&xdcode_home).expect("create Xdcode home");
    fs::create_dir(&cwd).expect("create workspace");
    let default_xdcode_home = dirs::home_dir().expect("home directory").join(".xdcode");
    let writable_default_xdcode_home = AbsolutePathBuf::from_absolute_path(&default_xdcode_home)
        .expect("absolute default Xdcode home");
    let policy = FileSystemSandboxPolicy::workspace_write(
        &[writable_default_xdcode_home],
        /*exclude_tmpdir_env_var*/ false,
        /*exclude_slash_tmp*/ false,
    );
    let workspace_xdcode_home = cwd.join(".xdcode");
    let editor_directory = dunce::canonicalize(&cwd)
        .expect("canonicalize workspace")
        .join(".xdcode")
        .join("editor");
    let editor_command = vec![
        "/bin/sh".to_string(),
        "-c".to_string(),
        "case \"$2\" in \"$1\"/*) printf edited > \"$2\" ;; *) exit 88 ;; esac".to_string(),
        "editor".to_string(),
        editor_directory.to_string_lossy().into_owned(),
    ];

    assert!(!workspace_xdcode_home.exists());
    assert!(policy.can_write_local_path_with_cwd(&xdcode_home, &cwd));
    assert!(policy.can_write_local_path_with_cwd(&default_xdcode_home, &cwd));
    assert!(!policy.can_write_local_path_with_cwd(&workspace_xdcode_home, &cwd));

    let content = run_editor("seed", &editor_command, &xdcode_home, &policy, &cwd)
        .await
        .expect("run editor with protected workspace fallback");

    assert_eq!(content, "edited");
    assert!(editor_directory.is_dir());
}
