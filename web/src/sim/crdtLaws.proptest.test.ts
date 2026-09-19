import { describe, expect, it } from 'vitest'
import fc from 'fast-check'
import {
  SafeMeshGCounterReplica,
  SafeMeshPnCounterReplica, SafeMeshRgaReplica, SafeMeshGSetReplica,
  SafeMeshEnableWinsFlagReplica, SafeMeshLwwRegisterReplica, SafeMeshLwwMapReplica,
  SafeMeshStringOrSetReplica,
} from '../../../rust/crates/safemesh-wasm/pkg/safemesh_wasm'

// Explicitly retain fast-check's 100-run default; every law gets fresh generated
// histories. Report the runner's actual count (and preserve its shrink/replay output).
const numRuns = 100
const peer = fc.integer({ min: 0, max: 2 })
const u64Max = (1n << 64n) - 1n
// appendBump takes an absolute tally, NOT an increment. Lower tallies are legal.
// Cover the entire u64 domain, especially JS precision and u64 total boundaries.
const tally = fc.oneof(
  fc.constantFrom(0n, 1n, (1n << 53n) - 1n, 1n << 53n, u64Max - 1n, u64Max),
  fc.bigInt({ min: 0n, max: u64Max }),
)
const unicode = fc.constantFrom('é', 'e\u0301', '水', '😀', '\u007f\u0080\u07ff\u0800\ud7ff\ue000\uffff\u{10000}\u{10ffff}')
const element = fc.oneof(fc.constant(''), unicode, fc.string({ unit: 'binary', maxLength: 24 }))
const counterHistory = fc.record({
  initial: fc.tuple(tally, tally, tally),
  operations: fc.array(fc.record({ peer, tally, observe: fc.option(peer, { nil: undefined }) }), { maxLength: 36 }),
})
const setHistory = fc.record({
  nonAscii: unicode,
  pool: fc.array(element, { minLength: 1, maxLength: 6 }),
  operations: fc.array(fc.record({
    peer, source: peer, index: fc.nat({ max: 20 }),
    kind: fc.constantFrom('add', 'remove', 'observe-remove', 'receive'),
  }), { maxLength: 36 }),
})

type Generated<A> = A extends fc.Arbitrary<infer T> ? T : never

type Replica = {
  free(): void
  logBytes(): Uint8Array
  mergeLogBytes(bytes: Uint8Array): unknown
  mergeRecordBytes(bytes: Uint8Array): unknown
}
type Scenario<R extends Replica> = { peers: R[]; records: Uint8Array[] }

// WASM handles (including addEntries' child handles) must be freed even on a
// failing/shrinking case. This also keeps generated runs independent.
function arena<R extends Replica>(create: (id: number) => R) {
  const handles: R[] = []
  return {
    make(id = 0) { const r = create(id); handles.push(r); return r },
    free() { for (const r of handles) r.free() },
  }
}

function counterScenario(input: Generated<typeof counterHistory>, make: (id: number) => SafeMeshGCounterReplica): Scenario<SafeMeshGCounterReplica> {
  const peers = [0, 1, 2].map(make)
  const records = peers.map((r, id) => r.appendBump(id, input.initial[id]))
  for (const op of input.operations) {
    if (op.observe !== undefined) peers[op.peer].mergeLogBytes(peers[op.observe].logBytes())
    records.push(peers[op.peer].appendBump(op.peer, op.tally))
  }
  return { peers, records }
}

function setScenario(input: Generated<typeof setHistory>, make: (id: number) => SafeMeshStringOrSetReplica): Scenario<SafeMeshStringOrSetReplica> {
  const peers = [0, 1, 2].map(make)
  const records: Uint8Array[] = []
  let token = 0n
  const add = (id: number, value: string) => records.push(peers[id].appendAdd(value, token++))
  const remove = (id: number, value: string) => records.push(peers[id].appendRemoveObserved(value))
  // Each case includes empty, repeated and non-ASCII elements, and both an
  // unobserved removal and an observed removal. The remaining history is random.
  add(0, '')
  add(1, input.nonAscii)
  add(2, input.nonAscii)
  remove(2, '')
  remove(0, '')
  const pool = ['', input.nonAscii, ...input.pool]
  for (const op of input.operations) {
    const value = pool[op.index % pool.length]
    if (op.kind === 'add') add(op.peer, value)
    if (op.kind === 'remove') remove(op.peer, value)
    if (op.kind === 'receive' || op.kind === 'observe-remove') {
      peers[op.peer].mergeLogBytes(peers[op.source].logBytes())
      if (op.kind === 'observe-remove') remove(op.peer, value)
    }
  }
  return { peers, records }
}

function counterState(r: SafeMeshGCounterReplica) {
  return { value: r.value(), state: Array.from(r.state()) }
}
function setState(r: SafeMeshStringOrSetReplica) {
  const entries = r.addEntries()
  try {
    // No state()/value() export exists for this wrapper. These three reads are
    // its complete carrier (including removed adds), not just visible membership.
    return {
      value: r.elements(),
      adds: entries.map(entry => [entry.element(), entry.token()]),
      tombstones: Array.from(r.tombstones()),
    }
  } finally { for (const entry of entries) entry.free() }
}

const laws = ['commutativity', 'associativity', 'idempotence', 'convergence'] as const
function checkLaws<T, R extends Replica>(
  name: string, history: fc.Arbitrary<T>, create: (id: number) => R,
  scenario: (input: T, make: (id: number) => R) => Scenario<R>, read: (r: R) => unknown,
) {
  describe(`${name}: compiled WASM merge laws`, () => {
    for (const law of laws) {
      it(`${law} (${numRuns} generated cases)`, () => {
        fc.assert(fc.property(history, fc.integer(), (input, deliverySeed) => {
          const scope = arena(create)
          try {
            const { peers, records } = scenario(input, scope.make)
            const logs = peers.map(r => r.logBytes())
            const from = (bytes: Uint8Array) => { const r = scope.make(); r.mergeLogBytes(bytes); return r }
            const join = (a: Uint8Array, b: Uint8Array) => {
              const r = from(a); r.mergeLogBytes(b); return r
            }
            if (law === 'commutativity') {
              for (const [a, b] of [[0, 1], [1, 2], [2, 0]]) {
                expect(read(join(logs[a], logs[b]))).toEqual(read(join(logs[b], logs[a])))
              }
            } else if (law === 'associativity') {
              const ab = join(logs[0], logs[1])
              const bc = join(logs[1], logs[2])
              expect(read(join(ab.logBytes(), logs[2]))).toEqual(read(join(logs[0], bc.logBytes())))
            } else if (law === 'idempotence') {
              for (const r of peers) {
                const before = read(r)
                r.mergeLogBytes(r.logBytes())
                expect(read(r)).toEqual(before)
              }
              const r = scope.make()
              for (const record of records) r.mergeRecordBytes(record)
              const before = read(r)
              for (const record of records) r.mergeRecordBytes(record)
              expect(read(r)).toEqual(before)
            } else {
              const expected = scope.make()
              for (const log of logs) expected.mergeLogBytes(log)
              // Three independently shuffled permutations of the SAME complete
              // record set; delivery seed is part of fast-check's counterexample.
              const orders = fc.sample(fc.shuffledSubarray(records, {
                minLength: records.length, maxLength: records.length,
              }), { seed: deliverySeed, numRuns: 3 })
              for (let id = 0; id < 3; id++) {
                for (const record of orders[id]) peers[id].mergeRecordBytes(record)
                expect(read(peers[id])).toEqual(read(expected))
                const fresh = scope.make()
                for (const record of orders[id]) fresh.mergeRecordBytes(record)
                expect(read(fresh)).toEqual(read(expected))
              }
            }
          } finally { scope.free() }
        }), {
          numRuns,
          reporter(details) {
            console.info(`${name} ${law}: ${details.numRuns} cases, ${details.numShrinks} shrinks, seed ${details.seed}`)
            // The default formatter includes seed, path and shrunk input.
            if (details.failed) throw new Error(fc.defaultReportMessage(details) ?? 'Property failed', { cause: details.errorInstance })
            expect(details.numRuns).toBe(numRuns)
          },
        })
      })
    }
  })
}

checkLaws('GCounter', counterHistory, id => new SafeMeshGCounterReplica(BigInt(id), 3), counterScenario, counterState)
checkLaws('StringOrSet', setHistory, id => new SafeMeshStringOrSetReplica(BigInt(id)), setScenario, setState)

// Coverage: GCounter, GSet, PnCounter, OrSet (StringOrSet), Rga,
// EnableWinsFlag, LwwRegister, LwwMap. All state transitions run in compiled WASM.
const small = fc.bigInt({ min: 0n, max: 7n })
const dotTime = fc.oneof(small, tally)
const pnHistory = fc.array(fc.record({ peer, increment: fc.boolean(), tally, source: peer }), { minLength: 3, maxLength: 36 })
function pnScenario(input: Generated<typeof pnHistory>, make: (id: number) => SafeMeshPnCounterReplica) {
  const peers = [0, 1, 2].map(make)
  const records = input.map(op => {
    peers[op.peer].mergeLogBytes(peers[op.source].logBytes())
    return op.increment ? peers[op.peer].appendInc(op.peer, op.tally) : peers[op.peer].appendDec(op.peer, op.tally)
  })
  return { peers, records }
}
checkLaws('PnCounter', pnHistory, id => new SafeMeshPnCounterReplica(BigInt(id), 3), pnScenario,
  r => ({ value: r.value(), state: Array.from(r.state()) }))

const flagHistory = fc.array(fc.record({ peer, source: peer, enable: fc.boolean(), token: small }), { minLength: 3, maxLength: 36 })
function flagScenario(input: Generated<typeof flagHistory>, make: (id: number) => SafeMeshEnableWinsFlagReplica) {
  const peers = [0, 1, 2].map(make)
  const records = input.map(op => {
    peers[op.peer].mergeLogBytes(peers[op.source].logBytes())
    return op.enable ? peers[op.peer].appendEnable(op.token) : peers[op.peer].appendDisableObserved()
  })
  return { peers, records }
}
checkLaws('EnableWinsFlag', flagHistory, id => new SafeMeshEnableWinsFlagReplica(BigInt(id)), flagScenario,
  r => ({ value: r.value(), enables: Array.from(r.enabledTokens()), tombstones: Array.from(r.tombstoneTokens()) }))

const registerHistory = fc.array(fc.record({ peer, timestamp: dotTime, writer: small, value: tally }), { minLength: 3, maxLength: 36 })
function registerScenario(input: Generated<typeof registerHistory>, make: (id: number) => SafeMeshLwwRegisterReplica) {
  const peers = [0, 1, 2].map(make)
  const records = input.map(op => peers[op.peer].appendSet(op.timestamp, op.writer, op.value))
  return { peers, records }
}
checkLaws('LwwRegister', registerHistory, id => new SafeMeshLwwRegisterReplica(BigInt(id)), registerScenario,
  r => ({ has: r.hasValue(), value: r.valueOr(0n), timestamp: r.timestampOr(0n), writer: r.writerReplicaOr(0n) }))

const mapHistory = fc.array(fc.record({ peer, key: small, timestamp: dotTime, writer: small, value: tally, remove: fc.boolean() }), { minLength: 3, maxLength: 36 })
function mapScenario(input: Generated<typeof mapHistory>, make: (id: number) => SafeMeshLwwMapReplica) {
  const peers = [0, 1, 2].map(make)
  const records = input.map(op => op.remove
    ? peers[op.peer].appendRemove(op.key, op.timestamp, op.writer)
    : peers[op.peer].appendSet(op.key, op.timestamp, op.writer, op.value))
  return { peers, records }
}
checkLaws('LwwMap', mapHistory, id => new SafeMeshLwwMapReplica(BigInt(id)), mapScenario,
  r => Array.from(r.stateBytes()))

// These carriers expose canonical state snapshots, not EventLog records. Adapt
// only the transport names: checkLaws still performs the same joins and delivery
// permutations, with the complete native carrier bytes as its state oracle.
function snapshotReplica<R extends { free(): void; stateBytes(): Uint8Array; mergeStateBytes(bytes: Uint8Array): void }>(native: R) {
  return {
    native,
    free: () => native.free(),
    logBytes: () => native.stateBytes(),
    mergeLogBytes: (bytes: Uint8Array) => native.mergeStateBytes(bytes),
    mergeRecordBytes: (bytes: Uint8Array) => native.mergeStateBytes(bytes),
  }
}
const rgaHistory = fc.array(fc.record({ peer, position: small, value: tally, remove: fc.boolean() }), { minLength: 3, maxLength: 36 })
const makeRga = () => snapshotReplica(new SafeMeshRgaReplica())
function rgaScenario(input: Generated<typeof rgaHistory>, make: (id: number) => ReturnType<typeof makeRga>) {
  const peers = [0, 1, 2].map(make)
  const records = input.map(op => op.remove
    ? peers[op.peer].native.delete(op.position)
    : peers[op.peer].native.insert(op.position, op.value))
  return { peers, records }
}
checkLaws('Rga', rgaHistory, makeRga, rgaScenario, r => Array.from(r.logBytes()))
const gsetHistory = fc.array(fc.record({ peer, value: fc.oneof(small, tally) }), { minLength: 3, maxLength: 36 })
const makeGSet = () => snapshotReplica(new SafeMeshGSetReplica())
function gsetScenario(input: Generated<typeof gsetHistory>, make: (id: number) => ReturnType<typeof makeGSet>) {
  const peers = [0, 1, 2].map(make)
  const records = input.map(op => peers[op.peer].native.insert(op.value))
  return { peers, records }
}
checkLaws('GSet', gsetHistory, makeGSet, gsetScenario, r => Array.from(r.logBytes()))
