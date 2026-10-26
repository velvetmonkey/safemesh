# v0.2.1 release compatibility bytes

Tagged source: `v0.2.1` (tag object `07fd38afe9260924ecda9d6069c19bc0c26b01c9`) at commit `7b581f769f1d187bdea4a5ede7159fd839982c33`. The pinned generator is `tests/v021_compat.rs` in this checkout, copied as the only new source file into an isolated checkout at that tag. The generator calls the tagged wire encoders and durable writers. A test-only `/dev/urandom` read hook supplies identity bytes `01..10`.

All 25 wire files are byte-identical to the retained v0.1.0 wire files; keeping them proves identity across releases. The eight stored files form two separate sets: `counter-*` and `set-*`. Each set contains exactly the journal, fence, rollback protection marker, and external rollback counter that the tagged writer left after one write. No transaction or alarm file was present after these writes. `release_wire_bytes_read` decodes and re-encodes every wire file. `release_stores_restart_to_documented_state` installs each complete stored set in a fresh directory and checks the tally or member, one record, and allocation sequence 1. `release_file_count` requires exactly 33 binary files.

## Values and SHA-256

| File | Documented value | SHA-256 |
| --- | --- | --- |
| `collision-report.bin` | empty GCounter collision report | `c8e4184ed4730d8f876cedeadd6295223970fcd3d97e4179a5abc62a6f107ed7` |
| `counter-counter.bin` | counter store rollback high-water 1, identity 01..10 | `34455f221e715c4759622efd881ff4b317780109a3e7694976e752f7e65fcfb1` |
| `counter-fence.bin` | counter store ownership: two writers, writer 0, generation 1 | `53afea624a503a0bf39e469e8979f67fcb1890ea0392adf9e155124f5ede9ebb` |
| `counter-journal.bin` | two-writer counter history, writer 0, one tally-5 record, sequence 1 | `a2d484c391772500bef065b7eff3c2d9c4e9d8e04d42d6dee374c095befa7b2d` |
| `counter-rollback.bin` | counter protection marker: two writers, writer 0, identity 01..10 | `601666c995289f59293e3bed3bca8a09aeca86ecc7da2cab67b36130537c7c39` |
| `event-log.bin` | empty two-writer GCounter event log | `460b39d4a7f215550ac65784999aa7e45be6c9463faaa4bbd13a6d641992507c` |
| `flag-disable.bin` | disable token 2 | `9e21e2abdeb139d4dbe62dc6feddcf65858e15f483d744a0a2dff566891b0ca2` |
| `flag-enable.bin` | enable token 2 | `29572fb8a94b59befdda71c42b5fcdedcf14d867ca0e719ec26be12be01e42aa` |
| `flag-state.bin` | enabled with token 2 | `c200f837bbdae0dda967c8776540cb8ba97fa3c00aecf109e3d3cbad179aa71e` |
| `gcounter-delta.bin` | replica 0, tally 5 | `f560fe4b4a50e06dd11cb07cee30d6da7a71fecd0801236400709ba55d0173ba` |
| `gset-state.bin` | GSet containing 7 | `6b65161d95ef0066846089a2440ffa275ea7359c93c4331cdc1b5a2dc914d2f0` |
| `map-remove.bin` | LWW map remove key 2, timestamp 3, replica 0 | `afa173755d641fee235710246890781ded7dabad26699444965724358c3e8058` |
| `map-set.bin` | LWW map key 2 value 7, timestamp 3, replica 0 | `32d93bec1772b28fed3d5a1adbe753175b37516098bfd2f478b81c0c24cfc8b3` |
| `map-state.bin` | LWW map key 2 value 7, timestamp 3, replica 0 | `05a4a835cc1f84cea8227b265efe03b2daa27ba1a652eebdf5d8e2ad97552eac` |
| `orset-u64-add.bin` | OR-Set add element 7 with token 2 | `324242ca5f0213c0b80355ae206c9f0bfa89f8f34001f6d6e966ae77444b674c` |
| `orset-u64-remove.bin` | OR-Set remove token 2 | `1d0fb1e8a20deecd23a912767dce1bbb15f3fda8192ab139756579910dff2e72` |
| `orset-u64-state.bin` | OR-Set containing 7 with token 2 | `08226be8d58655e3e9fa35cd30850cdd397665f4cf61e14e0723fb840d0297e6` |
| `orset-utf8-add.bin` | OR-Set add café with token 2 | `7e52a2d092eca05dc99b85939dd12dc0336bb9b98802ee66eacb78b14621a173` |
| `orset-utf8-remove.bin` | OR-Set remove token 2 | `b55669023da5ffa8e336e487846a1a019a304f9bfa41c8eefb635574094972f5` |
| `orset-utf8-state.bin` | OR-Set containing café with token 2 | `ec176799e6dcf77c458c24470ad27b2e1ad5f4db9904764ed6b3323ee8ba6a34` |
| `pn-dec.bin` | PN counter decrement, replica 1, tally 2 | `1827d9a0ef640d1fa58a88d7e6aaa637d7106404a82ecb673895ded429c8bbd8` |
| `pn-inc.bin` | PN counter increment, replica 0, tally 5 | `06b7be4c4a581600f0568e8f415288fe3ded543bbe17824e46fefdf351177b3d` |
| `record.bin` | GCounter record replica 0 sequence 1, tally 5 | `f7b0e5d2ef0177f2e6d4612ca298dcd81a8e2ed032371f692c1d3c3816d21b61` |
| `register-delta.bin` | LWW register value 7, timestamp 2, replica 0 | `865e598196fb166694da8d0e7f13454bf3049b48ebc97d23bf269dda625573d3` |
| `register-state.bin` | LWW register value 7, timestamp 2, replica 0 | `d17d1eaa7248c5420153207acb5ea16697aa9d8433b6a20adb91f59d533ae1ba` |
| `rga-delete.bin` | RGA delete position 2 | `bc1224b779fea980d924d7d5d38d0c33f8ebc97b19fb36bdf38ac64ecc95b08f` |
| `rga-insert.bin` | RGA insert value 7 at position 2 | `df55aba1d89b22204aebade8ae18b488a1992ea0314f35c40c141cb4dbc850e1` |
| `rga-state.bin` | RGA containing value 7 at position 2 | `fc32ae4551446047e32cbd82953858dbde8ac2e465862cdb8ef7ea033ebbb8f0` |
| `set-counter.bin` | UTF-8 OR-Set rollback high-water 1, identity 01..10 | `34455f221e715c4759622efd881ff4b317780109a3e7694976e752f7e65fcfb1` |
| `set-fence.bin` | UTF-8 OR-Set store ownership: two writers, writer 0, generation 1 | `53afea624a503a0bf39e469e8979f67fcb1890ea0392adf9e155124f5ede9ebb` |
| `set-journal.bin` | two-writer UTF-8 OR-Set history, writer 0, one café add, token 2, sequence 1 | `9b160b3feb1bfcfaf9351f0dc19f6c980b21b0f367e25cd2e2faacfaa039abe4` |
| `set-rollback.bin` | UTF-8 OR-Set protection marker: two writers, writer 0, identity 01..10 | `601666c995289f59293e3bed3bca8a09aeca86ecc7da2cab67b36130537c7c39` |
| `version-vector.bin` | empty VersionVector | `93e60f669b99ad3e3ee6284b139e57adfb419960f390858e46ea565bbf82d001` |

## Reproduce at the tag

From the repository root, set `SMV021_SCRATCH` to a fresh private scratch directory outside the checkout. Git, a C compiler, and the Rust toolchain pinned by `rust-toolchain.toml` are required.

```sh
set -e
original="$PWD"
scratch="${SMV021_SCRATCH:?set a fresh private scratch directory}"
git worktree add --detach "$scratch/tag-repro" v0.2.1
cp rust/crates/safemesh-crdt/tests/v021_compat.rs "$scratch/tag-repro/rust/crates/safemesh-crdt/tests/v021_compat.rs"
mkdir -p "$scratch/tmp" "$scratch/reproduced"
cd "$scratch/tag-repro"
TMPDIR="$scratch/tmp" CARGO_TARGET_DIR="$scratch/target" SMV021_GENERATE="$scratch/reproduced" cargo test --manifest-path rust/Cargo.toml -p safemesh-crdt --features local-writer --test v021_compat store::generate_v021 --locked -- --ignored --exact
set -- "$original"/rust/crates/safemesh-crdt/tests/fixtures/v0.2.1/*.bin
[ "$#" -eq 33 ] || { printf 'expected 33 committed fixtures, found %s\n' "$#" >&2; exit 1; }
set -- "$scratch"/reproduced/*.bin
[ "$#" -eq 33 ] || { printf 'expected 33 reproduced fixtures, found %s\n' "$#" >&2; exit 1; }
for f in "$original"/rust/crates/safemesh-crdt/tests/fixtures/v0.2.1/*.bin; do
    reproduced="$scratch/reproduced/$(basename "$f")"
    [ -f "$reproduced" ] || { printf 'missing reproduced fixture: %s\n' "$reproduced" >&2; exit 1; }
    cmp "$reproduced" "$f"
done
cd "$original"
git worktree remove --force "$scratch/tag-repro"
```

A changed byte needs a checked migration and a new release fixture set; do not refresh these v0.2.1 bytes.
