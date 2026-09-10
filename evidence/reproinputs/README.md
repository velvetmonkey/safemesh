# Release input measurements

These are measurements, not platform support claims. The compatibility manifest's supported release set is empty. Its bootstrap row is not a release. `manifest_format` versions this evidence format only; it is unrelated to any protocol identity.

## Compiler and dependency inputs

CI Rust is pinned to 1.96.1 and maturin to 1.14.1. Exact compiler hashes, installed targets and runtime/distribution versions are in `versions.txt` and `node24-versions.txt`. Initial Node was 22.22.3; the corrected web run and packaging use Node 24.21.0 with npm 10.9.8. The inherited NODE_ENV=production requires `npm ci --include=dev` for web verification. No Lean build was started: the branch has zero changed lean/ files; lake resolves to /home/monkey/bin/lake. Lean toolchain text is recorded, not executed.

`cargo generate-lockfile` resolved unchanged manifest requirements to 33 external packages (37 including four workspace crates): 3 external direct packages and 30 external transitive-only packages. `dependencies.json` includes direct development dependencies. The generated `rust/Cargo.lock` is force-added because the existing ignore rule ignores Cargo.lock; the ignore rule is unchanged. Existing JS and Lean dependency files are retained.

`cargo build --locked` and `cargo test --workspace --locked` passed. Temporarily requiring serde_json =1.0.150 instead of locked 1.0.151 made `cargo build --locked` refuse with exit 101; see `known-bad.txt`. The manifest was restored, and `restored.txt` is the empty porcelain status for that file. No dependency requirement was changed by this lane.

## Tested Rust floor

Candidate 1.82.0 failed, 1.83.0 failed, 1.88.0 failed, 1.89.0 passed for `cargo +VERSION build --workspace --locked --features safemesh-crdt/laws`. 1.82 and 1.83 lack File lock APIs; 1.88 reports unstable `file_lock`. Logs retain the failures. The workspace and all four crates declare the common tested floor 1.89, covering the optional local-writer API enabled by laws. This is not a claim that each default-feature crate individually requires 1.89. The declared floor also passed workspace tests with laws and both named cross-target builds; see the msrv logs. Earlier releases between 1.83 and 1.88 were not tested.

## Compatibility manifest mechanism

`compatibility.json` keeps package, frame, schema and durable-wrapper identities separate, with source line citations. The wrapper has no explicit version: the measured layout is 24 bytes of allocation metadata followed by the existing event log. Do not infer a wrapper version from the package version or this manifest's format number.

For each future measurement, read each identity from that source SHA, query GitHub releases and tags using the commands retained in the manifest, and retain artifact hashes and build inputs. Add a supported-release row only after an actual release exists and compatibility is measured; include source SHA, release/tag identity, separate protocol identities, artifact hashes and the compatibility test evidence. Keep the bootstrap row separate. This lane does not test upgrade/downgrade compatibility or publish a release.

## Remaining moving inputs and maintenance

The pins require deliberate upgrades and rerunning all fifteen L-checks rows. This lane changes only the two scoped CI inputs. docs.yml still selects stable; Python's pyproject.toml and the package-smoke fallback still allow a maturin range. CI still selects ubuntu-latest, Python 3.11 and Node 24 (minor/patch moving), and conditionally installs tools if absent. Existing gate commands do not universally pass --locked. The recorded release commands explicitly enforce the lock; this is not a claim that every future CI invocation is hermetic. Build tools and registry/cache provisioning still need preservation for offline replay.
