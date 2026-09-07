/-
SafeMesh — delta-state CRDT convergence, built on crdt-lean.
Copyright (C) 2026 Ben Cassie
SPDX-License-Identifier: Apache-2.0

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

/-- info: 'SafeMesh.deltaORSet_adds' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaORSet_adds

/-- info: 'SafeMesh.deltaORSet_tombs' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaORSet_tombs

/-- info: 'SafeMesh.deltaORSet_matches_full' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaORSet_matches_full

/-- info: 'SafeMesh.deltaORSet_elements_match' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaORSet_elements_match

/-- info: 'SafeMesh.deltaORSet_lookup_match' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaORSet_lookup_match

/-- info: 'SafeMesh.deltaORSet_lookup' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaORSet_lookup

/-- info: 'SafeMesh.deltaRGA_placed' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaRGA_placed

/-- info: 'SafeMesh.deltaRGA_tombs' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaRGA_tombs

/-- info: 'SafeMesh.deltaRGA_matches_full' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaRGA_matches_full

/-- info: 'SafeMesh.deltaRGA_read_match' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaRGA_read_match

/-- info: 'SafeMesh.deltaRGA_read_sec' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaRGA_read_sec

/-- info: 'SafeMesh.deltaRGA_read_mem' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaRGA_read_mem

/-- info: 'SafeMesh.deltaRGA_read_sorted' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.deltaRGA_read_sorted

def main : IO Unit :=
  IO.println "axiom gate passed: all checks pinned by #guard_msgs at compile time"

-- Record lifecycle and packet-A ownership: extend the existing axiom gate.
/-- info: 'SafeMesh.RecordKernel.admission' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.RecordKernel.admission

/-- info: 'SafeMesh.RecordKernel.idempotence' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.RecordKernel.idempotence

/-- info: 'SafeMesh.RecordKernel.collision' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.RecordKernel.collision

/-- info: 'SafeMesh.RecordKernel.replayAgrees' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.RecordKernel.replayAgrees

/-- info: 'SafeMesh.RecordKernel.sequenceFresh' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.RecordKernel.sequenceFresh

/-- info: 'SafeMesh.RecordKernel.ownedWriter' depends on axioms: [propext] -/
#guard_msgs in #print axioms SafeMesh.RecordKernel.ownedWriter

/-- info: 'SafeMesh.RecordKernel.ownedCoordinate' depends on axioms: [propext] -/
#guard_msgs in #print axioms SafeMesh.RecordKernel.ownedCoordinate

/-- info: 'SafeMesh.RecordKernel.tokenOwner' depends on axioms: [propext] -/
#guard_msgs in #print axioms SafeMesh.RecordKernel.tokenOwner

/-- info: 'SafeMesh.RecordKernel.tokenDisjoint' depends on axioms: [propext] -/
#guard_msgs in #print axioms SafeMesh.RecordKernel.tokenDisjoint

/-- info: 'SafeMesh.RecordKernel.tokenSequence' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.RecordKernel.tokenSequence

/-- info: 'SafeMesh.RecordKernel.allocatedOwned' depends on axioms: [propext] -/
#guard_msgs in #print axioms SafeMesh.RecordKernel.allocatedOwned

/-- info: 'SafeMesh.RecordKernel.competingRefused' depends on axioms: [propext] -/
#guard_msgs in #print axioms SafeMesh.RecordKernel.competingRefused

/-- info: 'SafeMesh.RecordKernel.staleRefused' depends on axioms: [propext] -/
#guard_msgs in #print axioms SafeMesh.RecordKernel.staleRefused

/-- info: 'SafeMesh.RecordKernel.foreignCoordinateRefused' depends on axioms: [propext] -/
#guard_msgs in #print axioms SafeMesh.RecordKernel.foreignCoordinateRefused

/-- info: 'SafeMesh.RecordKernel.foreignTokenRefused' depends on axioms: [propext] -/
#guard_msgs in #print axioms SafeMesh.RecordKernel.foreignTokenRefused

/-- info: 'SafeMesh.RecordKernel.refusedUnchanged' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.RecordKernel.refusedUnchanged

/-- info: 'SafeMesh.RecordKernel.ownedReachable' depends on axioms: [propext, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.RecordKernel.ownedReachable

/-- info: 'SafeMesh.RecordKernel.ownedConvergence' depends on axioms: [propext, Classical.choice, Quot.sound] -/
#guard_msgs in #print axioms SafeMesh.RecordKernel.ownedConvergence

/-- info: 'SafeMesh.RecordKernel.validPermitted' depends on axioms: [propext] -/
#guard_msgs in #print axioms SafeMesh.RecordKernel.validPermitted
