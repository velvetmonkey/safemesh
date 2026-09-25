# SafeMesh — delta-state CRDT convergence, built on crdt-lean.
# Copyright (C) 2026 Ben Cassie
# SPDX-License-Identifier: Apache-2.0
"""Differential test: Python StringOrSetReplica against WASM SafeMeshStringOrSetReplica.

Runs one scripted list of steps through the installed `safemesh_python` wheel and,
in a Node subprocess, through a wasm-pack `nodejs` package of safemesh-wasm. Every
step's result (bytes as hex, integers as decimal text, errors as their message)
must be identical on both sides.

Usage: python string_orset_wasm_parity.py <safemesh-wasm nodejs package dir>
       python string_orset_wasm_parity.py --selftest <package dir>

--selftest plants one known-bad result (a Python-only error text on one read
step) and requires the comparison to report exactly that step, so a harness that
compares nothing cannot pass.
"""

import json
import os
import subprocess
import sys

import safemesh_python as sm

MAX = str((1 << 64) - 1)

# Each step: [op, *args]. Names refer to replica handles or byte variables.
STEPS = [
    # Allocated identity: write, export, refuse a second live handle, restore.
    ["create_allocated", "a", "2", "0"],
    ["append_allocated_add", "a", "water", "r1"],
    ["inspect", "r1"],
    ["export_identity", "a", "id1"],
    ["import_identity", "id1", "dup"],
    ["create_allocated", "dup", "2", "0"],
    ["append_add", "a", "x", "5", "_"],
    ["append_remove_observed", "a", "nothing", "r0"],
    ["inspect", "r0"],
    ["export_identity", "a", "id1"],
    ["free", "a"],
    ["put_u64", "id1", 5, "0", "bad"],
    ["import_identity", "bad", "_"],
    ["put_u64", "id1", 5, "3", "bad"],
    ["import_identity", "bad", "_"],
    ["put_u64", "id1", 13, "1", "bad"],
    ["import_identity", "bad", "_"],
    ["put_u64", "id1", 21, "0", "bad"],
    ["import_identity", "bad", "_"],
    ["put_u64", "id1", 21, MAX, "bad"],
    ["import_identity", "bad", "_"],
    ["flip", "id1", -1, 1, "bad"],
    ["import_identity", "bad", "_"],
    ["import_identity", "id1", "b"],
    ["append_allocated_add", "b", "radio", "r2"],
    ["inspect", "r2"],
    ["elements", "b"],
    ["tombstones", "b"],
    ["add_entries", "b"],
    ["version_for", "b", "0"],
    ["log_bytes", "b", "logb"],
    ["export_identity", "b", "id2"],
    ["free", "b"],

    # Misuse of the lifecycle, with the WASM error texts.
    ["create_allocated", "_", "0", "0"],
    ["create_allocated", "_", "2", "2"],
    ["new", "p", "0"],
    ["append_allocated_add", "p", "x", "_"],
    ["export_identity", "p", "_"],
    ["literal", "534d4f49", "short"],
    ["import_identity", "short", "_"],
    ["literal", "534d4f4902" + "00" * 24, "wrongversion"],
    ["import_identity", "wrongversion", "_"],

    # Incoming records on an allocated replica must follow the allocation rule.
    ["create_allocated", "r", "2", "1"],
    ["new", "x", "0"],
    ["append_add", "x", "x", "7", "badtoken"],
    ["merge_record_bytes", "r", "badtoken"],
    ["log_bytes", "x", "badlog"],
    ["merge_log_bytes", "r", "badlog"],
    ["new", "y", "1"],
    ["append_add", "y", "y", "3", "ownauthor"],
    ["merge_record_bytes", "r", "ownauthor"],
    ["new", "z", "5"],
    ["append_remove_observed", "z", "z", "outsider"],
    ["merge_record_bytes", "r", "outsider"],
    ["merge_log_bytes", "r", "logb"],
    ["merge_record_bytes", "r", "r1"],
    ["merge_record_bytes", "r", "r2"],
    ["elements", "r"],
    ["append_allocated_add", "r", "tea", "r3"],
    ["inspect", "r3"],
    ["append_remove_observed", "r", "water", "r4"],
    ["inspect", "r4"],
    ["elements", "r"],
    ["tombstones", "r"],
    ["observed_tokens", "r", "radio"],
    ["log_bytes", "r", "logr"],

    # Allocated-writer collision: two handles for author 0, one after the other,
    # as two processes would hold them. Same record ID, different payload.
    ["create_allocated", "c1", "2", "0"],
    ["append_allocated_add", "c1", "apple", "c1rec"],
    ["log_bytes", "c1", "c1log"],
    ["free", "c1"],
    ["create_allocated", "c2", "2", "0"],
    ["append_allocated_add", "c2", "pear", "c2rec"],
    ["log_bytes", "c2", "c2log"],
    ["free", "c2"],
    ["inspect", "c1rec"],
    ["inspect", "c2rec"],
    ["free", "r"],
    ["create_allocated", "d", "2", "1"],
    ["merge_record_bytes", "d", "c1rec"],
    ["merge_record_bytes", "d", "c2rec"],
    ["merge_log_bytes", "d", "c2log"],
    ["merge_log_bytes", "d", "c1log"],
    ["elements", "d"],
    ["add_entries", "d"],
    ["version_for", "d", "0"],
    ["log_bytes", "d", "logd"],
    ["new", "e", "9"],
    ["merge_record_bytes", "e", "c2rec"],
    ["merge_record_bytes", "e", "c1rec"],
    ["elements", "e"],

    # Unallocated paths on the same inputs.
    ["new", "u", "1"],
    ["append_add", "u", "vaccine", "11", "u1"],
    ["append_add", "u", "vaccine", MAX, "u2"],
    ["append_remove_observed", "u", "vaccine", "u3"],
    ["inspect", "u3"],
    ["new", "v", "2"],
    ["merge_record_bytes", "v", "u1"],
    ["merge_record_bytes", "v", "u1"],
    ["log_bytes", "u", "ulog"],
    ["merge_log_bytes", "v", "ulog"],
    ["flip", "u1", 0, 255, "planted"],
    ["merge_record_bytes", "v", "planted"],
    ["inspect", "planted"],
    ["flip", "ulog", 3, 1, "badulog"],
    ["merge_log_bytes", "v", "badulog"],
    ["merge_record_bytes_budget", "v", "u3", 1],
    ["merge_log_bytes_budget", "v", "ulog", 1],
    ["tombstones", "v"],
    ["add_entries", "v"],
    ["version_for", "v", "1"],
    ["log_bytes", "v", "logv"],
]

PLANTED = STEPS.index(["elements", "e"])

JS = r"""
const { join, resolve } = require("node:path");
const wasm = require(join(resolve(process.argv[1]), "safemesh_wasm.js"));
const R = wasm.SafeMeshStringOrSetReplica;
const steps = JSON.parse(require("node:fs").readFileSync(0, "utf8"));
const hex = (b) => Buffer.from(b).toString("hex");
const opt = (v) => (v === undefined || v === null ? null : String(v));
const handles = {}, vars = {};
function run(step) {
  const [op, ...a] = step;
  switch (op) {
    case "new": handles[a[0]] = new R(BigInt(a[1])); return null;
    case "create_allocated": handles[a[0]] = R.createAllocated(BigInt(a[1]), BigInt(a[2])); return null;
    case "import_identity": handles[a[1]] = R.importIdentity(vars[a[0]]); return null;
    case "free": handles[a[0]].free(); delete handles[a[0]]; return null;
    case "append_add": vars[a[3]] = handles[a[0]].appendAdd(a[1], BigInt(a[2])); return hex(vars[a[3]]);
    case "append_allocated_add": vars[a[2]] = handles[a[0]].appendAllocatedAdd(a[1]); return hex(vars[a[2]]);
    case "append_remove_observed": vars[a[2]] = handles[a[0]].appendRemoveObserved(a[1]); return hex(vars[a[2]]);
    case "export_identity": vars[a[1]] = handles[a[0]].exportIdentity(); return hex(vars[a[1]]);
    case "log_bytes": vars[a[1]] = handles[a[0]].logBytes(); return hex(vars[a[1]]);
    case "merge_record_bytes": return handles[a[0]].mergeRecordBytes(vars[a[1]]);
    case "merge_log_bytes": return handles[a[0]].mergeLogBytes(vars[a[1]]);
    case "merge_record_bytes_budget": return handles[a[0]].mergeRecordBytes(vars[a[1]], a[2]);
    case "merge_log_bytes_budget": return handles[a[0]].mergeLogBytes(vars[a[1]], a[2]);
    case "elements": return handles[a[0]].elements();
    case "tombstones": return Array.from(handles[a[0]].tombstones(), String);
    case "observed_tokens": return Array.from(handles[a[0]].observedTokens(a[1]), String);
    case "add_entries": return handles[a[0]].addEntries().map((e) => [e.element(), String(e.token())]);
    case "version_for": return String(handles[a[0]].versionFor(BigInt(a[1])));
    case "inspect": {
      const r = R.inspectRecordBytes(vars[a[0]]);
      return [String(r.replica()), String(r.sequence()), r.deltaKind(), opt(r.element()),
              opt(r.token()), Array.from(r.tokens(), String)];
    }
    case "literal": vars[a[1]] = Uint8Array.from(Buffer.from(a[0], "hex")); return null;
    case "flip": {
      const b = Uint8Array.from(vars[a[0]]);
      const i = a[1] < 0 ? b.length + a[1] : a[1];
      b[i] ^= a[2]; vars[a[3]] = b; return hex(b);
    }
    case "put_u64": {
      const b = Uint8Array.from(vars[a[0]]);
      new DataView(b.buffer).setBigUint64(a[1], BigInt(a[2]), true);
      vars[a[3]] = b; return hex(b);
    }
    default: throw new Error(`unknown op ${op}`);
  }
}
const out = steps.map((step) => {
  try { return { ok: run(step) }; }
  catch (error) {
    if (error.name !== "SafeMeshError") return { crash: String(error) };
    return { error: error.message };
  }
});
process.stdout.write(JSON.stringify(out));
"""


def run_python(steps, planted=False):
    R = sm.StringOrSetReplica
    handles, variables = {}, {}

    def save(name, value):
        variables[name] = bytes(value)
        return variables[name].hex()

    def one(step):
        op, *a = step
        if op == "new":
            handles[a[0]] = R(int(a[1]))
        elif op == "create_allocated":
            handles[a[0]] = R.create_allocated(int(a[1]), int(a[2]))
        elif op == "import_identity":
            handles[a[1]] = R.import_identity(variables[a[0]])
        elif op == "free":
            del handles[a[0]]
        elif op == "append_add":
            return save(a[3], handles[a[0]].append_add(a[1], int(a[2])))
        elif op == "append_allocated_add":
            return save(a[2], handles[a[0]].append_allocated_add(a[1]))
        elif op == "append_remove_observed":
            return save(a[2], handles[a[0]].append_remove_observed(a[1]))
        elif op == "export_identity":
            return save(a[1], handles[a[0]].export_identity())
        elif op == "log_bytes":
            return save(a[1], handles[a[0]].log_bytes())
        elif op == "merge_record_bytes":
            return handles[a[0]].merge_record_bytes(variables[a[1]])
        elif op == "merge_log_bytes":
            return handles[a[0]].merge_log_bytes(variables[a[1]])
        elif op == "merge_record_bytes_budget":
            return handles[a[0]].merge_record_bytes(variables[a[1]], max_collection_elements=a[2])
        elif op == "merge_log_bytes_budget":
            return handles[a[0]].merge_log_bytes(variables[a[1]], max_collection_elements=a[2])
        elif op == "elements":
            return handles[a[0]].elements()
        elif op == "tombstones":
            return [str(t) for t in handles[a[0]].tombstones()]
        elif op == "observed_tokens":
            return [str(t) for t in handles[a[0]].observed_tokens(a[1])]
        elif op == "add_entries":
            return [[e, str(t)] for e, t in handles[a[0]].add_entries()]
        elif op == "version_for":
            return str(handles[a[0]].version_for(int(a[1])))
        elif op == "inspect":
            r = R.inspect_record_bytes(variables[a[0]])
            opt = lambda v: None if v is None else str(v)
            return [str(r.replica()), str(r.sequence()), r.delta_kind(), r.element(),
                    opt(r.token()), [str(t) for t in r.tokens()]]
        elif op == "literal":
            variables[a[1]] = bytes.fromhex(a[0])
        elif op == "flip":
            b = bytearray(variables[a[0]])
            b[a[1]] ^= a[2]
            return save(a[3], b)
        elif op == "put_u64":
            b = bytearray(variables[a[0]])
            b[a[1]:a[1] + 8] = int(a[2]).to_bytes(8, "little")
            return save(a[3], b)
        else:
            raise AssertionError("unknown op " + op)
        return None

    out = []
    for index, step in enumerate(steps):
        try:
            out.append({"ok": one(step)})
        except ValueError as error:
            out.append({"error": str(error)})
        except Exception as error:  # A non-ValueError is a binding defect.
            out.append({"crash": repr(error)})
    if planted:
        # One Python-only result on a read step that nothing later depends on.
        out[PLANTED] = {"error": "planted python-only text"}
    return out


def run_wasm(package, steps):
    result = subprocess.run(
        ["node", "-e", JS, package],
        input=json.dumps(steps),
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        sys.stderr.write(result.stderr)
        raise SystemExit("node side failed (exit %d)" % result.returncode)
    return json.loads(result.stdout)


def differences(python, wasm):
    found = []
    for step, left, right in zip(STEPS, python, wasm):
        if "crash" in left or "crash" in right or left != right:
            found.append((step, left, right))
    if len(python) != len(wasm):
        found.append(("length", len(python), len(wasm)))
    return found


def main(argv):
    selftest = argv[:1] == ["--selftest"]
    if selftest:
        argv = argv[1:]
    if len(argv) != 1 or not os.path.isdir(argv[0]):
        raise SystemExit(__doc__)
    wasm = run_wasm(argv[0], STEPS)
    python = run_python(STEPS, planted=selftest)
    found = differences(python, wasm)
    if selftest:
        if [f[0] for f in found] != [STEPS[PLANTED]]:
            raise SystemExit("selftest: planted difference not reported alone: %r" % (found,))
        print("STRING_ORSET_WASM_PARITY_SELFTEST=true")
        return
    for step, left, right in found:
        print("DIFF", json.dumps(step), "python=", json.dumps(left), "wasm=", json.dumps(right))
    if found:
        raise SystemExit("%d of %d steps differ" % (len(found), len(STEPS)))
    errors = sum(1 for r in python if "error" in r)
    verdicts = [r["ok"] for r in python if r.get("ok") in ("accepted", "duplicate", "collision")]
    assert "collision" in verdicts, "the scenario must reach an allocated-writer collision"
    print("STRING_ORSET_WASM_PARITY=true steps=%d errors=%d collisions=%d"
          % (len(STEPS), errors, verdicts.count("collision")))


if __name__ == "__main__":
    main(sys.argv[1:])
