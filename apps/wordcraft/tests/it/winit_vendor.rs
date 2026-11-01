//! The build takes winit from `vendor/winit` (`vendor/README.md`): storytold/winit (winit 0.30.13
//! with the craft suite's patches) plus WordCraft's own. A re-vendored or upgraded copy without
//! the patches would silently bring the platform bugs back.

#[test]
fn winit_resolves_to_the_vendored_copy() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap();
    let winit = lock.split("[[package]]").find(|p| p.contains("\nname = \"winit\"\n")).expect("winit is in Cargo.lock");
    assert!(winit.contains("\nversion = \"0.30.13\"\n"), "the vendored copy is 0.30.13: re-vendor and re-apply the patches");
    assert!(!winit.contains("\nsource = "), "winit must resolve to vendor/winit, not a registry or git");

    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(manifest.contains("winit = { path = \"vendor/winit\" }"), "the workspace no longer patches crates.io winit to vendor/winit");
    assert!(manifest.contains("exclude = [\"vendor/winit\"]"), "vendor/winit must stay out of the workspace (its own lints)");

    // The base is storytold/winit at a recorded revision, its patch list kept beside the source.
    const FORK_REV: &str = "a517b3846ee8457d84ee4975110bfda6797c44dc";
    let baseline = std::fs::read_to_string(root.join("vendor/patches/winit/00-vendor-baseline.md")).unwrap();
    assert!(baseline.contains(FORK_REV), "the baseline names the storytold/winit revision");
    let vcs = std::fs::read_to_string(root.join("vendor/winit/.cargo_vcs_info.json")).unwrap();
    assert!(vcs.contains(FORK_REV), "the vendored copy's upstream revision");
    let patches = std::fs::read_to_string(root.join("vendor/winit/PATCHES.md")).unwrap();
    assert!(patches.contains("W22") && patches.contains("W26"), "the craft suite's patch list (macOS IME W6–W10, W22, W24, W26)");
    let delta = std::fs::read_to_string(root.join("vendor/patches/winit/source-delta.json")).unwrap();
    assert!(delta.contains(FORK_REV), "source-delta.json is against the same revision");
}
