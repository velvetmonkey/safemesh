import { SafeMeshGCounterReplica } from '../../../rust/crates/safemesh-wasm/pkg/safemesh_wasm'
import { describe, expect, it } from 'vitest'
import {
  addElement,
  bumpCounter,
  convergence,
  createSimulation,
  dropCounterPacketToPeer,
  dropNextPacket,
  duplicateNextPacket,
  queueAntiEntropyPackets,
  removeElement,
  reorderQueue,
  runAntiEntropyNow,
  setAntiEntropyMs,
  setDropRate,
  setPartitioned,
  tick,
} from './simulation'

describe('mesh simulation', () => {
  it.each([0, 5000])('Heal reconnects and repairs immediately with interval %i', (interval) => {
    let sim = setPartitioned(setAntiEntropyMs(setDropRate(createSimulation(2), 0), interval), true)
    sim = dropNextPacket(bumpCounter(sim, 0))
    expect(convergence(sim).gcounterValues).toEqual([1, 0])

    sim = runAntiEntropyNow(setPartitioned(sim, false))

    expect(sim.partitioned).toBe(false)
    expect(convergence(sim).gcounterValues).toEqual([1, 1])
    expect(sim.antiEntropyMs).toBe(interval)
    expect(sim.now).toBe(0)
    expect(sim.nextAntiEntropyAt).toBe(interval || Number.POSITIVE_INFINITY)
  })

  it.each([0, 5000])('manual repair respects a partition with interval %i', (interval) => {
    let sim = setPartitioned(setAntiEntropyMs(createSimulation(2), interval), true)
    sim = dropNextPacket(bumpCounter(sim, 0))
    sim = runAntiEntropyNow(sim)
    expect(convergence(sim).gcounterValues).toEqual([1, 0])
    expect(sim.antiEntropyMs).toBe(interval)
  })

  it('runs scheduled repair at each deadline, never before it', () => {
    let sim = setAntiEntropyMs(setDropRate(createSimulation(2), 0), 5000)
    sim = dropNextPacket(bumpCounter(sim, 0))
    sim = tick(sim, 4999)
    expect(convergence(sim).gcounterValues).toEqual([1, 0])
    sim = tick(sim, 1)
    expect(convergence(sim).gcounterValues).toEqual([1, 1])
    expect(sim.nextAntiEntropyAt).toBe(10000)
    sim = dropNextPacket(bumpCounter(sim, 0))
    sim = tick(sim, 4999)
    expect(convergence(sim).gcounterValues).toEqual([2, 1])
    sim = tick(sim, 1)
    expect(convergence(sim).gcounterValues).toEqual([2, 2])
    expect(sim.nextAntiEntropyAt).toBe(15000)
  })

  it('keeps scheduled repair disabled before and after Heal', () => {
    let sim = setAntiEntropyMs(setDropRate(createSimulation(2), 0), 0)
    sim = dropNextPacket(bumpCounter(sim, 0))
    sim = tick(sim, 60000)
    expect(convergence(sim).gcounterValues).toEqual([1, 0])
    sim = runAntiEntropyNow(setPartitioned(sim, false))
    expect(convergence(sim).gcounterValues).toEqual([1, 1])
    sim = dropNextPacket(bumpCounter(sim, 0))
    sim = tick(sim, 60000)
    expect(convergence(sim).gcounterValues).toEqual([2, 1])
    expect(sim.antiEntropyMs).toBe(0)
    expect(sim.nextAntiEntropyAt).toBe(Number.POSITIVE_INFINITY)
  })

  it('defers scheduled repair across a partition until reconnect', () => {
    let sim = setPartitioned(setAntiEntropyMs(createSimulation(2), 5000), true)
    sim = dropNextPacket(bumpCounter(sim, 0))
    sim = tick(sim, 5000)
    expect(convergence(sim).gcounterValues).toEqual([1, 0])
    sim = tick(setPartitioned(sim, false), 0)
    expect(convergence(sim).gcounterValues).toEqual([1, 1])
    expect(sim.nextAntiEntropyAt).toBe(10000)
  })

  it('diverges under partition and converges after reconnect', () => {
    let sim = setDropRate(createSimulation(4), 0)
    sim = setPartitioned(sim, true)
    sim = bumpCounter(sim, 0)
    sim = addElement(sim, 1, 'medkit')
    sim = tick(sim, 2000, () => 1)

    expect(convergence(sim).sameRawState).toBe(false)
    expect(sim.queue.length).toBeGreaterThan(0)

    sim = setPartitioned(sim, false)
    for (let i = 0; i < 20; i += 1) {
      sim = tick(sim, 250, () => 1)
    }

    const status = convergence(sim)
    expect(status.sameRawState).toBe(true)
    expect(status.converged).toBe(true)
    expect(status.gcounterValue).toBe(1)
    expect(status.orsetElements).toEqual(['medkit'])
  })

  it('preserves OR-Set add-wins through transport delivery', () => {
    let sim = setDropRate(createSimulation(3), 0)
    sim = addElement(sim, 0, 'relay')
    for (let i = 0; i < 10; i += 1) sim = tick(sim, 250, () => 1)
    sim = setPartitioned(sim, true)
    sim = removeElement(sim, 1, 'relay')
    sim = addElement(sim, 2, 'relay')
    sim = setPartitioned(sim, false)
    for (let i = 0; i < 20; i += 1) sim = tick(sim, 250, () => 1)

    expect(convergence(sim).orsetElements).toEqual(['relay'])
  })

  it('recovers a dropped delta with anti-entropy state merge', () => {
    let sim = setAntiEntropyMs(setDropRate(createSimulation(4), 1), 0)
    sim = addElement(sim, 0, 'medkit')
    sim = tick(sim, 2000, () => 0)

    expect(sim.queue).toHaveLength(0)
    expect(convergence(sim).sameRawState).toBe(false)

    sim = setAntiEntropyMs(sim, 5000)
    sim = runAntiEntropyNow(sim)

    const status = convergence(sim)
    expect(status.sameRawState).toBe(true)
    expect(status.converged).toBe(true)
    expect(status.orsetElements).toEqual(['medkit'])
  })

  it('can manually drop the next queued delta', () => {
    let sim = setDropRate(createSimulation(3), 0)
    sim = addElement(sim, 0, 'insulin')

    expect(sim.queue).toHaveLength(2)
    sim = dropNextPacket(sim)

    expect(sim.queue).toHaveLength(1)
    expect(sim.log[0].technical).toContain('operator dropped')
  })

  it('can queue visible anti-entropy repair packets before state changes', () => {
    let sim = setDropRate(createSimulation(4), 0)
    sim = addElement(sim, 0, 'insulin')
    sim = bumpCounter(sim, 0)
    sim = dropCounterPacketToPeer(sim, 3)
    sim = tick(sim, 2000, () => 1)

    expect(convergence(sim).sameRawState).toBe(false)
    expect(sim.peers[3].gcounter[0]).toBe(0)

    sim = queueAntiEntropyPackets(sim)

    expect(sim.queue.some((packet) => packet.phase === 'repair' && packet.to === 3)).toBe(true)
    expect(sim.peers[3].gcounter[0]).toBe(0)

    sim = tick(sim, 3000, () => 1)

    const status = convergence(sim)
    expect(status.converged).toBe(true)
    expect(status.gcounterValue).toBe(1)
    expect(status.orsetElements).toEqual(['insulin'])
  })

  it('can duplicate a queued delta without changing convergence', () => {
    let sim = setDropRate(createSimulation(3), 0)
    sim = addElement(sim, 0, 'insulin')
    sim = duplicateNextPacket(sim)

    expect(sim.queue.some((packet) => packet.duplicated)).toBe(true)
    for (let i = 0; i < 12; i += 1) sim = tick(sim, 250, () => 1)

    const status = convergence(sim)
    expect(status.converged).toBe(true)
    expect(status.orsetElements).toEqual(['insulin'])
  })

  it('can reorder queued deltas and still converge', () => {
    let sim = setDropRate(createSimulation(4), 0)
    sim = addElement(sim, 0, 'medkit')
    sim = addElement(sim, 1, 'water')
    const before = sim.queue.map((packet) => packet.id)

    sim = reorderQueue(sim)
    const after = sim.queue.map((packet) => packet.id)

    expect(after).not.toEqual(before)
    for (let i = 0; i < 20; i += 1) sim = tick(sim, 250, () => 1)

    const status = convergence(sim)
    expect(status.converged).toBe(true)
    expect(status.orsetElements).toEqual(['medkit', 'water'])
  })

  it('reports per-peer core-state matches separately from equal reads', () => {
    let sim = setPartitioned(setDropRate(createSimulation(4), 0), true)
    for (const id of [0, 1, 2, 3]) sim = bumpCounter(sim, id)
    sim = tick(sim, 2000, () => 1)

    const status = convergence(sim)
    // Four carrier vectors that each sum to 1: every read is equal, no core state is.
    expect(status.gcounterValues).toEqual([1, 1, 1, 1])
    expect(status.readMatches).toEqual([true, true, true, true])
    expect(status.rawStateMatches).toEqual([true, false, false, false])
    expect(status.sameReads).toBe(true)
    expect(status.sameRawState).toBe(false)
    expect(status.converged).toBe(false)
  })

  it('does not run anti-entropy while partitioned, then reconciles after reconnect', () => {
    let sim = setAntiEntropyMs(createSimulation(3), 5000)
    sim = setPartitioned(sim, true)
    sim = addElement(sim, 0, 'water')
    sim = runAntiEntropyNow(sim)

    expect(convergence(sim).sameRawState).toBe(false)

    sim = setPartitioned(sim, false)
    sim = runAntiEntropyNow(sim)

    expect(convergence(sim).sameRawState).toBe(true)
  })
})

describe('wide counter reads', () => {
  function wideSimulation(tally: bigint) {
    const sim = createSimulation(2)
    const source = new SafeMeshGCounterReplica(0n, 2)
    source.appendBump(0, tally)
    sim.peers[0].gcounterLog = source.logBytes()
    source.free()
    return sim
  }

  it('reports an unrepresentable displayed total instead of rounding it', () => {
    expect(convergence(wideSimulation(9007199254740991n)).gcounterValue).toBe(9007199254740991)
    expect(() => convergence(wideSimulation(9007199254740993n))).toThrow(RangeError)
  })

  it('refuses a rounded anti-entropy label and a rounded state snapshot', () => {
    const sim = wideSimulation(9007199254740993n)
    expect(() => queueAntiEntropyPackets(sim)).toThrow(RangeError)
    expect(() => runAntiEntropyNow(sim)).toThrow(RangeError)
  })
})
