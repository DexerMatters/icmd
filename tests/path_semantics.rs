// SAF-07: file loading must preserve the operating system's path resolution.
// A lexical fold of `link/..` can select a different file than the kernel does
// after traversing the link, so the loader must open the caller's exact path.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};

use icmd::{ImageSource, RasterImage};

fn write_png(path: &Path, red: u8, green: u8, width: u32, height: u32) {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..(width * height) {
        pixels.extend_from_slice(&[red, green, 0, 255]);
    }
    let file = fs::File::create(path).expect("create png");
    let encoder = image::codecs::png::PngEncoder::new(file);
    image::ImageEncoder::write_image(
        encoder,
        &pixels,
        width,
        height,
        image::ExtendedColorType::Rgba8,
    )
    .expect("encode png");
}

fn temp_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("icmd-path-{tag}-{}-{unique}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

// Layout:
//   base/a/target.png          (lexical resolution: 3x1)
//   base/a/link -> ../b/inner
//   base/b/target.png          (kernel resolution: 2x1)
//   base/b/inner/target.png    (link target: 4x1)
// `a/link/../target.png` must resolve through the link to `b/target.png`.
fn symlink_topology() -> (PathBuf, PathBuf) {
    let base = temp_dir("symlink");
    fs::create_dir_all(base.join("a")).unwrap();
    fs::create_dir_all(base.join("b/inner")).unwrap();
    write_png(&base.join("a/target.png"), 255, 0, 3, 1);
    write_png(&base.join("b/target.png"), 0, 255, 2, 1);
    write_png(&base.join("b/inner/target.png"), 0, 0, 4, 1);
    std::os::unix::fs::symlink("../b/inner", base.join("a/link")).unwrap();
    let path = base.join("a/link/../target.png");
    (base, path)
}

#[test]
fn open_resolves_dot_dot_after_traversing_a_symlink() {
    let (base, path) = symlink_topology();
    let image = RasterImage::open(&path).expect("open through the symlink");
    assert_eq!(
        (image.width(), image.height()),
        (2, 1),
        "the kernel resolves the link before `..`; a lexical fold would pick a/target.png"
    );
    let _ = fs::remove_dir_all(base);
}

#[test]
fn image_source_preserves_the_caller_path_and_resolves_it_correctly() {
    let (base, path) = symlink_topology();
    let source = ImageSource::file(&path);
    assert_eq!(
        source.path(),
        Some(path.as_path()),
        "the caller's exact path must be retained, not lexically rewritten"
    );
    let image = icmd::RasterImage::open(source.path().unwrap()).expect("open");
    assert_eq!((image.width(), image.height()), (2, 1));
    let _ = fs::remove_dir_all(base);
}

#[test]
fn errors_name_the_original_path() {
    let missing = std::env::temp_dir().join("icmd-path-missing-does-not-exist.png");
    let error = RasterImage::open(&missing).expect_err("missing file must fail");
    let message = error.to_string();
    assert!(
        message.contains("icmd-path-missing-does-not-exist.png"),
        "the error must preserve the caller's path, got: {message}"
    );
}

#[test]
fn relative_and_absolute_paths_load_the_same_file() {
    let base = temp_dir("relative");
    write_png(&base.join("one.png"), 10, 20, 5, 1);
    let absolute = base.join("one.png");
    let cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&base).unwrap();
    let relative = RasterImage::open("one.png").expect("relative open");
    std::env::set_current_dir(cwd).unwrap();
    assert_eq!(
        (relative.width(), relative.height()),
        (5, 1),
        "a relative path must open the same file as its absolute form"
    );
    let absolute = RasterImage::open(&absolute).expect("absolute open");
    assert_eq!((absolute.width(), absolute.height()), (5, 1));
    let _ = fs::remove_dir_all(base);
}
