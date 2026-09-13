// Ensures the native raster feature fails with an actionable message and never
// silently links an untested Chafa ABI. The build script only inspects the
// version; chafa-sys performs the actual linking.
const MINIMUM_CHAFA: (u32, u32, u32) = (1, 12, 0);

fn parse(version: &str) -> Option<(u32, u32, u32)> {
    let mut parts = version.trim().split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts
        .next()
        .and_then(|value| {
            let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .unwrap_or(0);
    Some((major, minor, patch))
}

fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_NATIVE_RASTER");
    if std::env::var_os("CARGO_FEATURE_NATIVE_RASTER").is_none() {
        return;
    }
    let output = std::process::Command::new("pkg-config")
        .args(["--modversion", "chafa"])
        .output();
    let Ok(output) = output else {
        // The dependency's own build script reports a hard error; a warning
        // here keeps discovery strategies that do not use pkg-config working.
        println!(
            "cargo:warning=the `native-raster` feature could not run pkg-config; \
             the Chafa development package (>= {}.{}.{}) is required",
            MINIMUM_CHAFA.0, MINIMUM_CHAFA.1, MINIMUM_CHAFA.2
        );
        return;
    };
    if !output.status.success() {
        println!(
            "cargo:warning=the `native-raster` feature requires the Chafa development \
             package (>= {}.{}.{}); build with --no-default-features for a pure-Rust build",
            MINIMUM_CHAFA.0, MINIMUM_CHAFA.1, MINIMUM_CHAFA.2
        );
        return;
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    match parse(&version) {
        Some(found) if found >= MINIMUM_CHAFA => {}
        Some(found) => panic!(
            "Chafa {}.{}.{} is too old for the `native-raster` feature; {}.{}.{} or newer \
             is required, or build with --no-default-features",
            found.0, found.1, found.2, MINIMUM_CHAFA.0, MINIMUM_CHAFA.1, MINIMUM_CHAFA.2
        ),
        None => println!("cargo:warning=unrecognized Chafa version `{version}`"),
    }
}
