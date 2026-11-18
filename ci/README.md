# ci

Scripts and workflows for EvCore, derived from Orca Harness.

## Releases are disabled

EvCore has no release channel of its own yet, so this repository does not
publish anything. The upstream release pipeline was removed from
`.github/workflows/` and its publishing steps are disabled:

- `.github/workflows/release-orcacode.yml` (which created GitHub Releases for
  `orcacode-v*` tags and pushed binaries to the Orca release host) is gone.
  Its history stays in git.
- `ci/release.sh` refuses `notes`, `prepare`, `tag`, `build` and `publish`
  with an error.
- The `Makefile` release targets (`notes`, `prepare`, `tag`, `release`,
  `dist`, `publish`) fail with the same message.

No workflow replaces them. Re-enabling a release requires a decision recorded
in `PROVENANCE.md` and a new, reviewed pipeline with its own assets, host and
version policy.

## Checks

- `check-binary-size.sh <path>`: the uncompressed release binary must stay strictly under 6,700,000 bytes (6.7 decimal MB), or 9,000,000 bytes for a static `*-linux-musl` target, which carries its own libc.
- `check-source-size.sh`: every source file under `crates/` stays below 600 lines.

In `.github/workflows/ci.yml`, `check-source-size.sh` runs in the `lint` job, and `check-binary-size.sh` runs on the static musl binary in the `size` job, which builds `x86_64-unknown-linux-musl` with cargo-zigbuild. Clippy runs in its own `clippy` job, so a source-size failure cannot skip it. CI does not measure the darwin arm64 binary against its 6.7 MB limit. `check-source-size.sh` is also run by `make audit`.

## Parked workflows

`harness-test.yml` and `harness-bench.yml` are ready-to-use test and
benchmark workflows kept here rather than under `.github/workflows/`. They
still scope their paths to `orca-harness/**` from the monorepo layout; drop
that prefix when moving them into place.
