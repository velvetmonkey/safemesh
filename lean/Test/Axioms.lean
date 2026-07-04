/-
SafeMesh — delta-state CRDT convergence, built on crdt-lean.
Copyright (C) 2026 Ben Cassie
SPDX-License-Identifier: AGPL-3.0-or-later

Axiom gate: every public theorem is pinned to the clean baseline
{propext, Classical.choice, Quot.sound} (or fewer) via #guard_msgs, which
fails elaboration on ANY drift — new axioms, sorry (sorryAx), or native_decide
(Lean.ofReduceBool / Lean.trustCompiler). This module is a defaultTarget:
`lake build` elaborates it, so the gate cannot silently not-run.
-/
import SafeMesh

/-- info: 'SafeMesh.deltaState_eq_replicaState' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaState_eq_replicaState

/-- info: 'SafeMesh.deltaApply_eq_deltaState' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaApply_eq_deltaState

/-- info: 'SafeMesh.delta_dissemination_sec' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.delta_dissemination_sec

/-- info: 'SafeMesh.delta_matches_state' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.delta_matches_state

/-- info: 'SafeMesh.merge_deltaState' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.merge_deltaState

/-- info: 'SafeMesh.deltaGCounter_correct' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaGCounter_correct

/-- info: 'SafeMesh.deltaGCounter_matches_full' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaGCounter_matches_full

/-- info: 'SafeMesh.deltaState_fst' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaState_fst

/-- info: 'SafeMesh.deltaState_snd' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaState_snd

/-- info: 'SafeMesh.deltaPNCounter_correct_P' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaPNCounter_correct_P

/-- info: 'SafeMesh.deltaPNCounter_correct_N' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaPNCounter_correct_N

/-- info: 'SafeMesh.deltaPNCounter_matches_full' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaPNCounter_matches_full

/-- info: 'SafeMesh.deltaPNCounter_value_matches' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaPNCounter_value_matches

def main : IO Unit :=
  IO.println "axiom gate passed: all checks pinned by #guard_msgs at compile time"
