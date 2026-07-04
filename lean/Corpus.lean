/-
SafeMesh — delta-state CRDT convergence, built on crdt-lean.
Copyright (C) 2026 Ben Cassie
SPDX-License-Identifier: AGPL-3.0-or-later

Conformance-corpus emitter (the Lean side of the differential VGD bridge).

Instantiates the PROVEN counter definitions at `ι = Fin N` (concrete,
computable) and prints a deterministic JSON corpus: for each case, the input
bump sequence in delivery order and the expected converged output
(per-coordinate state + counter value), computed by the machine-checked defs
(`SafeMesh.deltaApply`, `SafeMesh.deltaBump`, `SafeMesh.deltaBumpP/N`,
`Crdt.gcounterValue`, `Crdt.pncounterValue`). Fixed sample set, no RNG.

TCB note (honest): this exe runs the definitions through Lean's COMPILER, not
the kernel — corpus generation trusts compiled evaluation the same way
`native_decide` would. The theorems themselves stay kernel-checked and this
module is OUTSIDE the proof path: it is not in `defaultTargets`, is imported
by nothing, and the axiom gate does not touch it.
-/
import SafeMesh

open SafeMesh

/-- Replica count for the whole corpus — small enough to eyeball, big enough
for distinct-coordinate interactions. -/
abbrev N : Nat := 4

abbrev GC := Crdt.GCounter (Fin N)
abbrev PNC := Crdt.PNCounter (Fin N)

/-! ## G-Counter cases -/

structure GCase where
  name : String
  /-- Bumps in DELIVERY ORDER: (replica, asserted tally). -/
  bumps : List (Fin N × Nat)

/-- The corpus deliberately includes permutations and redeliveries of the same
bump-set: the proven order/duplicate-insensitivity (`delta_dissemination_sec`)
means their expected outputs are identical — and the Rust side must agree. -/
def gCases : List GCase := [
  ⟨"g_empty", []⟩,
  ⟨"g_single", [(0, 5)]⟩,
  ⟨"g_two_replicas", [(0, 3), (2, 7)]⟩,
  ⟨"g_monotone_same_replica", [(1, 1), (1, 4), (1, 2)]⟩,
  ⟨"g_all_replicas", [(0, 10), (1, 20), (2, 30), (3, 40)]⟩,
  ⟨"g_perm_a", [(0, 2), (1, 9), (3, 6)]⟩,
  ⟨"g_perm_b", [(3, 6), (0, 2), (1, 9)]⟩,
  ⟨"g_redelivery", [(2, 8), (2, 8), (2, 8)]⟩,
  ⟨"g_mixed_order_dupes", [(1, 5), (0, 1), (1, 5), (3, 3), (0, 4)]⟩
]

def runG (c : GCase) : GC :=
  deltaApply (c.bumps.map fun b => deltaBump b.1 b.2)

/-! ## PN-Counter cases -/

inductive PNOp where
  | inc (r : Fin N) (v : Nat)
  | dec (r : Fin N) (v : Nat)

structure PNCase where
  name : String
  ops : List PNOp

def pnCases : List PNCase := [
  ⟨"pn_empty", []⟩,
  ⟨"pn_inc_only", [.inc 0 5, .inc 2 3]⟩,
  ⟨"pn_dec_only", [.dec 1 4]⟩,
  ⟨"pn_net_positive", [.inc 0 10, .dec 0 4]⟩,
  ⟨"pn_net_negative", [.inc 1 2, .dec 2 9]⟩,
  ⟨"pn_perm_a", [.inc 0 7, .dec 1 3, .inc 2 5]⟩,
  ⟨"pn_perm_b", [.inc 2 5, .inc 0 7, .dec 1 3]⟩,
  ⟨"pn_redelivery", [.inc 3 6, .dec 3 6, .inc 3 6]⟩,
  ⟨"pn_all_coords", [.inc 0 1, .inc 1 2, .inc 2 3, .inc 3 4, .dec 0 1, .dec 3 2]⟩
]

def pnDelta : PNOp → PNC
  | .inc r v => deltaBumpP r v
  | .dec r v => deltaBumpN r v

def runPN (c : PNCase) : PNC :=
  deltaApply (c.ops.map pnDelta)

/-! ## JSON emission (hand-rolled: deterministic, zero deps, ASCII-only) -/

def natList (l : List Nat) : String :=
  "[" ++ String.intercalate "," (l.map toString) ++ "]"

def coords (f : Fin N → Nat) : List Nat :=
  (List.finRange N).map f

def gBumpJson (b : Fin N × Nat) : String :=
  s!"[{b.1.val},{b.2}]"

def gCaseJson (c : GCase) : String :=
  let s := runG c
  s!"\{\"name\":\"{c.name}\",\"n\":{N}," ++
  s!"\"bumps\":[{String.intercalate "," (c.bumps.map gBumpJson)}]," ++
  s!"\"expected_state\":{natList (coords s)}," ++
  s!"\"expected_value\":{Crdt.gcounterValue s}}"

def pnOpJson : PNOp → String
  | .inc r v => s!"[\"inc\",{r.val},{v}]"
  | .dec r v => s!"[\"dec\",{r.val},{v}]"

def pnCaseJson (c : PNCase) : String :=
  let s := runPN c
  s!"\{\"name\":\"{c.name}\",\"n\":{N}," ++
  s!"\"ops\":[{String.intercalate "," (c.ops.map pnOpJson)}]," ++
  s!"\"expected_p\":{natList (coords s.1)}," ++
  s!"\"expected_n\":{natList (coords s.2)}," ++
  s!"\"expected_value\":{Crdt.pncounterValue s}}"

def corpusJson : String :=
  "{\"schema\":\"safemesh-conformance-v1\",\"replicas\":" ++ toString N ++
  ",\"gcounter\":[\n  " ++
  String.intercalate ",\n  " (gCases.map gCaseJson) ++
  "\n],\"pncounter\":[\n  " ++
  String.intercalate ",\n  " (pnCases.map pnCaseJson) ++
  "\n]}"

def main : IO Unit := IO.println corpusJson
