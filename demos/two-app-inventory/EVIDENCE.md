# Measured journey, 2026-09-17

Local Linux x86_64, CPython 3.12.3, maturin 1.14.1, Rust 1.96.1.
Package sources: main `e4d91a7284611c5f9dda12fdcb6410b515b91621`.
Wheel: `safemesh_python-0.1.0-cp38-abi3-manylinux_2_34_x86_64.whl`; SHA-256 computed from the built artifact:
`a97615df17217ae6cff54597cbcfe5b499309c64986df429a661a53e61c48f59`.
This is a historical observation, not a wheel pin or a cross-build digest promise.

The default journey exited 0, retaining **6001 unique records
in each store** in **83.636 seconds** from the start of edits
through final restart verification (installation excluded). Both ended at
`[3001, 3000]`, total `6001`. SQLite `integrity_check` returned `ok` for both
retained stores; each contained 228,038 bytes of SafeMesh record payloads.

The normalized full-history hash was
`3043279323857be3ecaaa463afbfc773fbe42bc7d76869453c4f41ad996c5e93` on both sides.
The raw log hashes differed across peers because arrival orders differed; exact
raw bytes survived the clinic's partitioned restart. No wire fields were patched.

Reconnection sent 8,000 records from clinic (2,000 newly accepted, 6,000
duplicates) and 12,000 from warehouse (2,000 newly accepted, 10,000 duplicates),
both in reverse arrival order and with each record sent twice. The clinic was
SIGKILLed once with 3,000 records, and both apps were SIGKILLed with 6,001 records
before the final replay checks. Final processes had different PIDs and distinct
venv module paths; each deployment contained only its own entry point plus the
shared application plumbing copied locally.

The full journey physically dropped the last clinic record from an HTTP payload:
clinic total 6,001 versus warehouse 6,000. The acceptance oracle rejected it,
then successful delivery healed the divergence. A separate `--leave-divergent
--rounds 5 --interval 0` invocation exited **1** with `AssertionError: divergent
state`: `[16, 15]` versus `[15, 15]`. The corrected short smoke journey exited 0.

The first smoke attempt exited 1 because it incorrectly assumed that raw
serialized log bytes were independent of arrival order. The retained
full-history assertion now uses sorted opaque-record replay through the public
API; raw log hashes remain separately visible. See README for all API friction.

Additional validation: local wheel build/install and `cargo test -p
safemesh-python --locked` passed. No Lean sources changed; no Lean build started.
The complete CI gate is not claimed as locally run. This directory adds an
explicitly runnable acceptance journey, not a CI workflow.

Retained lane evidence on the measurement host:
`/home/monkey/scratch/safemeshgradea3/retained.log`, `retained/`,
`negative.log`, `negative/`, `smoke.log`, `smoke-fixed.log`, `wheel.log`, and
`python-tests.log`. Each command's exit code is in the corresponding `.exit`
file. These scratch paths are evidence for this run, not prerequisites for
reproduction; follow README with a fresh run directory.
