#![allow(clippy::expect_used)]

use std::process::Command;

use divan::Bencher;

fn main() {
    divan::main();
}

/// Exercises the Bazel-backed end-to-end benchmark path with a cheap,
/// deterministic Xdcode invocation. Richer scenarios can add separate
/// benchmark binaries without making the shared harness depend on them.
#[divan::bench(sample_count = 20, sample_size = 1)]
fn xdcode_help(bencher: Bencher) {
    let xdcode = xdcode_utils_cargo_bin::cargo_bin("xdcode")
        .expect("xdcode binary should be available through Bazel runfiles");

    bencher.bench_local(move || {
        let output = Command::new(&xdcode)
            .arg("--help")
            .output()
            .expect("xdcode --help should run");
        assert!(output.status.success(), "xdcode --help should succeed");
    });
}
