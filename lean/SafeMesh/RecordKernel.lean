/-
SafeMesh — record admission and replay.
Copyright (C) 2026 Ben Cassie
SPDX-License-Identifier: Apache-2.0
-/
import SafeMesh.Delta

/-!
# Record lifecycle

The M1 lifecycle has four theorems: admission, idempotence, collision, replay
agreement. The M2 ownership extension follows below.
The live state carries accepted-record provenance as well as its CRDT value:
an accepted delta can be below the current value, so value inequality alone
cannot characterize admission. Provenance is logical bookkeeping, not a second
physical copy required in Rust. The cached value is updated incrementally;
replay independently joins the log. Reachability does NOT assume their equality.

Records use full payload equality, not hash equality. Reject collisions with a
named outcome; do not mutate the log or live state. This is intentionally NOT
convergence for arbitrary colliding input sets: whichever payload arrives first
is accepted. Equal ACCEPTED sets do determine equal live states.
-/
namespace SafeMesh.RecordKernel

abbrev RecordId := ℕ × ℕ
abbrev Record (S : Type) := RecordId × S

inductive Outcome where
  | accepted
  | duplicate
  | collision
  deriving DecidableEq, Repr

variable {S : Type} [SemilatticeSup S] [OrderBot S] [DecidableEq S]

/-- Independent log and cached live state; arbitrary inconsistent states can
be represented, including the current Rust defect. -/
structure Replica (S : Type) where
  log : Finset (Record S)
  live : Finset (Record S) × S

/-- Deterministic rebuild from the accepted set. -/
def replay (log : Finset (Record S)) : Finset (Record S) × S :=
  (log, log.sup Prod.snd)

def empty : Replica S := ⟨∅, (∅, ⊥)⟩

/-- Compare the entire record before distinguishing an id collision. -/
def outcome (s : Replica S) (r : Record S) : Outcome :=
  if r ∈ s.log then .duplicate
  else if ∃ q ∈ s.log, q.1 = r.1 then .collision
  else .accepted

/-- Admission is the single decision controlling both mutations. -/
def step (s : Replica S) (r : Record S) : Replica S :=
  if outcome s r = .accepted then
    ⟨insert r s.log, (insert r s.live.1, s.live.2 ⊔ r.2)⟩
  else s

/-- Only initialization and the actual transition establish reachability. -/
inductive Reachable : Replica S → Prop where
  | empty : Reachable empty
  | step {s : Replica S} (r : Record S) : Reachable s → Reachable (step s r)

/-- 1. ADMISSION: both log change and live-state change are equivalent to
acceptance. The final clause pins the exact incremental payload application.
The provenance premise is supplied for every reachable state by theorem 4.
A value alone need not change when an accepted delta is redundant. -/
theorem admission (s : Replica S) (r : Record S) (hs : s.live.1 = s.log) :
    ((step s r).log ≠ s.log ↔ outcome s r = .accepted) ∧
    ((step s r).live ≠ s.live ↔ outcome s r = .accepted) ∧
    (step s r).live.2 =
      if outcome s r = .accepted then s.live.2 ⊔ r.2 else s.live.2 := by
  by_cases hr : r ∈ s.log
  · simp [step, outcome, hr]
  · by_cases hid : ∃ q ∈ s.log, q.1 = r.1
    · simp [step, outcome, hr, hid]
    · have hlog : insert r s.log ≠ s.log := Finset.insert_ne_self.mpr hr
      have hlive : (insert r s.live.1, s.live.2 ⊔ r.2) ≠ s.live := by
        intro h
        have hp := congrArg Prod.fst h
        rw [hs] at hp
        exact hlog hp
      simp [step, outcome, hr, hid, hlog, hlive]

/-- 2. IDEMPOTENCE: redelivery changes neither log nor live state, even for
an arbitrary initial cache. The outcome of redelivery may be duplicate. -/
theorem idempotence (s : Replica S) (r : Record S) :
    step (step s r) r = step s r := by
  by_cases hr : r ∈ s.log
  · simp [step, outcome, hr]
  · by_cases hid : ∃ q ∈ s.log, q.1 = r.1
    · simp [step, outcome, hr, hid]
    · simp [step, outcome, hr, hid]

/-- The old Rust transition: apply the payload regardless of whether the
id-only log accepts it. Restricted to one natural-number max-counter coordinate,
which is sufficient to express the measured product failure. -/
def legacyStep (s : Replica ℕ) (r : Record ℕ) : Replica ℕ :=
  let log := if ∃ q ∈ s.log, q.1 = r.1 then s.log else insert r s.log
  ⟨log, (log, s.live.2 ⊔ r.2)⟩

def legacyWitness : Replica ℕ :=
  legacyStep (legacyStep empty ((1, 1), 5)) ((1, 1), 9)

/-- Exact observed defect: first payload retained, second applied. -/
def RustDefect (s : Replica ℕ) : Prop :=
  s.log = {((1, 1), 5)} ∧ s.live.2 = 9 ∧ (replay s.log).2 = 5

/-- 4. REPLAY AGREES, with the negative product control in the same theorem.
Every reachable cache equals its independently rebuilt log. The legacy
transition really produces the defect, and no reachable state can exhibit it.
Listed before theorem 3 so that accepted-set determinism can reuse the proof. -/
theorem replayAgrees :
    (∀ (T : Type) [SemilatticeSup T] [OrderBot T] [DecidableEq T]
      (s : Replica T), Reachable s → replay s.log = s.live) ∧
    RustDefect legacyWitness ∧
    (∀ s : Replica ℕ, Reachable s → ¬ RustDefect s) := by
  have invariant : ∀ (T : Type) [SemilatticeSup T] [OrderBot T] [DecidableEq T]
      (s : Replica T), Reachable s → replay s.log = s.live := by
    intro T _ _ _ s h
    induction h with
    | empty => simp [empty, replay]
    | @step s r h ih =>
      by_cases ha : outcome s r = .accepted
      · have hcache : s.live = (s.log, s.log.sup Prod.snd) := ih.symm
        simp [step, ha, replay, hcache, Finset.sup_insert, sup_comm]
      · simpa [step, ha] using ih
  refine ⟨invariant, ?_, ?_⟩
  · -- Unfold the defect predicate and both legacy deliveries explicitly.
    -- The witness is checked by the kernel, without native evaluation.
    norm_num [RustDefect, legacyWitness, legacyStep, empty, replay]
  · intro s hs bug
    have eq := congrArg Prod.snd (invariant ℕ s hs)
    rw [bug.2.1, bug.2.2] at eq
    exact (by decide : (5 : ℕ) ≠ 9) eq

/-- 3. COLLISION: an existing id with a different payload has the explicit
collision outcome and no mutation (for an id-unique log). The second clause
is accepted-set determinism/order independence, including histories containing
rejections and duplicates: equal accepted sets imply equal live states.
No claim is made for equal *input* sets containing conflicting payloads. -/
theorem collision :
    (∀ (s : Replica S) (r q : Record S),
      (∀ a ∈ s.log, ∀ b ∈ s.log, a.1 = b.1 → a = b) →
      q ∈ s.log → q.1 = r.1 → q.2 ≠ r.2 →
      outcome s r = .collision ∧ step s r = s) ∧
    (∀ a b : Replica S, Reachable a → Reachable b →
      a.log = b.log → a.live = b.live) := by
  constructor
  · intro s r q unique hq hid different
    have hr : r ∉ s.log := by
      intro hr
      exact different (congrArg Prod.snd (unique q hq r hr hid))
    have hex : ∃ q ∈ s.log, q.1 = r.1 := ⟨q, hq, hid⟩
    simp [outcome, step, hr, hex]
  · intro a b ha hb logs
    rw [← replayAgrees.1 S a ha, ← replayAgrees.1 S b hb, logs]

#print axioms admission
#print axioms idempotence
#print axioms collision
#print axioms replayAgrees

end SafeMesh.RecordKernel

/-
RUST OBLIGATION
Compare an incoming id AND the full decoded payload against the stored record
before application. Return Accepted for a new id, Duplicate for an identical
record, and Collision for a different payload at an existing id. Surface
Collision to the caller; it must not be silently reported as successful dedup.
Only Accepted may atomically append the record and join its delta into live
state. Duplicate and Collision must leave both unchanged. Start empty, or
initialize the cache by replay; preserve one payload per id. Accepted provenance
in this model can be represented by the log/index itself, without another copy.
The payload interpretation must be a bottomed join-semilattice, with the same
join used in application and replay. Reject ill-formed payloads before admission.
Crash recovery must replay the committed log before serving live state; this
module proves atomic transitions, not storage durability or crash atomicity.
Version vectors, cursors, envelopes, authentication, overflow, and network
reporting of collisions are outside this four-theorem kernel. Do not infer
same-input-set convergence for conflicting ids: rejection makes the accepted
set depend on arrival order. Equal accepted sets DO give identical state.

WIRE FORMAT
No record-byte change is implied: id and payload already suffice. This changes
the admission API/semantics to expose Collision. A protocol that transmits that
outcome may need a separately specified response extension; no bytes are changed
or specified here. Naturals abstract bounded Rust identifiers and tallies.
-/

/-! ## Fixed ownership (M2 packet A)

The configured writers are `0 .. writers-1`. A counter coordinate belongs to
its same-numbered author. An add's token is `sequence * writers + author`;
record sequences are positive and bounded at the Rust boundary. A remove
references observed tokens from ANY author; it does not allocate a token.
The local-filesystem shell supplies lock ownership and the current generation.
This model proves the decision on those facts, not OS lock implementation or
storage durability. Rust transcribes these definitions under corpus conformance.
-/
namespace SafeMesh.RecordKernel

structure WriterConfig where
  writers : Nat
  writer : Nat
  deriving Repr

structure WriteContext where
  config : WriterConfig
  held : Bool
  generation : Nat
  currentGeneration : Nat
  localWrite : Bool
  deriving Repr

inductive OwnedPayload where
  | counter (coordinate : Nat)
  | add (token : Nat)
  | remove
  deriving Repr

def token (writers author sequence : Nat) : Nat := sequence * writers + author

def maxWord : Nat := 18446744073709551615

def nextSequence (last : Nat) : Option Nat :=
  if last < maxWord then some (last + 1) else none

theorem sequenceFresh (last next : Nat) (h : nextSequence last = some next) :
    last < next ∧ next ≤ maxWord := by
  unfold nextSequence at h
  split at h
  · rename_i hp
    simp only [Option.some.injEq] at h
    omega
  · contradiction

def allocateToken (writers author sequence : Nat) : Option Nat :=
  if author < writers ∧ 0 < sequence ∧ token writers author sequence ≤ maxWord
  then some (token writers author sequence) else none

def payloadOwned (writers : Nat) (id : RecordId) : OwnedPayload → Prop
  | .counter coordinate => coordinate = id.1
  | .add t => allocateToken writers id.1 id.2 = some t
  | .remove => True

instance (writers : Nat) (id : RecordId) (p : OwnedPayload) :
    Decidable (payloadOwned writers id p) := by
  cases p <;> unfold payloadOwned <;> infer_instance

def permitted (c : WriteContext) (id : RecordId) (p : OwnedPayload) : Prop :=
  c.config.writer < c.config.writers ∧ c.held = true ∧ 0 < c.generation ∧
  c.generation = c.currentGeneration ∧ id.1 < c.config.writers ∧ 0 < id.2 ∧
  (c.localWrite = true → id.1 = c.config.writer) ∧ payloadOwned c.config.writers id p

instance (c : WriteContext) (id : RecordId) (p : OwnedPayload) :
    Decidable (permitted c id p) := by unfold permitted; infer_instance

def refuses (c : WriteContext) (id : RecordId) (p : OwnedPayload) : Bool :=
  !decide (permitted c id p)

/-- The author, local writer and coordinate obligations follow from the exact
predicate used to generate the executable oracle. -/
theorem ownedWriter (c : WriteContext) (id : RecordId) (p : OwnedPayload)
    (h : refuses c id p = false) :
    id.1 < c.config.writers ∧ (c.localWrite = true → id.1 = c.config.writer) := by
  have hp : permitted c id p := by simpa [refuses] using h
  exact ⟨hp.2.2.2.2.1, hp.2.2.2.2.2.2.1⟩

theorem ownedCoordinate (c : WriteContext) (id : RecordId) (coordinate : Nat)
    (h : refuses c id (.counter coordinate) = false) : coordinate = id.1 := by
  have hp : permitted c id (.counter coordinate) := by simpa [refuses] using h
  exact hp.2.2.2.2.2.2.2

/-- Modulo recovers the configured author, so two distinct authors cannot mint
one token. This includes every sequence, not merely the sampled corpus. -/
theorem tokenOwner (n a s : Nat) (ha : a < n) : token n a s % n = a := by
  simp [token, Nat.add_mod, Nat.mod_eq_of_lt ha]

theorem tokenDisjoint (n a b s t : Nat) (ha : a < n) (hb : b < n)
    (hne : a ≠ b) : token n a s ≠ token n b t := by
  intro h
  have hm := congrArg (fun v => v % n) h
  dsimp at hm
  rw [tokenOwner n a s ha, tokenOwner n b t hb] at hm
  exact hne hm

/-- A writer also cannot reuse a token at two distinct record sequences. -/
theorem tokenSequence (n a s t : Nat) (hn : 0 < n)
    (h : token n a s = token n a t) : s = t := by
  have hm : s * n = t * n := by simpa [token] using h
  exact Nat.eq_of_mul_eq_mul_right hn hm

theorem allocatedOwned (n a s t : Nat) (h : allocateToken n a s = some t) :
    a < n ∧ 0 < s ∧ t = token n a s ∧ t ≤ maxWord := by
  unfold allocateToken at h
  split at h
  · rename_i hp
    simp only [Option.some.injEq] at h
    exact ⟨hp.1, hp.2.1, h.symm, h ▸ hp.2.2⟩
  · contradiction

theorem competingRefused (c : WriteContext) (id : RecordId) (p : OwnedPayload)
    (h : c.held = false) : refuses c id p = true := by
  simp [refuses, permitted, h]

theorem staleRefused (c : WriteContext) (id : RecordId) (p : OwnedPayload)
    (h : c.generation ≠ c.currentGeneration) : refuses c id p = true := by
  simp [refuses, permitted, h]

theorem foreignCoordinateRefused (c : WriteContext) (id : RecordId) (coord : Nat)
    (h : coord ≠ id.1) : refuses c id (.counter coord) = true := by
  simp [refuses, permitted, payloadOwned, h]

theorem foreignTokenRefused (c : WriteContext) (id : RecordId) (t : Nat)
    (h : allocateToken c.config.writers id.1 id.2 ≠ some t) :
    refuses c id (.add t) = true := by
  simp [refuses, permitted, payloadOwned, h]

/-- Validation wraps, and precedes, the SAME M1 step. The second component is
allocation metadata. Refusal leaves the entire pair byte-representable as-is. -/
def ownedStep {S : Type} [SemilatticeSup S] [OrderBot S] [DecidableEq S]
    (c : WriteContext) (p : OwnedPayload) (r : Record S)
    (s : Replica S × Nat) : Replica S × Nat :=
  if refuses c r.1 p then s else
    (step s.1 r, if outcome s.1 r = .accepted ∧ r.1.1 = c.config.writer
      then max s.2 r.1.2 else s.2)

theorem refusedUnchanged {S : Type} [SemilatticeSup S] [OrderBot S] [DecidableEq S]
    (c : WriteContext) (p : OwnedPayload) (r : Record S) (s : Replica S × Nat)
    (h : refuses c r.1 p = true) : ownedStep c p r s = s := by
  simp [ownedStep, h]

theorem ownedReachable {S : Type} [SemilatticeSup S] [OrderBot S] [DecidableEq S]
    (c : WriteContext) (p : OwnedPayload) (r : Record S) (s : Replica S × Nat)
    (h : Reachable s.1) : Reachable (ownedStep c p r s).1 := by
  unfold ownedStep
  split
  · exact h
  · exact Reachable.step r h

/-- The guard preserves the M1 equal-accepted-set convergence guarantee. -/
theorem ownedConvergence {S : Type} [SemilatticeSup S] [OrderBot S] [DecidableEq S]
    (a b : Replica S) (ha : Reachable a) (hb : Reachable b) (h : a.log = b.log) :
    a.live = b.live := collision.2 a b ha hb h

/-- Positive control: exactly the permitted domain passes the guard. -/
theorem validPermitted (c : WriteContext) (id : RecordId) (p : OwnedPayload)
    (h : permitted c id p) : refuses c id p = false := by simp [refuses, h]

end SafeMesh.RecordKernel

#print axioms SafeMesh.RecordKernel.sequenceFresh
#print axioms SafeMesh.RecordKernel.ownedWriter
#print axioms SafeMesh.RecordKernel.ownedCoordinate
#print axioms SafeMesh.RecordKernel.tokenOwner
#print axioms SafeMesh.RecordKernel.tokenDisjoint
#print axioms SafeMesh.RecordKernel.tokenSequence
#print axioms SafeMesh.RecordKernel.allocatedOwned
#print axioms SafeMesh.RecordKernel.competingRefused
#print axioms SafeMesh.RecordKernel.staleRefused
#print axioms SafeMesh.RecordKernel.foreignCoordinateRefused
#print axioms SafeMesh.RecordKernel.foreignTokenRefused
#print axioms SafeMesh.RecordKernel.refusedUnchanged
#print axioms SafeMesh.RecordKernel.ownedReachable
#print axioms SafeMesh.RecordKernel.ownedConvergence
#print axioms SafeMesh.RecordKernel.validPermitted
