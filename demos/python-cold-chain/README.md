# SafeMesh for builders: Python cold-chain data mule

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


This is the field-science story demo: a vaccine shipment leaves a clinic, a courier collects while offline, a freezer power blip is recorded, and a lab receives the shipment after a data-mule sync. The point is not the hardware. The point is that the same Rust core can sit under a Python workflow and make the modeled custody state converge after ugly delivery.

## Historical capture

This image was added in revision
[`7795963`](https://github.com/velvetmonkey/safemesh/commit/77959639aac1ab67116a53c0d3487d2ef5e4ae2b).
Its exact capture revision and date are unknown. The image's evidence digests
are historical; the [current text transcript](#what-you-should-see) below shows
the output of the current demo.

![Historical Python cold-chain terminal run](../assets/python-cold-chain-terminal.png)

## 30-second run

First complete [Before you run](../README.md#before-you-run), including the checkout and tools for
this demo. The timing below excludes that setup and is an observation on the named machine, not a
guarantee on other machines or networks.

Runtime tested on Ubuntu 24.04.4 x86_64 (AMD EPYC-Genoa, Rust/Cargo 1.96.1, CPython 3.12.3, maturin
1.14.1): the entire block below took 10.29 seconds with an empty Cargo target directory and an
already populated Cargo registry, then 3.08 seconds reusing that build. Each run created a fresh
virtualenv, installed a freshly packaged local wheel, and reached `CONVERGED=true`.

Build and install the local wheel into a temporary virtualenv:

```sh
tmp=$(mktemp -d)
(cd rust/crates/safemesh-python && maturin build --release --features extension-module --out "$tmp/wheels")
python3 -m venv "$tmp/venv"
"$tmp/venv/bin/pip" install --no-index --find-links "$tmp/wheels" safemesh-python
"$tmp/venv/bin/python" rust/crates/safemesh-python/examples/data_mule_demo.py
```

No registry publish is involved.

## What You Should See

```text
SafeMesh for builders / Python data mule
Case: vaccine shipment 9001 crosses a clinic, courier, and lab while links fail.
Honest boundary: this shows modeled custody convergence and matching log bytes; it does not prove sensor truth, real transport delivery, or durable storage.

[1/5] Clinic seals the shipment
  custody=clinic audit=clinic_collected_vaccine

[2/5] Courier collects offline
  custody=courier link=clinic<->courier lab_link=offline

[3/5] Freezer power blip is recorded
  temperature_alert=true sensor_truth=outside_safemesh_claim

[4/5] Data mule reaches the lab
  sync=log_bytes merge=Rust_core thin_binding=Python

[5/5] Lab receives and everyone syncs
  holders=['lab', 'lab', 'lab']
  audit_trail=['clinic_collected_vaccine', 'courier_collected_offline', 'freezer_power_blip_recorded', 'lab_receipt_signed']
  audit_counts=[4, 4, 4]
  temperature_alerts=[true, true, true]
  evidence_digests=holder:bb629e94dc243e37 audit:e831eab62272bb0c alert:0be44c27e43f0127
  CONVERGED=true python_data_mule_sample=9001 holders=[300, 300, 300] audit_counts=[4, 4, 4] temperature_alerts=[True, True, True]
```

The full run also prints the audit labels and short SHA-256 evidence digests for the holder, audit, and alert logs.

## What It Demonstrates

- Python calls a thin PyO3 binding; merge logic stays in the one Rust core.
- Custody starts at the clinic, moves to the courier, then settles on the lab after sync.
- A freezer excursion alert reaches every replica.
- Audit counts and log-byte digests match after the heal.
- The script exits nonzero unless all modeled replicas converge on the expected final view.

## Honest Claim Boundary

The G-Counter audit path reaches the Lean-backed Rust carrier. Runtime tested: the installed Python data-mule demo checks modeled LWW Map custody, Enable-wins Flag alerts, and matching log bytes through the Python binding. These paths are not separate Lean proofs in v0.1. Artifact available: the commands above create a local wheel. Maintainer-supported status for the Python binding is unknown.

This demo does not prove the temperature sensor was truthful, that custody law was followed, that storage is durable, or that a real network delivered packets. It shows the modeled state and bytes converge once the logs are exchanged.

## Why This Matters

Cold-chain and field-science systems often have intermittent collectors, offline couriers, and audit needs that outlive the network conditions of the day. This demo keeps the claim narrow: the delivery path can be messy, but once the modeled logs meet, the replicas settle on the same custody view.
