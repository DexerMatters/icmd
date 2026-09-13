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
    for excluded in ["agents/", ".scratch/", "examples/res/"] {
        assert!(
            excludes.contains(excluded),
            "`{excluded}` must be excluded from the published package"
        );
    }
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
