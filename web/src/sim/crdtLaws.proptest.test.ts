import { describe, expect, it } from 'vitest'
import fc from 'fast-check'
import {
  SafeMeshGCounterReplica,
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

type Replica = SafeMeshGCounterReplica | SafeMeshStringOrSetReplica
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
