# SafeMesh Python

`safemesh-python` is the PyO3/maturin wrapper over the SafeMesh Rust core. It exposes byte-oriented replica/event-log helpers for Python code while keeping merge behavior in one Rust implementation.

## Claim boundary

This package is engineered and tested binding glue. The G-Counter path reaches the Lean-backed Rust carrier; LWW Register, Enable-wins Flag, and LWW Map remain tested-not-proven. The binding itself is not a separate proof.

See the repository `CLAIMS.md` and `WHAT-IS-PROVEN.md` for the full wording rule.

## Install

Build a local wheel with maturin; v0.1 CI performs a wheel build and install smoke test but does not publish to PyPI.

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
left.merge_log_bytes(right.log_bytes())

print(left.value(), right.value())
```

## Demo

Narrated walkthrough: [`../../../demos/python-cold-chain/README.md`](../../../demos/python-cold-chain/README.md).

Run the data-mule demo after installing the local wheel:

```sh
cd rust/crates/safemesh-python
.venv-smoke/bin/python examples/data_mule_demo.py
```

It prints `CONVERGED=true` when the sample holder, audit count, and temperature-alert flag converge after partition and heal.

Run `./scripts/package-smoke.sh` from the repository root to build the wheel, install it into a temporary virtualenv, run the demo, and avoid publishing.
