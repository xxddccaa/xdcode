use std::ffi::OsStr;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;

use semver::Version;
use serde::Deserialize;
use xdcode_utils_absolute_path::AbsolutePathBuf;

const BIN_DIRNAME: &str = "bin";
const CODE_MODE_HOST_EXECUTABLE_NAME: &str = if cfg!(windows) {
    "xdcode-code-mode-host.exe"
} else {
    "xdcode-code-mode-host"
};
const PACKAGE_METADATA_FILENAME: &str = "xdcode-package.json";
const PATH_DIRNAME: &str = "xdcode-path";
const RELEASES_DIRNAME: &str = "releases";
const RESOURCES_DIRNAME: &str = "xdcode-resources";
const STANDALONE_PACKAGES_DIRNAME: &str = "standalone";
static INSTALL_CONTEXT: OnceLock<InstallContext> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StandalonePlatform {
    Unix,
    Windows,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct XdcodePackageLayout {
    /// The package root that contains the metadata file and layout directories.
    pub package_dir: AbsolutePathBuf,
    /// Directory containing the Xdcode entrypoint executable.
    pub bin_dir: AbsolutePathBuf,
    /// Directory containing managed helper binaries and data files, when present.
    pub resources_dir: Option<AbsolutePathBuf>,
    /// Folder that should be prepended to the PATH, when present.
    pub path_dir: Option<AbsolutePathBuf>,
}

/// Version metadata recorded in a bundled Xdcode runtime package.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct XdcodePackageManifest {
    pub version: Version,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallContext {
    pub method: InstallMethod,
    pub package_layout: Option<XdcodePackageLayout>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InstallMethod {
    Standalone {
        /// The managed standalone release directory. Legacy installs use paths
        /// such as
        /// `~/.xdcode/packages/standalone/releases/0.111.0-x86_64-unknown-linux-musl`.
        /// Package-layout installs use the package root that contains `bin/`,
        /// `xdcode-resources/`, and `xdcode-path/`.
        release_dir: AbsolutePathBuf,
        /// The bundled resource directory for managed dependencies.
        resources_dir: Option<AbsolutePathBuf>,
        /// The platform of the standalone release, either `Unix` or `Windows`.
        platform: StandalonePlatform,
    },
    /// A Xdcode binary launched through the npm-managed `xdcode.js` shim.
    Npm,
    /// A Xdcode binary launched through the bun-managed `xdcode.js` shim.
    Bun,
    /// A Xdcode binary launched through the pnpm-managed `xdcode.js` shim.
    Pnpm,
    /// A Xdcode binary launched through the Vite+-managed `xdcode.js` shim.
    VitePlus,
    /// A Xdcode binary that appears to come from a Homebrew install prefix.
    Brew,
    /// Any other execution environment.
    ///
    /// This commonly covers `cargo run`, app-bundled Xdcode binaries, custom
    /// internal launchers, and tests that execute Xdcode from an arbitrary path.
    Other,
}

impl InstallContext {
    pub fn from_exe(
        is_macos: bool,
        current_exe: Option<&Path>,
        method_override: Option<InstallMethod>,
    ) -> Self {
        let xdcode_home = xdcode_utils_home_dir::find_xdcode_home().ok();
        Self::from_exe_with_xdcode_home(
            is_macos,
            current_exe,
            method_override,
            xdcode_home.as_deref(),
        )
    }

    fn from_exe_with_xdcode_home(
        is_macos: bool,
        current_exe: Option<&Path>,
        method_override: Option<InstallMethod>,
        xdcode_home: Option<&Path>,
    ) -> Self {
        let package_layout = current_exe.and_then(XdcodePackageLayout::from_exe);
        let method = if let Some(method) = method_override {
            method
        } else if let Some(exe_path) = current_exe {
            install_method_from_exe(exe_path, xdcode_home, package_layout.as_ref(), is_macos)
        } else {
            InstallMethod::Other
        };

        Self {
            method,
            package_layout,
        }
    }

    pub fn current() -> &'static Self {
        INSTALL_CONTEXT.get_or_init(|| {
            let current_exe = std::env::current_exe().ok();
            let method_override = if std::env::var_os("XDCODE_MANAGED_BY_VITE_PLUS").is_some() {
                Some(InstallMethod::VitePlus)
            } else if std::env::var_os("XDCODE_MANAGED_BY_PNPM").is_some() {
                Some(InstallMethod::Pnpm)
            } else if std::env::var_os("XDCODE_MANAGED_BY_NPM").is_some() {
                Some(InstallMethod::Npm)
            } else if std::env::var_os("XDCODE_MANAGED_BY_BUN").is_some() {
                Some(InstallMethod::Bun)
            } else {
                None
            };
            Self::from_exe(
                cfg!(target_os = "macos"),
                current_exe.as_deref(),
                method_override,
            )
        })
    }

    /// Read the manifest for the package that contains the current executable.
    pub fn package_manifest(&self) -> Option<XdcodePackageManifest> {
        let package_layout = self.package_layout.as_ref()?;
        let manifest =
            std::fs::read_to_string(package_layout.package_dir.join(PACKAGE_METADATA_FILENAME))
                .ok()?;
        serde_json::from_str(&manifest).ok()
    }

    pub fn rg_command(&self) -> PathBuf {
        if let Some(package_layout) = &self.package_layout
            && let Some(path_dir) = &package_layout.path_dir
        {
            let bundled_rg = path_dir.join(default_rg_command());
            if bundled_rg.is_file() {
                return bundled_rg.into_path_buf();
            }
        }

        if let InstallMethod::Standalone {
            resources_dir: Some(resources_dir),
            ..
        } = &self.method
        {
            let bundled_rg = resources_dir.join(default_rg_command());
            if bundled_rg.is_file() {
                return bundled_rg.into_path_buf();
            }
        }

        default_rg_command()
    }

    pub fn code_mode_host_program(&self) -> PathBuf {
        // prefer the one packed under xdcode-resources
        self.bundled_resource(CODE_MODE_HOST_EXECUTABLE_NAME)
            .map_or_else(
                || self.code_mode_host_program_from_exe(std::env::current_exe().ok().as_deref()),
                AbsolutePathBuf::into_path_buf,
            )
    }

    fn code_mode_host_program_from_exe(&self, current_exe: Option<&Path>) -> PathBuf {
        let executable_dir = if let Some(package_layout) = &self.package_layout {
            Some(package_layout.bin_dir.clone())
        } else if let InstallMethod::Standalone { release_dir, .. } = &self.method {
            Some(release_dir.clone())
        } else {
            current_exe
                .and_then(Path::parent)
                .and_then(canonical_absolute_path)
        };
        if let Some(executable_dir) = executable_dir {
            let executable = executable_dir.join(CODE_MODE_HOST_EXECUTABLE_NAME);
            if executable.is_file() {
                return executable.into_path_buf();
            }
        }

        current_exe
            .and_then(Path::parent)
            .map(|parent| parent.join(CODE_MODE_HOST_EXECUTABLE_NAME))
            .unwrap_or_else(|| PathBuf::from(CODE_MODE_HOST_EXECUTABLE_NAME))
    }

    pub fn bundled_resource(&self, file_name: impl AsRef<Path>) -> Option<AbsolutePathBuf> {
        if let Some(package_layout) = &self.package_layout
            && let Some(resources_dir) = &package_layout.resources_dir
        {
            let resource = resources_dir.join(file_name.as_ref());
            if resource.is_file() {
                return Some(resource);
            }
        }

        if let InstallMethod::Standalone {
            resources_dir: Some(resources_dir),
            ..
        } = &self.method
        {
            let resource = resources_dir.join(file_name);
            if resource.is_file() {
                return Some(resource);
            }
        }

        None
    }
}

impl XdcodePackageLayout {
    fn from_exe(exe_path: &Path) -> Option<Self> {
        let canonical_exe = canonical_absolute_path(exe_path)?;
        let exe_dir = canonical_exe.parent()?;
        // WinGet preserves a target-qualified executable at the package root.
        // Only recognize that layout when metadata names this exact executable.
        #[cfg(windows)]
        if let Ok(contents) = std::fs::read(exe_dir.join(PACKAGE_METADATA_FILENAME))
            && let Ok(metadata) = serde_json::from_slice::<serde_json::Value>(&contents)
            && metadata["layoutVersion"] == 1
            && metadata["entrypoint"].as_str().map(OsStr::new) == canonical_exe.file_name()
        {
            return Some(Self {
                resources_dir: existing_dir(exe_dir.join(RESOURCES_DIRNAME)),
                path_dir: existing_dir(exe_dir.join(PATH_DIRNAME)),
                package_dir: exe_dir.clone(),
                bin_dir: exe_dir,
            });
        }
        match exe_dir.file_name() {
            Some(name) if name == OsStr::new(BIN_DIRNAME) => Self::from_package_bin_dir(exe_dir),
            Some(name) if name == OsStr::new(RESOURCES_DIRNAME) => {
                let package_dir = exe_dir.parent()?;
                Self::from_package_bin_dir(package_dir.join(BIN_DIRNAME))
            }
            Some(name) if name == OsStr::new("MacOS") => {
                // A provisioned CLI keeps helpers and metadata in the outer
                // package. current_exe points inside the bundle, not at bin/xdcode.
                let contents = exe_dir.parent()?;
                let bundle = contents.parent()?;
                if contents.file_name()? != OsStr::new("Contents")
                    || bundle.file_name()? != OsStr::new("XdcodeCLI.app")
                {
                    return None;
                }
                Self::from_package_bin_dir(bundle.parent()?.join(BIN_DIRNAME))
            }
            Some(_) | None => None,
        }
    }

    fn from_package_bin_dir(bin_dir: AbsolutePathBuf) -> Option<Self> {
        if !bin_dir.is_dir() {
            return None;
        }
        let package_dir = bin_dir.parent()?;
        if !package_dir.join(PACKAGE_METADATA_FILENAME).is_file() {
            return None;
        }

        Some(Self {
            resources_dir: existing_dir(package_dir.join(RESOURCES_DIRNAME)),
            path_dir: existing_dir(package_dir.join(PATH_DIRNAME)),
            package_dir,
            bin_dir,
        })
    }
}

fn install_method_from_exe(
    exe_path: &Path,
    xdcode_home: Option<&Path>,
    package_layout: Option<&XdcodePackageLayout>,
    is_macos: bool,
) -> InstallMethod {
    if let Some(standalone_method) =
        standalone_install_method(exe_path, xdcode_home, package_layout)
    {
        return standalone_method;
    }

    if is_macos && (exe_path.starts_with("/opt/homebrew") || exe_path.starts_with("/usr/local")) {
        InstallMethod::Brew
    } else {
        InstallMethod::Other
    }
}

fn standalone_install_method(
    exe_path: &Path,
    xdcode_home: Option<&Path>,
    package_layout: Option<&XdcodePackageLayout>,
) -> Option<InstallMethod> {
    let canonical_xdcode_home = canonical_absolute_path(xdcode_home?)?;
    let release_dir = if let Some(package_layout) = package_layout {
        package_layout.package_dir.clone()
    } else {
        canonical_absolute_path(exe_path)?.parent()?
    };
    let releases_root = canonical_xdcode_home
        .join("packages")
        .join(STANDALONE_PACKAGES_DIRNAME)
        .join(RELEASES_DIRNAME);
    if !release_dir.starts_with(releases_root.as_path()) {
        return None;
    }

    let resources_dir = release_dir.join(RESOURCES_DIRNAME);
    Some(InstallMethod::Standalone {
        release_dir,
        resources_dir: resources_dir.is_dir().then_some(resources_dir),
        platform: standalone_platform(),
    })
}

fn canonical_absolute_path(path: &Path) -> Option<AbsolutePathBuf> {
    let canonical_path = std::fs::canonicalize(path).ok()?;
    AbsolutePathBuf::from_absolute_path(canonical_path).ok()
}

fn standalone_platform() -> StandalonePlatform {
    if cfg!(windows) {
        StandalonePlatform::Windows
    } else {
        StandalonePlatform::Unix
    }
}

fn existing_dir(path: AbsolutePathBuf) -> Option<AbsolutePathBuf> {
    path.is_dir().then_some(path)
}

fn default_rg_command() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from("rg.exe")
    } else {
        PathBuf::from("rg")
    }
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
