# winit vendor baseline: storytold/winit `a517b384` (winit 0.30.13 + the craft suite patches)

The source is the craft suite's winit fork, <https://github.com/storytold/winit> at revision
`a517b3846ee8457d84ee4975110bfda6797c44dc` (2026-10-24, "Preserve input order across requested
IME resets"): the published winit 0.30.13 (tag `v0.30.13`, `e9809ef5`) plus the suite's patches
W1–W27, the integrated Wayland clipboard and the ordered IME reset, listed in
`vendor/winit/PATCHES.md` (copied unchanged from the fork). It is the revision upstream WordCraft
pins in `[patch.crates-io]`; WordCraft-ja vendors it instead so its own macOS input-method patch
(`01-macos-ime-surrounding-text.md`) can sit on top. Among the fork's patches, the ones this patch
builds on: W6 (`NSTextInputClient` commit flow: `pending_commit`), W7 (marked text discarded when
IME is disabled), W8 (emoji-picker text committed), W22 (a caret for input methods, `ImeDocument`,
replacement ranges next to it, text typed without a composition committed — `plain_insert`), W24
(`Window::reset_ime`) and W26 (`WindowExtMacOS::take_ime_delete_surrounding`, which egui-winit
calls with each `Ime::Commit`).

The files were made with `cargo package -p winit --no-verify` in a checkout of that revision (the
packaged archive's SHA-256 is `53136e4f04264721d11d18753f507df9a25ee737f7d401cbe88b70bfcc89c7fa`;
`cargo package` normalises the manifest, so the workspace-inherited fields are written out).
`src/`, `Cargo.toml`, `LICENSE`, `README.md`, `build.rs` and `.cargo_vcs_info.json` (which records
the revision) are copied unchanged from the package, `PATCHES.md` from the repository root; the
crate's examples, tests and docs are not. The crate is Apache-2.0 licensed.

The baseline carries no behavioural change of WordCraft's. Every change is one of the numbered
patch files that follow, each marked `WordCraft patch` in the source.

## History

- 2026-10-19: winit 0.30.13 from crates.io (archive SHA-256
  `a6755fa58a9f8350bd1e472d4c3fcc25f824ec358933bba33306d0b63df5978d`).
- 2026-11-01: re-based on storytold/winit `a517b384` when merging upstream WordCraft, which had
  moved to that fork; patch 01 re-applied on the fork's macOS view (see there).
