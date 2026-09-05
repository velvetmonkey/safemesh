/-
SafeMesh — record admission and replay.
Copyright (C) 2026 Ben Cassie
SPDX-License-Identifier: Apache-2.0
-/
import SafeMesh.Delta

/-!
# Record lifecycle

Four theorems only: admission, idempotence, collision, replay agreement.
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
  · decide
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
