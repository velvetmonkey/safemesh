# v0.1.0 release compatibility bytes

Tagged source: `v0.1.0` at `295b04ef6f64d11395452ddb8029747c6e03fb22`. The pinned generator is `tests/v010_compat.rs` in this checkout. Copy that one test file into an isolated checkout at the tag and run it there; no product source from current main is copied. The generator calls the tagged wire encoders and durable writer. It supplies fixed identity bytes `01..10` to the tagged writer through a test-only `/dev/urandom` read hook. The transaction fixture joins the tagged `LocalReplica::allocation_bytes` and tagged `EventLog::to_wire_bytes`, the two outputs that the tagged legacy transaction writer concatenates.

## Values and SHA-256

All wire values are also declared in `wire_cases`; `release_wire_bytes_read_and_refuse` decodes the committed files and compares re-encoded values to these exact bytes. The stored files are installed together in throwaway directories by `release_store_files_restart_to_documented_state` and restarted to tally `[5, 0]`, one accepted record, allocation sequence 1 and one collision alarm. The transaction and journal are alternative history containers, never opened together.

| File | Documented value | SHA-256 |
| --- | --- | --- |
| `alarms.bin` | stored CollisionReport: one record-ID collision, local tally 5 versus offered tally 7 | `5f9b7ded5b990f87f2ea8417d6b42cb10d8b8b41c57eb6c2e7d9ecedcef3377b` |
| `collision-report.bin` | empty CollisionReport<GCounterDelta> | `c8e4184ed4730d8f876cedeadd6295223970fcd3d97e4179a5abc62a6f107ed7` |
| `counter.bin` | rollback high-water sequence 1, identity bytes 01..10 | `34455f221e715c4759622efd881ff4b317780109a3e7694976e752f7e65fcfb1` |
| `event-log.bin` | empty EventLog<GCounterDelta> bound to two writers | `460b39d4a7f215550ac65784999aa7e45be6c9463faaa4bbd13a6d641992507c` |
| `fence.bin` | writers 2, writer 0, generation 1 | `53afea624a503a0bf39e469e8979f67fcb1890ea0392adf9e155124f5ede9ebb` |
| `flag-disable.bin` | Disable tokens [2] | `9e21e2abdeb139d4dbe62dc6feddcf65858e15f483d744a0a2dff566891b0ca2` |
| `flag-enable.bin` | Enable token 2 | `29572fb8a94b59befdda71c42b5fcdedcf14d867ca0e719ec26be12be01e42aa` |
| `flag-state.bin` | enabled token 2 | `c200f837bbdae0dda967c8776540cb8ba97fa3c00aecf109e3d3cbad179aa71e` |
| `gcounter-delta.bin` | replica 0, tally 5 | `f560fe4b4a50e06dd11cb07cee30d6da7a71fecd0801236400709ba55d0173ba` |
| `gset-state.bin` | contains 7 | `6b65161d95ef0066846089a2440ffa275ea7359c93c4331cdc1b5a2dc914d2f0` |
| `journal.bin` | two-writer counter store, writer 0, one bump to tally 5 and one collision alarm | `a2d484c391772500bef065b7eff3c2d9c4e9d8e04d42d6dee374c095befa7b2d` |
| `map-remove.bin` | Remove key 2, timestamp 3, replica 0 | `afa173755d641fee235710246890781ded7dabad26699444965724358c3e8058` |
| `map-set.bin` | Set key 2, timestamp 3, replica 0, value 7 | `32d93bec1772b28fed3d5a1adbe753175b37516098bfd2f478b81c0c24cfc8b3` |
| `map-state.bin` | key 2 maps to value 7 at timestamp 3, replica 0 | `05a4a835cc1f84cea8227b265efe03b2daa27ba1a652eebdf5d8e2ad97552eac` |
| `orset-u64-add.bin` | Add element 7, token 2 | `324242ca5f0213c0b80355ae206c9f0bfa89f8f34001f6d6e966ae77444b674c` |
| `orset-u64-remove.bin` | Remove token 2 | `1d0fb1e8a20deecd23a912767dce1bbb15f3fda8192ab139756579910dff2e72` |
| `orset-u64-state.bin` | element 7 at token 2 | `08226be8d58655e3e9fa35cd30850cdd397665f4cf61e14e0723fb840d0297e6` |
| `orset-utf8-add.bin` | Add café, token 2 | `7e52a2d092eca05dc99b85939dd12dc0336bb9b98802ee66eacb78b14621a173` |
| `orset-utf8-remove.bin` | Remove token 2 | `b55669023da5ffa8e336e487846a1a019a304f9bfa41c8eefb635574094972f5` |
| `orset-utf8-state.bin` | café at token 2 | `ec176799e6dcf77c458c24470ad27b2e1ad5f4db9904764ed6b3323ee8ba6a34` |
| `pn-dec.bin` | Dec replica 1, tally 2 | `1827d9a0ef640d1fa58a88d7e6aaa637d7106404a82ecb673895ded429c8bbd8` |
| `pn-inc.bin` | Inc replica 0, tally 5 | `06b7be4c4a581600f0568e8f415288fe3ded543bbe17824e46fefdf351177b3d` |
| `record.bin` | record ID (0,1), GCounterDelta replica 0 tally 5 | `f7b0e5d2ef0177f2e6d4612ca298dcd81a8e2ed032371f692c1d3c3816d21b61` |
| `register-delta.bin` | timestamp 2, replica 0, value 7 | `865e598196fb166694da8d0e7f13454bf3049b48ebc97d23bf269dda625573d3` |
| `register-state.bin` | timestamp 2, replica 0, value 7 | `d17d1eaa7248c5420153207acb5ea16697aa9d8433b6a20adb91f59d533ae1ba` |
| `rga-delete.bin` | Delete position 2 | `bc1224b779fea980d924d7d5d38d0c33f8ebc97b19fb36bdf38ac64ecc95b08f` |
| `rga-insert.bin` | Insert position 2, value 7 | `df55aba1d89b22204aebade8ae18b488a1992ea0314f35c40c141cb4dbc850e1` |
| `rga-state.bin` | position 2 contains value 7 | `fc32ae4551446047e32cbd82953858dbde8ac2e465862cdb8ef7ea033ebbb8f0` |
| `rollback.bin` | marker for writers 2, writer 0, identity bytes 01..10 | `601666c995289f59293e3bed3bca8a09aeca86ecc7da2cab67b36130537c7c39` |
| `transaction.bin` | two-writer counter store, writer 0, sequence 1, tally 5 | `e531dd6d0190fe9db19a39ab9fa4a551faa462373b4982e350e4eba314e8587f` |
| `version-vector.bin` | empty VersionVector | `93e60f669b99ad3e3ee6284b139e57adfb419960f390858e46ea565bbf82d001` |

## Reproduce at the tag

From the repository root, set a fresh private scratch directory outside the checkout and run:

This block requires Git and the Rust 1.96.1 toolchain pinned by `rust-toolchain.toml`; an older system Cargo cannot read this lock file.

```sh
set -e
original="$PWD"
scratch="${SMQ3_SCRATCH:?set a fresh private scratch directory}"
git worktree add --detach "$scratch/tag-repro" v0.1.0
cp rust/crates/safemesh-crdt/tests/v010_compat.rs "$scratch/tag-repro/rust/crates/safemesh-crdt/tests/v010_compat.rs"
mkdir -p "$scratch/tmp" "$scratch/reproduced"
cd "$scratch/tag-repro"
TMPDIR="$scratch/tmp" CARGO_TARGET_DIR="$scratch/tag-repro-target" SMQ3_GENERATE="$scratch/reproduced" cargo test --manifest-path rust/Cargo.toml -p safemesh-crdt --features local-writer --test v010_compat store::generate_v010 -- --ignored --exact
set -- "$original"/rust/crates/safemesh-crdt/tests/fixtures/v0.1.0/*.bin
[ "$#" -eq 31 ] || { printf 'expected 31 committed fixtures, found %s\n' "$#" >&2; exit 1; }
for f do
    reproduced="$scratch/reproduced/$(basename "$f")"
    [ -f "$reproduced" ] || { printf 'missing reproduced fixture: %s\n' "$reproduced" >&2; exit 1; }
    cmp "$reproduced" "$f"
done
```

A changed byte needs a checked migration and a new release fixture set; do not refresh v0.1.0 bytes. The unversioned transaction and fence cannot identify a compatible-looking newer wrapper. A future journal tail can be treated as interrupted-append debris.
