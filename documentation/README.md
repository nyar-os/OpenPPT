# OpenPPT maintainer guide

Maintainer material for this repository. User-facing documentation lives in the [root readme](../readme.md) and the [package README](../projects/packages/open-ppt/README.md); this folder covers layout, build, release and dependency policy.

## Project layout

| Path | Role |
| --- | --- |
| `projects/crates/deck` | `open-ppt-deck`: editable deck model, editor, SVG renderer, import/export, PDF projection, outline |
| `projects/crates/napi` | `open-ppt-napi`: Node-API binding that maps values onto the deck crate |
| `projects/packages/open-ppt` | `@wae/open-ppt`: CLI (`open-ppt`) and TypeScript SDK |
| `projects/packages/open-ppt-ui` | `@wae/open-ppt-ui`: VMZ static UI staged into `dist/gui` for the desktop window |
| `projects/packages/open-ppt-skills` | `@wae/open-ppt-skills`: skill documents only, no runtime code |
| `projects/packages/open-ppt-<platform>-<arch>` | Prebuilt native addon packages: `win32-x64`, `darwin-x64`, `darwin-arm64`, `linux-x64`, `linux-arm64` |
| `projects/sites/homepage` | `@wae/open-ppt-homepage`: VMZ product homepage |
| `wae.config.ts` | Native targets, package list, build order and CI contract |
| `Cargo.toml`, `pnpm-workspace.yaml` | Rust workspace and pnpm workspace |

Build order is fixed by `wae.config.ts`: native, TypeScript, UI, skills, staging.

## Build and test

Prerequisites: Node.js 18 or newer (CI uses Node 22), pnpm `10.28.0` from the root `packageManager` field, and the Rust toolchain from `rust-toolchain.toml` (nightly, minimal). Rust edition is 2024 with `rust-version = "1.90"`.

```bash
pnpm install                 # install workspace dependencies
wae native build             # build the addon and stage it into the platform packages
pnpm run build               # wae build: native -> ts -> ui -> skills -> staging
pnpm typecheck               # tsc --noEmit across projects/packages/*
pnpm test                    # node --test for every package
cargo test -p open-ppt-deck --lib
pnpm run check:rust          # cargo fmt --check, clippy -D warnings, workspace tests
```

`dist/`, `*.node`, `target/` and `Cargo.lock` are git-ignored. The addon built by `wae native build` is copied into the matching `open-ppt-<platform>-<arch>` package; release packaging must not depend on a locally built `.node` outside that staging step.

TypeScript tests live in `projects/packages/*/tests/` and compile to `dist-test/` via `tsc -p tsconfig.test.json`; Rust integration tests live in `projects/crates/*/tests/`.

## Studio build

`@wae/open-ppt-ui` is a private VMZ package. Its build is part of `wae build` (`native → ts → ui → skills → staging`); run it directly with:

```bash
pnpm --filter @wae/open-ppt-ui run check   # vmz check
pnpm --filter @wae/open-ppt-ui run build   # vmz build --profile static
pnpm --filter @wae/open-ppt-ui run serve   # local preview
```

`vmz build` emits `dist/static`, which the staging step copies to `projects/packages/open-ppt/dist/gui` for the native WebView host.

## Release and publishing

Real semver versions are published only through npm Trusted Publisher (OIDC) from CI, triggered by a version tag or an explicitly authorized workflow dispatch. Do not publish real versions from a workstation.

Current repository state:

- `.github/workflows/publish-npm.yml` builds the native addon for all five targets, stages each `.node` into its platform package, builds the TypeScript surface and GUI, and publishes through `nifty publish` on `v*` tags.
- `nifty.config.ts` declares the Trusted Publisher target (`nyar-os/OpenPPT`, `publish-npm.yml`, `NPM_PUBLISH`).
- `@doki-land/nifty` is a root dev dependency, so `pnpm exec nifty` is available locally.
- `ci.workflows` lists `check-rust.yml` and `check-typescript.yml` with `requiredJobs: ['build', 'check']` and `shaBinding: false`.
- The `release` block is intentionally unset. `@wae/open-ppt` and the five platform packages are publishable; `@wae/open-ppt-ui` and `@wae/open-ppt-skills` remain `private`.

Placeholder publishing uses the local Nifty path (`nifty publish --placeholder`) for `0.0.0` stubs and Trusted Publisher configuration. It is not a real release.

`nifty` credentials live in `.secrets/nifty.json`. The whole `.secrets/` directory is local-only and git-ignored; never commit it, print it, or replace it with an environment-variable fallback.

## Dependency pin discipline

Every cross-repository Cargo dependency pins a full 40-character `rev`; branches and tags are not allowed.

The shared document engine is pinned through `document-presentation`, `document-presentation-pdf`, `document-presentation-pptx`, `document-formats`, `boost-fs`, `data-encoding` and `data-binary-zip`. All of them currently resolve to the same upstream revision recorded in `Cargo.toml`.

OpenPPT may deliberately pin a different revision than another consumer of the same engine. When upgrading:

1. Update every related pin in one change so the graph stays consistent.
2. Run `cargo metadata` (and a build) in a clean environment; a local path overlay would hide a broken `rev`.
3. Update fixtures and tests for any wire or API change instead of adding a compatibility layer.
4. Rebuild and re-stage the native addon, then re-run the package tests.

## CI

- `.github/workflows/check-rust.yml`: matrix over `ubuntu-latest`, `macos-latest` and `windows-latest`; installs nightly with `rustfmt` and `clippy`; runs `cargo build --workspace --release` and `cargo test --workspace --release`.
- `.github/workflows/check-typescript.yml`: Ubuntu with pnpm and Node 22; `pnpm install --frozen-lockfile`, then `pnpm typecheck` and `pnpm test`.
- Both workflows trigger on pushes and pull requests to `master` and `dev`.

## Documentation boundaries

- Public READMEs are English only and must not name internal planning directories or handoff files.
- User-facing files describe product behavior and verified commands. Maintainer-only facts belong in this folder.
- When a capability changes, update `open-ppt capabilities`, the package README and the skill document together, so none of them claims more than the installed engine reports.
