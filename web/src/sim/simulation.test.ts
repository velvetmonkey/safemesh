import { SafeMeshGCounterReplica } from '../../../rust/crates/safemesh-wasm/pkg/safemesh_wasm'
import { describe, expect, it, vi } from 'vitest'
import {
  addElement,
  bumpCounter,
  convergence,
  createSimulation,
  MAX_SIMULATION_PEERS,
  dropCounterPacketToPeer,
  dropNextPacket,
  duplicateNextPacket,
  queueAntiEntropyPackets,
  readORSet,
  removeElement,
  reorderQueue,
  runAntiEntropyNow,
  setAntiEntropyMs,
  setDropRate,
  setLatency,
  setPeerCount,
  type Simulation,
  setPartitioned,
  tick,
} from './simulation'

describe('mesh simulation', () => {
  it.each(['manual', 'periodic'])('%s repair cannot bypass total packet loss', (mode) => {
    let sim = setDropRate(createSimulation(2), 1)
    sim = addElement(bumpCounter(sim, 0), 0, 'water')
    sim = tick(sim, 2000, () => 0.5)
    expect(sim.queue).toHaveLength(0)
    expect(convergence(sim).sameRawState).toBe(false)

    for (let round = 0; round < 5; round += 1) {
      sim = mode === 'manual' ? runAntiEntropyNow(sim) : tick(sim, 5000, () => 0.5)
      expect(convergence(sim).gcounterValues).toEqual([1, 0])
      expect(readORSet(sim.peers[1].orset)).toEqual([])
      expect(sim.queue.some((packet) => packet.phase === 'repair')).toBe(true)
      sim = tick(sim, 2000, () => 0.5)
      expect(convergence(sim).sameRawState).toBe(false)
      expect(sim.queue).toHaveLength(0)
    }
  })

  it.each([0.08, 0.5])('periodic repair eventually converges under partial loss %s', (dropRate) => {
    let sim = setDropRate(createSimulation(4), 1)
    sim = addElement(bumpCounter(sim, 0), 1, 'water')
    sim = tick(sim, 2000, () => 0.5)
    expect(sim.queue).toHaveLength(0)
    expect(convergence(sim).sameRawState).toBe(false)
    sim = setDropRate(sim, dropRate)
    let seed = 37
    let drops = 0
    let deliveries = 0
    const random = () => {
      seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0
      const value = seed / 2 ** 32
      if (value < dropRate) drops += 1
      else deliveries += 1
      return value
    }
    for (let elapsed = 0; elapsed < 120000 && !convergence(sim).converged; elapsed += 100) {
      sim = tick(sim, 100, random)
    }
    expect(drops).toBeGreaterThan(0)
    expect(deliveries).toBeGreaterThan(0)
    expect(convergence(sim).converged).toBe(true)
    expect(convergence(sim).gcounterValues).toEqual([1, 1, 1, 1])
    for (const peer of sim.peers) expect(readORSet(peer.orset)).toEqual(['water'])
    console.info(`partial loss=${dropRate}: converged at ${sim.now}ms; repair drops=${drops}, deliveries=${deliveries}`)
  })

  it('holds queued manual repair during a new partition and observes latency on retry', () => {
    let sim = setLatency(setAntiEntropyMs(setDropRate(createSimulation(2), 0), 0), 1200)
    sim = dropNextPacket(bumpCounter(sim, 0))
    sim = runAntiEntropyNow(sim)
    expect(sim.queue[0].deliverAt).toBeGreaterThanOrEqual(sim.now + sim.latencyMs)
    sim = tick(setPartitioned(sim, true), 2000)
    expect(convergence(sim).gcounterValues).toEqual([1, 0])
    expect(sim.queue).toHaveLength(1)
    sim = tick(setPartitioned(sim, false), 1199)
    expect(convergence(sim).gcounterValues).toEqual([1, 0])
    sim = tick(sim, 1)
    expect(convergence(sim).gcounterValues).toEqual([1, 1])
    expect(convergence(sim).converged).toBe(true)
  })

  it.each([0, 5000])('Heal reconnects and schedules repair with interval %i', (interval) => {
    let sim = setPartitioned(setAntiEntropyMs(setDropRate(createSimulation(2), 0), interval), true)
    sim = dropNextPacket(bumpCounter(sim, 0))
    expect(convergence(sim).gcounterValues).toEqual([1, 0])

    sim = runAntiEntropyNow(setPartitioned(sim, false))

    expect(sim.partitioned).toBe(false)
    expect(convergence(sim).gcounterValues).toEqual([1, 0])
    expect(sim.queue).toHaveLength(1)
    expect(sim.antiEntropyMs).toBe(interval)
    expect(sim.now).toBe(0)
    expect(sim.nextAntiEntropyAt).toBe(interval || Number.POSITIVE_INFINITY)
    sim = tick(sim, sim.queue[0].deliverAt - 1)
    expect(convergence(sim).gcounterValues).toEqual([1, 0])
    sim = tick(sim, 1)
    expect(convergence(sim).gcounterValues).toEqual([1, 1])
  })

  it.each([0, 5000])('manual repair respects a partition with interval %i', (interval) => {
    let sim = setPartitioned(setAntiEntropyMs(createSimulation(2), interval), true)
    sim = dropNextPacket(bumpCounter(sim, 0))
    const before = sim
    sim = runAntiEntropyNow(sim)
    expect(sim.log).toHaveLength(before.log.length + 1)
    expect(sim.log.slice(1)).toEqual(before.log)
    expect(sim.log[0]).toMatchObject({
      plain: 'Anti-entropy cannot cross the partition yet',
      tone: 'partition',
      technical: 'anti-entropy skipped: partition still enabled',
    })
    expect(sim.queue).toEqual(before.queue)
    expect(convergence(sim).gcounterValues).toEqual([1, 0])
    expect(sim.antiEntropyMs).toBe(interval)
  })

  it('logs scheduled repair blocked by a partition when the deadline arrives', () => {
    const before = setPartitioned(setAntiEntropyMs(createSimulation(2), 5000), true)
    const early = tick(before, 4999)
    expect(early.log).toEqual(before.log)
    const due = tick(early, 1)
    expect(due.log).toHaveLength(before.log.length + 1)
    expect(due.log.slice(1)).toEqual(before.log)
    expect(due.log[0]).toMatchObject({
      at: 5000,
      plain: 'Anti-entropy cannot cross the partition yet',
      tone: 'partition',
      technical: 'anti-entropy skipped: partition still enabled',
    })
    expect(due.peers).toEqual(before.peers)
    expect(due.queue).toEqual(before.queue)
  })

  it('runs scheduled repair at each deadline, never before it', () => {
    let sim = setAntiEntropyMs(setDropRate(createSimulation(2), 0), 5000)
    sim = dropNextPacket(bumpCounter(sim, 0))
    sim = tick(sim, 4999)
    expect(convergence(sim).gcounterValues).toEqual([1, 0])
    sim = tick(sim, 1)
    expect(convergence(sim).gcounterValues).toEqual([1, 0])
    expect(sim.nextAntiEntropyAt).toBe(10000)
    sim = tick(sim, sim.queue[0].deliverAt - sim.now)
    expect(convergence(sim).gcounterValues).toEqual([1, 1])
    sim = dropNextPacket(bumpCounter(sim, 0))
    sim = tick(sim, 9999 - sim.now)
    expect(convergence(sim).gcounterValues).toEqual([2, 1])
    sim = tick(sim, 1)
    expect(convergence(sim).gcounterValues).toEqual([2, 1])
    expect(sim.nextAntiEntropyAt).toBe(15000)
    sim = tick(sim, sim.queue[0].deliverAt - sim.now)
    expect(convergence(sim).gcounterValues).toEqual([2, 2])
  })

  it('keeps scheduled repair disabled before and after Heal', () => {
    let sim = setAntiEntropyMs(setDropRate(createSimulation(2), 0), 0)
    sim = dropNextPacket(bumpCounter(sim, 0))
    sim = tick(sim, 60000)
    expect(convergence(sim).gcounterValues).toEqual([1, 0])
    sim = runAntiEntropyNow(setPartitioned(sim, false))
    expect(convergence(sim).gcounterValues).toEqual([1, 0])
    sim = tick(sim, sim.queue[0].deliverAt - sim.now)
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
    expect(convergence(sim).gcounterValues).toEqual([1, 0])
    expect(sim.nextAntiEntropyAt).toBe(10000)
    sim = tick(sim, sim.queue[0].deliverAt - sim.now, () => 0.5)
    expect(convergence(sim).gcounterValues).toEqual([1, 1])
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

  it('recovers a dropped delta through anti-entropy delivery after loss subsides', () => {
    let sim = setAntiEntropyMs(setDropRate(createSimulation(4), 1), 0)
    sim = addElement(sim, 0, 'medkit')
    sim = tick(sim, 2000, () => 0)

    expect(sim.queue).toHaveLength(0)
    expect(convergence(sim).sameRawState).toBe(false)

    sim = setAntiEntropyMs(setDropRate(sim, 0), 5000)
    sim = runAntiEntropyNow(sim)
    expect(convergence(sim).sameRawState).toBe(false)
    sim = tick(sim, 2000)

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

  it('preserves an OR-Set packet when no counter packet matches and delivers it on schedule', () => {
    let sim = setAntiEntropyMs(setDropRate(createSimulation(3), 0), 0)
    sim = addElement(sim, 0, 'insulin')
    sim = tick(sim, sim.queue[0].deliverAt)
    const queued = sim.queue
    expect(queued).toHaveLength(1)
    expect(queued[0]).toMatchObject({ to: 2, delta: { kind: 'orset.add' } })
    const before = structuredClone(queued)

    sim = dropCounterPacketToPeer(sim, 2)

    expect(sim.queue).toEqual(before)
    expect(sim.queue).toBe(queued)
    expect(sim.log[0]).toMatchObject({
      plain: 'No queued G-Counter bump to drop for Camp 2',
      technical: 'drop requested: no matching gcounter.bump packet queued for peer 2',
      tone: 'drop',
    })
    sim = tick(sim, queued[0].deliverAt - sim.now - 1)
    expect(sim.queue).toEqual(before)
    expect(readORSet(sim.peers[2].orset)).toEqual([])
    sim = tick(sim, 1)
    expect(sim.queue).toEqual([])
    expect(readORSet(sim.peers[2].orset)).toEqual(['insulin'])
    expect(sim.log[0].tone).toBe('deliver')
  })

  it('drops only one matching counter packet from a mixed queue', () => {
    let sim = addElement(createSimulation(3), 0, 'insulin')
    sim = bumpCounter(bumpCounter(sim, 0), 0)
    const before = structuredClone(sim.queue)
    const index = before.findIndex((packet) => packet.to === 2 && packet.delta.kind === 'gcounter.bump')
    expect(index).toBeGreaterThan(0)

    sim = dropCounterPacketToPeer(sim, 2)

    expect(sim.queue).toEqual(before.filter((_, packetIndex) => packetIndex !== index))
    expect(sim.queue.some((packet) => packet.to === 2 && packet.delta.kind === 'gcounter.bump')).toBe(true)
    expect(sim.log[0].technical).toContain('operator dropped G(0:=1) 0->2')
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
    expect(convergence(sim).sameRawState).toBe(false)
    sim = tick(sim, 2000, () => 0.5)

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

describe('Lab numeric boundary regressions', () => {
  it('enforces the peer quota before allocation for construction and reset', () => {
    const sim = createSimulation()
    const before = structuredClone(sim)
    for (const count of [MAX_SIMULATION_PEERS + 1, 1000, 4000000000]) {
      const allocate = vi.spyOn(Array, 'from')
      try {
        const error = new RangeError(`peerCount must be a nonnegative safe integer at most ${MAX_SIMULATION_PEERS}`)
        expect(() => createSimulation(count)).toThrow(error)
        expect(() => setPeerCount(sim, count)).toThrow(error)
        expect(allocate).not.toHaveBeenCalled()
      } finally {
        allocate.mockRestore()
      }
    }
    expect(sim).toEqual(before)
  })

  it('preserves empty, default, moderate, and maximum supported simulations', () => {
    expect(createSimulation().peers).toHaveLength(4)
    for (const count of [0, 4, 50, MAX_SIMULATION_PEERS]) {
      const sim = createSimulation(count)
      expect(sim.peers).toHaveLength(count)
      expect(sim.peers.every((peer, id) => peer.id === id && peer.gcounter.length === count)).toBe(true)
      expect(convergence(sim).converged).toBe(true)
      expect(setPeerCount(createSimulation(), count).peers).toHaveLength(count)
      if (count > 0) {
        const sent = bumpCounter(setDropRate(sim, 0), 0)
        const updated = tick(sent, Math.max(0, ...sent.queue.map((packet) => packet.deliverAt)))
        expect(convergence(updated).gcounterValues).toEqual(Array(count).fill(1))
        expect(convergence(updated).converged).toBe(true)
      }
    }
  })

  const badNumbers = [NaN, Infinity, -Infinity, -1, Number.MAX_SAFE_INTEGER + 1, '2', null, true] as number[]
  it.each(badNumbers)('refuses invalid peer counts %s before constructing carriers', (bad) => {
    expect(() => createSimulation(bad)).toThrow(RangeError)
    expect(() => setPeerCount(createSimulation(2), bad)).toThrow(RangeError)
  })
  it('refuses fractional peer counts instead of truncating them', () => {
    expect(() => createSimulation(2.5)).toThrow(RangeError)
    expect(() => setPeerCount(createSimulation(2), 2.5)).toThrow(RangeError)
  })
  for (const [name, setter] of [['latency', setLatency], ['anti entropy', setAntiEntropyMs], ['clock', tick]] as const) {
    it.each(badNumbers)(`${name} refuses invalid duration %s without mutation`, (bad) => {
      const sim = createSimulation(2)
      const before = structuredClone(sim)
      expect(() => setter(sim, bad)).toThrow(RangeError)
      expect(sim).toEqual(before)
    })
  }
  it.each([...badNumbers, 1.01])('refuses invalid drop probability %s', (bad) => {
    expect(() => setDropRate(createSimulation(2), bad)).toThrow(RangeError)
  })
  it.each([0, 1, 'false', null, undefined])('refuses nonboolean partition %s', (bad) => {
    expect(() => setPartitioned(createSimulation(2), bad as unknown as boolean)).toThrow(TypeError)
  })
  for (const [name, operation] of [
    ['bump', (sim: Simulation, peer: number) => bumpCounter(sim, peer)],
    ['add', (sim: Simulation, peer: number) => addElement(sim, peer, 'water')],
    ['remove', (sim: Simulation, peer: number) => removeElement(sim, peer, 'water')],
    ['drop to peer', dropCounterPacketToPeer],
  ] as const) {
    it.each([...badNumbers, 0.5, 2])(`${name} refuses invalid peer %s`, (bad) => {
      const sim = bumpCounter(createSimulation(2), 0)
      const before = structuredClone(sim)
      expect(() => operation(sim, bad)).toThrow(RangeError)
      expect(sim).toEqual(before)
    })
  }
  for (const [name, operation] of [
    ['add', (sim: Simulation) => addElement(sim, 0, 'water')],
    ['bump', (sim: Simulation) => bumpCounter(sim, 0)],
    ['duplicate', duplicateNextPacket],
    ['repair', queueAntiEntropyPackets],
  ] as const) {
    it.each([-1, 0.5, Number.MAX_SAFE_INTEGER, Number.MAX_SAFE_INTEGER + 1])(`${name} refuses an invalid or exhausted ID %s`, (nextId) => {
      const sim = { ...bumpCounter(createSimulation(2), 0), nextId }
      const before = structuredClone(sim)
      expect(() => operation(sim)).toThrow(RangeError)
      expect(sim).toEqual(before)
    })
  }
  it.each([-1, 0.5, NaN, Number.MAX_SAFE_INTEGER])('refuses invalid or exhausted local tally %s', (localTally) => {
    const base = createSimulation(2)
    const sim = { ...base, peers: base.peers.map((peer, id) => id === 0 ? { ...peer, localTally } : peer) }
    expect(() => bumpCounter(sim, 0)).toThrow(RangeError)
  })
  for (const [name, operation] of [
    ['clock', (sim: Simulation) => tick(sim, 1)],
    ['anti entropy setter', (sim: Simulation) => setAntiEntropyMs(sim, 1)],
    ['emission', (sim: Simulation) => bumpCounter(sim, 0)],
    ['manual repair', runAntiEntropyNow],
    ['queued repair', queueAntiEntropyPackets],
    ['reorder', reorderQueue],
    ['partition retry', (sim: Simulation) => tick(setPartitioned(sim, true), 0)],
  ] as const) {
    it(`${name} refuses schedule overflow without mutation`, () => {
      const sim = { ...bumpCounter(createSimulation(3), 0), now: Number.MAX_SAFE_INTEGER }
      const before = structuredClone(sim)
      expect(() => operation(sim)).toThrow(RangeError)
      expect(sim).toEqual(before)
    })
  }
  it('refuses duplicate delivery time overflow', () => {
    const queued = bumpCounter(createSimulation(2), 0)
    queued.queue[0].deliverAt = Number.MAX_SAFE_INTEGER
    expect(() => duplicateNextPacket(queued)).toThrow(RangeError)
  })
  it.each([0.25, 0.5])('refuses a fractional clock overflow that rounds back to MAX_SAFE_INTEGER (%s)', (duration) => {
    const sim = { ...createSimulation(2), now: Number.MAX_SAFE_INTEGER }
    expect(() => tick(sim, duration)).toThrow(RangeError)
    expect(() => setAntiEntropyMs(sim, duration)).toThrow(RangeError)
  })
  it('preserves zero, fractional durations/probabilities, valid peers and last safe ID', () => {
    expect(createSimulation(0).peers).toEqual([])
    let sim = setDropRate(setLatency(setAntiEntropyMs(createSimulation(2), 0), 0.25), 0.5)
    sim = tick(sim, 0.25)
    expect(sim.now).toBe(0.25)
    expect(sim.nextAntiEntropyAt).toBe(Infinity)
    sim = addElement({ ...sim, nextId: Number.MAX_SAFE_INTEGER - 1 }, 0, 'water')
    expect(sim.nextId).toBe(Number.MAX_SAFE_INTEGER)
    expect(sim.queue[0].delta).toMatchObject({ token: BigInt(Number.MAX_SAFE_INTEGER - 1) })
    expect(convergence(tick(setDropRate(sim, 0), 1)).orsetElements).toEqual(['water'])
  })
})
