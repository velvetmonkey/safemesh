export type ReplicaId = number

export type GCounterDelta = {
  kind: 'gcounter.bump'
  replica: ReplicaId
  tally: number
}

export type GCounterState = number[]

function checkNatural(value: number, name: string): void {
  if (!Number.isInteger(value) || value < 0) {
    throw new RangeError(`${name} must be a finite nonnegative integer`)
  }
}

export function bottomGCounter(replicas: number): GCounterState {
  checkNatural(replicas, 'replicas')
  return Array.from({ length: replicas }, () => 0)
}

// Mirrors SafeMesh.deltaBump / SafeMesh.deltaGCounter_correct:
// a bump is one replica coordinate asserting its current grow-only tally.
export function bumpDelta(replica: ReplicaId, tally: number): GCounterDelta {
  checkNatural(replica, 'replica')
  checkNatural(tally, 'tally')
  return { kind: 'gcounter.bump', replica, tally }
}

// Mirrors SafeMesh.delta_dissemination_sec and merge_deltaState:
// applying a delta is joining it into the carrier, here pointwise max.
export function applyGCounterDelta(
  state: GCounterState,
  delta: GCounterDelta,
): GCounterState {
  checkNatural(delta.replica, 'replica')
  checkNatural(delta.tally, 'tally')
  const next = [...state]
  if (delta.replica >= 0 && delta.replica < next.length) {
    next[delta.replica] = Math.max(next[delta.replica], delta.tally)
  }
  return next
}

// Mirrors the full-state G-Counter join used by Crdt.merge.
export function mergeGCounter(left: GCounterState, right: GCounterState): GCounterState {
  if (left.length !== right.length) {
    throw new RangeError('G-Counter merge requires equal replica counts')
  }
  return left.map((value, index) => Math.max(value, right[index]))
}

// This numeric reference API refuses coordinates or totals that cannot be read exactly.
// Callers (including PN-Counter subtraction) retain a number result on success.
export function readGCounter(state: GCounterState): number {
  return state.reduce((sum, value) => {
    if (!Number.isSafeInteger(value) || value < 0) {
      throw new RangeError('G-Counter read requires safe nonnegative integer coordinates')
    }
    if (value > Number.MAX_SAFE_INTEGER - sum) {
      throw new RangeError('G-Counter total exceeds the safe integer range')
    }
    return sum + value
  }, 0)
}

export function sameGCounter(left: GCounterState, right: GCounterState): boolean {
  return left.length === right.length && left.every((value, index) => value === right[index])
}
