# Provenance

EvCore's first code commit is a snapshot import, not a continuation of any
earlier history. Its only Git ancestor is the EvCore template commit
`a1d4252f96200031691bcd179441243639216b23` ("Initial commit"). No commits,
branches or tags from Orca Harness or FineCore were imported.

Import date: 2026-11-15.

## Source

| Item | Value |
| --- | --- |
| Upstream | [okikorg/orca-harness](https://github.com/okikorg/orca-harness) |
| Upstream commit | `2a9fd399cfbbaad773e3cbed92bb6e24d7561f01` (an ancestor of upstream `main`; 5 commits after `orcacode-v0.9.0`, 7 before `orcacode-v0.10.0`) |
| Upstream tree | `9c35d0dd0d7e0f226063f16dd80161e231628e2b` |
| Import reference | FineCore commit `e036dd7a7e2a040b4da1152d99aede0552cb1641` ("chore: import Orca upstream tree verbatim at 2a9fd399") |
| Import reference tree | `9c35d0dd0d7e0f226063f16dd80161e231628e2b` (identical to upstream; `git diff-tree` is empty) |
| Historical reference | [oigorbrito/FineCore](https://github.com/oigorbrito/FineCore), consulted only |
| Identity source | [FineCore PR #188](https://github.com/oigorbrito/FineCore/pull/188), merge commit `d2011bc752842c66fc508d65c03c011c959cc8df` |

Both source commits were fetched directly from GitHub and resolve to the same
tree object: 1101 blobs (1076 `100644`, 25 `100755`), no symbolic links and no
submodules. The EvCore tree was built from those tree and blob objects alone.

## Differences from the upstream tree

The final tree is `9c35d0dd` plus the changes below; no other path differs and
no file mode changes. `git diff 2a9fd399 <this commit>` reproduces the list.

### Identity, adapted from PR #188

PR #188 was written against Orca v0.12.0 (`d89bf274`), which is newer than
this baseline, so its hunks were applied by content rather than copied, with
`FineCore`/`FINECORE`/`finecore` written as `EvCore`/`EVCORE`/`evcore`. No
v0.12.0 code was brought in.

- `crates/cli/Cargo.toml`, `crates/cli/src/evcore.rs`: an `evcore` binary that
  `include!`s `main.rs`, built alongside the unchanged `orcacode` binary.
- `crates/cli/src/main.rs`, `crates/cli/src/headless.rs`: system and bare
  prompts begin "You are EvCore". The USAGE text says "the application's device
  login" (PR #188 changed this line in `usage.rs`, which the baseline keeps in
  `main.rs`).
- `crates/cli/src/run_args.rs`: `--help` shows an EvCore header and the invoked
  binary name. `--version`, `resume` errors and the `update` line use the
  invoked binary name or the disabled-updater note. These are the same
  replacements as PR #188, applied to the baseline's parser.
- `crates/cli/src/approval.rs`, `crates/cli/src/tui/runtime/terminal.rs`: the
  headless approval hint, session-end line and resume hint use the invoked
  binary name.
- `crates/cli/src/plugin/mod.rs`: plugin usage reads `evcore plugin …` for both
  binaries, as in PR #188.
- `crates/cli/src/tui/components/welcome.rs`: welcome card `EVCORE` with the
  subtitle "EvCore — your terminal coding agent".
- `crates/cli/src/tui/command_catalog.rs`, `tui/commands/{dispatch,integrations,refine}.rs`,
  `tui/keys/helpers.rs`, `tui/keys/overlays/apply.rs`,
  `tui/runtime/submission.rs`, `tui/render/overlays.rs`: neutral wording
  ("exit the application", "restart the application", "Missing models? Check
  account access.").
- Tests: `crates/cli/src/main_tests.rs`, `tui/tests/core_4_welcome_help.rs` and
  the new `crates/cli/tests/evcore_identity.rs`.
- `README.md`: the title is EvCore, and a provenance note says the
  documentation below it describes upstream Orcacode.

### Approved functional change: upstream updater disabled

This change comes from PR #188 commit `5d5fd1dd`. Upstream `update` ran the
orcapods.ai installer, and startup polled `orcapods.ai/dl/orcacode/latest.json`.
In EvCore, either path would replace EvCore with upstream Orcacode.

- `crates/cli/src/update.rs`: `update` exits with status 1 and prints
  `update unavailable: no EvCore update channel is configured`. It makes no
  network call and starts no installer.
- `crates/cli/src/runtime/interactive.rs`: the startup update check is removed.

No replacement update mechanism was added.

### Deviations from PR #188

- `evcore_identity.rs` drops `--provider local`. The baseline has no
  `--provider` flag, and `--base-url` already selects the local provider.
- `core_4_welcome_help.rs::welcome_centres_the_visible_card_not_its_maximum_width`
  compares against the widest visible card row. PR #188 kept the subtitle as
  the reference and added a ±1 cell tolerance. The upstream test assumed the
  subtitle was the widest row, but the shorter EvCore subtitle is not, so a
  tolerance cannot fix that. Rendering is unchanged, and the exact upstream
  assertion is restored.
- Text produced by the mechanical rename uses "an EvCore" instead of "a EvCore".

### Template files

The template's `README.md` and `evidence-repo-template/` (19 files) are not in
this tree. They remain in the ancestor commit `a1d4252f`.

## Not changed

These items stay as they are in the baseline: crate and package names
(`orcacode`, `orca-harness-*`), `ORCA_*` variables, configuration, session and
credential paths (`orcacode/…`), the `orcacode/<version>` user agent, the
OpenRouter title "Orca Code" (PR #188 also kept it), the `orca` theme, plugin
manifest paths, APIs, protocols, authentication, workflows (including
`release-orcacode.yml`), `docs/`, `LICENSE` and `NOTICE`.

The FineCore README provenance block is not imported, because it describes a
different base. FineCore-specific tooling under `legacy/` is not imported
either.
