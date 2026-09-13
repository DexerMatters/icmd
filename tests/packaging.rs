// Packaging metadata is a release gate: the crate must declare its own license,
// ownership, and a content allowlist so planning documents, local scratch
// state, and demo media never ship in a published package.
#[test]
fn manifest_declares_release_metadata_and_content_allowlist() {
    let manifest = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
    )
    .expect("the crate manifest must be readable");

    for required in [
        "license = \"MIT OR Apache-2.0\"",
        "description =",
        "repository =",
        "readme = \"README.md\"",
        "rust-version =",
    ] {
        assert!(
            manifest.contains(required),
            "the manifest must declare `{required}` for a published release"
        );
    }

    let excludes = manifest
        .split("exclude = [")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("the manifest must declare a package exclude list");
    for excluded in ["agents/", ".scratch/", "examples/res/", "fixtures/"] {
        assert!(
            excludes.contains(excluded),
            "`{excluded}` must be excluded from the published package"
        );
    }
}

// The release checklist requires CI to cover the pure-Rust build, the native
// feature, the MSRV, a downstream fixture, and scheduled hardening runs.
#[test]
fn ci_covers_the_release_gates() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let ci = std::fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .expect("the CI workflow must exist");
    for gate in [
        "cargo test --no-default-features",
        "cargo test --all-features",
        "icmd-high-level-fixture",
        "cargo audit",
        "cargo package",
    ] {
        assert!(ci.contains(gate), "CI must run `{gate}`");
    }
    let stress = std::fs::read_to_string(root.join(".github/workflows/stress.yml"))
        .expect("the scheduled stress workflow must exist");
    assert!(
        stress.contains("cargo miri test"),
        "Miri must run on a schedule"
    );
    assert!(
        stress.contains("--test property"),
        "the property corpus must replay on a schedule"
    );
}

#[test]
fn license_files_are_present() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for file in ["LICENSE-MIT", "LICENSE-APACHE", "README.md"] {
        assert!(
            root.join(file).exists(),
            "`{file}` must exist for the declared license and readme"
        );
    }
}
