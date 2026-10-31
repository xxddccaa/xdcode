// Compile the actual build.rs without Cargo. Only C compilation/pkg-config are
// substituted, so tests observe its real resolved-source tracking directives.
mod pkg_config {
    use std::path::PathBuf;
    pub struct Config;
    pub struct Library {
        pub include_paths: Vec<PathBuf>,
        pub link_paths: Vec<PathBuf>,
        pub libs: Vec<String>,
    }
    impl Config {
        pub fn new() -> Self {
            Self
        }
        pub fn cargo_metadata(self, _: bool) -> Self {
            self
        }
        pub fn probe(self, _: &str) -> Result<Library, String> {
            Ok(Library {
                include_paths: vec![],
                link_paths: vec![],
                libs: vec![],
            })
        }
    }
}

mod cc {
    pub struct Build;
    impl Build {
        pub fn new() -> Self {
            Self
        }
        pub fn file(&mut self, _: std::path::PathBuf) -> &mut Self {
            self
        }
        pub fn include(&mut self, _: &std::path::Path) -> &mut Self {
            self
        }
        pub fn define(&mut self, _: &str, _: Option<&str>) -> &mut Self {
            self
        }
        pub fn flag(&mut self, _: String) -> &mut Self {
            self
        }
        pub fn compile(&mut self, _: &str) {}
    }
}

mod subject {
    use crate::{cc, pkg_config};
    include!("../../../xdcode-rs/bwrap/build.rs");
    pub fn run() {
        main();
    }
}

fn main() {
    subject::run();
}
