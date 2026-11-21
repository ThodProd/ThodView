# Project instructions

ThodView is a Rust (egui/eframe) image viewer for Windows x64. The C#/WPF 1.x code was replaced by the 2.0 Rust rewrite; `src/ThodView` is only stale build output and is ignored.

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

- Before the first grep, search or source-file read for a codebase question, run `graphify query "<question>"`. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts.
- After every code change (create, edit, rename or delete of a source file), run `graphify update .`. Use `graphify update . --force` when files were deleted or the graph would shrink.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review. Read raw source only for lines the graph or an earlier read already located.
- Dirty graphify-out/ files are expected; do not skip graphify because the graph is dirty.

## Build and verification

```powershell
cargo test --locked
.\scripts\build.ps1
```

`scripts/build.ps1` reads the version from `Cargo.toml`, builds `ThodView.exe`, bundles FFmpeg and ExifTool from `.cache`, and writes `artifacts/thodview-<version>-win-x64.zip`.

## Release

- The version lives in `Cargo.toml` (`[package] version`).
- Pushing a `v*` tag runs `.github/workflows/release.yml`, which builds on `windows-latest` and publishes the ZIP as a GitHub release.
- Keep `README.md` and `THIRD-PARTY-NOTICES.md` in sync with dependency changes.
