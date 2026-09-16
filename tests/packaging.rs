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
        "cargo test --no-default-features --features markdown",
        "cargo test --all-features",
        "icmd-high-level-fixture",
        "cargo audit",
        "cargo package",
    ] {
        assert!(ci.contains(gate), "CI must run `{gate}`");
    }
    // The documentation binary needs Chafa, pkg-config, and libclang, so its
    // tests belong in the job that installs them, not in the pure-Rust job.
    for gate in [
        "cargo test -p cargo-icmd",
        "cargo clippy -p cargo-icmd --all-targets --no-deps -- -D warnings",
        "cargo package -p cargo-icmd --list",
    ] {
        assert!(ci.contains(gate), "CI must run `{gate}`");
    }
    let native_job = ci
        .split("native-raster:")
        .nth(1)
        .expect("the native raster job must exist");
    assert!(
        native_job.contains("libchafa-dev pkg-config clang"),
        "the documentation tool's native prerequisites must be installed"
    );
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

// The documentation tool ships its own raster asset and its displayed snippets.
// Both are release artifacts: the asset must exist with a license note, and the
// runnable snippets must be compiled by a test so visible code cannot drift.
#[test]
fn documentation_tool_packages_its_assets_and_compiles_its_snippets() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let tool = root.join("tools/cargo-icmd");
    let manifest = std::fs::read_to_string(tool.join("Cargo.toml"))
        .expect("the documentation tool manifest must exist");
    for required in [
        "name = \"cargo-icmd\"",
        "path = \"src/main.rs\"",
        "native-raster",
        "markdown",
    ] {
        assert!(
            manifest.contains(required),
            "the tool manifest must declare `{required}`"
        );
    }

    assert!(
        tool.join("assets/guide.png").exists(),
        "the bundled raster asset must ship in the tool package"
    );
    let asset_note = std::fs::read_to_string(tool.join("assets/README.md"))
        .expect("the asset needs a source and license note");
    for required in ["Source:", "License:", "MIT OR Apache-2.0"] {
        assert!(
            asset_note.contains(required),
            "the asset note must document `{required}`"
        );
    }

    let mut snippets = 0;
    for entry in std::fs::read_dir(tool.join("snippets")).expect("snippets/ must exist") {
        let path = entry.expect("readable entry").path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            snippets += 1;
        }
    }
    assert!(
        snippets >= 10,
        "the guide must ship real Rust snippets, found only {snippets}"
    );
    let compile_gate = std::fs::read_to_string(tool.join("src/snippets.rs"))
        .expect("the snippet registry must exist");
    assert!(
        compile_gate.contains("include_str!"),
        "displayed source must be included verbatim from snippet files"
    );
    assert!(
        tool.join("src/snippets_compile.rs").exists(),
        "every displayed snippet must be compiled by a test"
    );
}

// API acceptance checklist: the fixture set covers every documented tier and
// feature combination, and each is a real workspace member.
#[test]
fn downstream_fixtures_cover_high_level_and_advanced_tiers() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace = std::fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml exists");
    for member in ["fixtures/high-level", "fixtures/advanced"] {
        assert!(
            workspace.contains(member),
            "the workspace must include the `{member}` fixture"
        );
    }
    let high = std::fs::read_to_string(root.join("fixtures/high-level/src/main.rs"))
        .expect("the high-level fixture exists");
    assert!(
        high.contains("icmd::ui!"),
        "the high-level fixture must exercise macro expansion"
    );
    let high_manifest =
        std::fs::read_to_string(root.join("fixtures/high-level/Cargo.toml")).expect("manifest");
    assert!(
        high_manifest.contains("default-features = false"),
        "the high-level fixture must cover the feature-off build"
    );
    let advanced = std::fs::read_to_string(root.join("fixtures/advanced/src/main.rs"))
        .expect("the advanced fixture exists");
    assert!(
        advanced.contains("icmd::advanced::"),
        "the advanced fixture must exercise the advanced tier"
    );
    let advanced_manifest =
        std::fs::read_to_string(root.join("fixtures/advanced/Cargo.toml")).expect("manifest");
    assert!(
        !advanced_manifest.contains("default-features = false"),
        "the advanced fixture must cover the native-feature build"
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

// Product-safe condition #9: a security contact and process must be published.
#[test]
fn security_policy_is_present_and_actionable() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let policy = std::fs::read_to_string(root.join("SECURITY.md"))
        .expect("a security policy must be published");
    for required in [
        "Reporting a vulnerability",
        "Supported versions",
        "Scope",
        "acknowledgement",
    ] {
        assert!(
            policy.contains(required),
            "the security policy must document `{required}`"
        );
    }
    // Reporting must be private: publishing a vulnerability in an issue is the
    // failure mode this section exists to prevent.
    assert!(
        policy.contains("Do not open a public issue"),
        "the policy must tell reporters not to file a public issue"
    );
}

// Rustdoc is the crate's documentation channel: module headers use `//!` and
// items use `///`. Plain `//` comments are not carried, so this gate both
// requires rustdoc and forbids the comment noise it replaced.
#[test]
fn crate_source_documents_its_public_api_with_rustdoc() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = 0;
    let mut rustdoc = 0;
    let mut stack = vec![root];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("src is readable") {
            let path = entry.expect("readable entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("source is readable");
            for (number, line) in source.lines().enumerate() {
                let trimmed = line.trim_start();
                if trimmed.starts_with("///") || trimmed.starts_with("//!") {
                    rustdoc += 1;
                } else {
                    assert!(
                        !trimmed.starts_with("//"),
                        "{}:{} is a plain `//` comment; documentation uses `///` or `//!`: {line}",
                        path.display(),
                        number + 1
                    );
                }
            }
            files += 1;
        }
    }
    assert!(files > 0, "the crate source must have been inspected");
    assert!(
        rustdoc > 200,
        "the public API must be documented with `///` and `//!`, found only {rustdoc} lines"
    );
}
