# SafeMesh Python

**v0 scope:** G-Counter and OR-Set are **supported**, within the [language-path limits](https://velvetmonkey.github.io/safemesh/#v0-support). G-Set, PN-Counter, RGA/Text, LWW Register (`LwwRegister`), Enable-wins Flag (`EnableWinsFlag`) and LWW Map (`LwwMap`) are **experimental**, including their deltas and wrappers. Existing proof/test evidence is unchanged by release status.


SafeMesh's Rust crate floor for consumers is **Rust 1.89**. For the source builds,
demos and locked wasm-pack 0.15.0 installation on this page, use **Rust 1.96.1**,
the full-gate CI version. Install rustup first (Linux/Bash, with curl and a native
C compiler/linker), then select that toolchain:

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.96.1
. "$HOME/.cargo/env"
rustup default 1.96.1
```

The default applies to your user account; the repository's `rust-toolchain.toml`
also selects 1.96.1 inside this checkout. An outside application's toolchain remains
its own choice; consuming the crate requires at least 1.89.

Install the Python build tool in an activated virtual environment before building:

```sh
python3 -m venv .venv-build
. .venv-build/bin/activate
python3 -m pip install 'maturin>=1.7,<2'
```


`safemesh-python` is the PyO3/maturin wrapper over the SafeMesh Rust core. It exposes byte-oriented replica/event-log helpers for Python code while keeping merge behavior in one Rust implementation.

## Claim boundary

This package provides binding glue with **API present** in `src/lib.rs`. **Artifact available** means a locally built wheel, not a PyPI release. **Build checked** and **runtime tested**: the local wheel was built with maturin 1.15.0, installed, and used to run `examples/data_mule_demo.py` on Ubuntu 24.04 x64 with CPython 3.12.3. **Integration tested** beyond that modeled local demo is unclaimed; **maintainer-supported** status is unknown. The G-Counter path reaches the Lean-backed Rust carrier; LWW Register, Enable-wins Flag, and LWW Map remain tested-not-proven. The binding itself is not a separate proof.

See the repository `CLAIMS.md` and `WHAT-IS-PROVEN.md` for the full wording rule.

## Install

Build a local wheel with maturin; v0.1 CI performs a wheel build and install smoke test but does not publish to PyPI.

The Linux x64 wheel reaches CPython 3.8 and later through PyO3's `abi3-py38` stable ABI. This is
artifact reach, not a claim that every interpreter or platform is tested; macOS, Windows, ARM64,
and other operating-system or architecture combinations have no artifact evidence here.

The checked distribution coordinate is CPython 3.11 on Linux x64. The `full-gate` CI job runs on the
`ubuntu-latest` runner label with Python 3.11; this is the tested set, and maintainer support remains
unknown.

```sh
cd rust/crates/safemesh-python
maturin build --release --features extension-module --out dist
python3 -m venv .venv-smoke
.venv-smoke/bin/pip install --no-index --find-links dist safemesh-python
```

## Quickstart

```python
import safemesh_python as sm

left = sm.GCounterReplica(1, 3)
right = sm.GCounterReplica(2, 3)

right.merge_record_bytes(left.append_bump(1, 5))
admissions = left.merge_log_bytes(right.log_bytes())
assert admissions == ["duplicate"]

print(left.value(), right.value())
```

`merge_log_bytes` returns one `"accepted"`, `"duplicate"`, or `"collision"`
verdict per input record, in order. Decode and whole-batch validation errors
raise before any record is applied.

## Demo

Narrated walkthrough: [`../../../demos/python-cold-chain/README.md`](../../../demos/python-cold-chain/README.md).

Run the data-mule demo after installing the local wheel:

```sh
cd rust/crates/safemesh-python
.venv-smoke/bin/python examples/data_mule_demo.py
```

It prints `CONVERGED=true` when the sample holder, audit count, and temperature-alert flag converge after partition and heal.

Run `./scripts/package-smoke.sh` from the repository root to build the wheel, install it into a temporary virtualenv, run the demo, and avoid publishing.

## Checked coordinates and OR-Set

`GCounter.try_apply_bump(replica, tally)` raises `IndexError` for an invalid
coordinate without changing state. `apply_bump` uses the same checked path.

```python
left, right = sm.OrSet(), sm.OrSet()
left.add(10, 101)
right.merge(left)
left.apply_remove(left.observed_tokens(10))
right.add(10, 201)  # Concurrent add uses a fresh token.
left.merge(right)
assert left.elements() == [10]
```

The set delegates to Rust `OrSet<u64, u64>`: elements and tokens are unsigned
64-bit integers (JavaScript uses `bigint`). Tokens are global to the set; use a
fresh, replica-unique token for every add to obtain add-wins behavior. Removal
persists tombstones even before an add arrives, and a reused token affects every
element carrying it. Observed tokens include tombstoned adds. Merge unions all
adds and tombstones, and reads return sorted unique live members. This is the
core's token semantics, including token reuse; the binding does not allocate IDs.
