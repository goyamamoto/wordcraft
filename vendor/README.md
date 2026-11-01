# Vendored dependencies

Third-party crates carried in the tree with a narrow local patch, so a platform fix can ship
before (or without) an upstream release. Each crate keeps its upstream licence and provenance in
its own directory; `patches/<crate>/` says what was changed and why, one file per change, with
the regression test that guards it.

| Crate | Version | Base | Licence | Local patches | Guard |
|---|---|---|---|---|---|
| winit | 0.30.13 | storytold/winit `a517b384` (`patches/winit/00-vendor-baseline.md`, `winit/PATCHES.md`) | Apache-2.0 | `patches/winit/01-macos-ime-surrounding-text.md` | `apps/wordcraft/tests/it/winit_vendor.rs`, `apps/wordcraft/tests/it/winit_macos_ime_surrounding_text.rs` |

The workspace manifest patches crates.io to `vendor/winit` (`[patch.crates-io]`), so every
crate in the build that depends on winit (eframe, egui-winit, accesskit_winit…) takes this copy;
`Cargo.lock` records it without a `source`. The directory is excluded from the workspace
(`exclude`), so it keeps its own lints and formatting and `cargo fmt`/`clippy --workspace`
leave it alone.

Upstream WordCraft takes winit from the craft suite's fork (storytold/winit) at a pinned
revision; WordCraft-ja vendors that same revision, so its macOS input-method patch can sit on
top. Re-vendoring (when upstream moves its pin): check out the fork at the new revision, run
`cargo package -p winit --no-verify` there, copy `src/`, `Cargo.toml`, `LICENSE`, `README.md`,
`build.rs` and `.cargo_vcs_info.json` from the package (`target/package/winit-<version>/`;
examples, tests and docs are left out) and `PATCHES.md` from the repository, re-apply each patch
in order, update `patches/winit/00-vendor-baseline.md` and `source-delta.json`, and run the
guards. `patches/winit/source-delta.json` lists every file that differs from the fork's package,
with both checksums, so a review can diff exactly those. Drop the override (back to upstream's
git pin) once the fork carries the change.
