import { afterEach, describe, expect, it, vi } from 'vitest'
import { SafeMeshGCounterReplica, SafeMeshStringOrSetReplica, SafeMeshStringOrSetAddEntry } from '../../../rust/crates/safemesh-wasm/pkg/safemesh_wasm'
import * as mesh from './simulation'

const counts = vi.hoisted(() => ({ counter: [0, 0], set: [0, 0], entry: [0, 0] }))
vi.mock('../../../rust/crates/safemesh-wasm/pkg/safemesh_wasm', async (importOriginal) => {
  const api = await importOriginal<typeof import('../../../rust/crates/safemesh-wasm/pkg/safemesh_wasm')>()
  return {
    ...api,
    SafeMeshGCounterReplica: class extends api.SafeMeshGCounterReplica {
      constructor(...args: ConstructorParameters<typeof api.SafeMeshGCounterReplica>) {
        super(...args)
        counts.counter[0]++
      }
      free() { counts.counter[1]++; super.free() }
    },
    SafeMeshStringOrSetReplica: class extends api.SafeMeshStringOrSetReplica {
      constructor(...args: ConstructorParameters<typeof api.SafeMeshStringOrSetReplica>) {
        super(...args)
        counts.set[0]++
      }
      free() { counts.set[1]++; super.free() }
      addEntries() {
        const entries = super.addEntries()
        counts.entry[0] += entries.length
        for (const entry of entries) {
          const free = entry.free.bind(entry)
          entry.free = () => { counts.entry[1]++; free() }
        }
        return entries
      }
    },
  }
})

afterEach(() => vi.restoreAllMocks())
const snapshot = () => Object.fromEntries(Object.entries(counts).map(([key, value]) => [key, [...value]]))
function balanced(before: ReturnType<typeof snapshot>) {
  for (const [kind, [built, freed]] of Object.entries(counts)) {
    expect(built - before[kind][0], kind).toBe(freed - before[kind][1])
  }
}

describe('native simulation handle ownership', () => {
  it('releases every handle across drops, delivery, both repairs and empty repairs', () => {
    const before = snapshot()
    let sim = mesh.setAntiEntropyMs(mesh.setDropRate(mesh.createSimulation(4), 0.5), 100)
    sim = mesh.bumpCounter(sim, 0)
    sim = mesh.addElement(sim, 1, 'water')
    sim = mesh.addElement(sim, 1, 'food')
    sim = mesh.removeElement(sim, 1, 'water')
    sim = mesh.removeElement(sim, 2, 'absent')
    sim = mesh.duplicateNextPacket(sim)
    sim = mesh.reorderQueue(sim)
    sim = mesh.tick(mesh.setPartitioned(sim, true), 2000, () => 0)
    sim = mesh.setPartitioned(sim, false)
    sim = mesh.dropNextPacket(sim)
    sim = mesh.queueAntiEntropyPackets(sim)
    sim = mesh.tick(mesh.setAntiEntropyMs(sim, 0), 10000, () => 0)
    sim = mesh.runAntiEntropyNow(sim)
    sim = mesh.queueAntiEntropyPackets(sim)
    sim = mesh.tick(sim, 10000, () => 1)
    expect(mesh.convergence(sim).converged).toBe(true)
    balanced(before)
  })

  it('builds four counters once and none for idle clocks, including scheduled repair', () => {
    let sim = mesh.setAntiEntropyMs(mesh.setDropRate(mesh.createSimulation(4), 0), 0)
    for (let i = 0; i < 8; i++) sim = mesh.bumpCounter(sim, i % 4)
    sim = mesh.addElement(sim, 0, 'water')
    sim = mesh.tick(sim, 2000)
    sim = mesh.setAntiEntropyMs(sim, 100)
    const before = snapshot()
    mesh.convergence(sim)
    expect(counts.counter[0] - before.counter[0]).toBe(4)
    const warmed = snapshot()
    for (let i = 0; i < 200; i++) {
      sim = mesh.tick(sim, 33)
      expect(mesh.convergence(sim).converged).toBe(true)
    }
    expect(snapshot()).toEqual(warmed)
    balanced(before)
  })

  it('invalidates on landing and checks queue emptiness even with cached carriers', () => {
    let sim = mesh.setAntiEntropyMs(mesh.setDropRate(mesh.createSimulation(2), 0), 0)
    sim = mesh.duplicateNextPacket(mesh.bumpCounter(sim, 0))
    expect(mesh.convergence(sim).gcounterValues).toEqual([1, 0])
    sim = mesh.tick(sim, 550)
    expect(mesh.convergence(sim).gcounterValues).toEqual([1, 1])
    expect(mesh.convergence(sim).sameRawState).toBe(true)
    expect(mesh.convergence(sim).converged).toBe(false)
    // Dropping the duplicate changes only the queue; carriers retain their identity.
    expect(mesh.convergence(mesh.dropNextPacket(sim)).converged).toBe(true)
    sim = mesh.tick(sim, 220)
    expect(mesh.convergence(sim).converged).toBe(true)
  })

  it('cleans up failed reconstruction, partial convergence and merge errors', () => {
    const sim = mesh.bumpCounter(mesh.createSimulation(4), 0)
    for (const method of ['mergeLogBytes', 'value', 'sameStateAs', 'mergeRecordBytes'] as const) {
      const before = snapshot()
      vi.spyOn(SafeMeshGCounterReplica.prototype, method).mockImplementation(() => { throw new Error('injected') })
      expect(() => method === 'mergeRecordBytes' ? mesh.bumpCounter(sim, 1) : mesh.convergence(sim)).toThrow('injected')
      balanced(before)
      vi.restoreAllMocks()
    }
  })

  it('cleans up OR-Set failures and the first handle when reconstruction of its partner fails', () => {
    const sim = mesh.addElement(mesh.createSimulation(2), 1, 'water')
    for (const method of ['observedTokens', 'mergeLogBytes', 'versionFor'] as const) {
      const before = snapshot()
      vi.spyOn(SafeMeshStringOrSetReplica.prototype, method).mockImplementation(() => { throw new Error('injected') })
      expect(() => method === 'observedTokens' ? mesh.removeElement(sim, 1, 'water') : mesh.queueAntiEntropyPackets(sim)).toThrow('injected')
      balanced(before)
      vi.restoreAllMocks()
    }
  })

  it('releases every entry when reading the first entry throws', () => {
    let sim = mesh.addElement(mesh.createSimulation(2), 0, 'water')
    sim = mesh.addElement(sim, 0, 'food')
    const before = snapshot()
    vi.spyOn(SafeMeshStringOrSetAddEntry.prototype, 'element').mockImplementation(() => { throw new Error('injected') })
    expect(() => mesh.convergence(sim)).toThrow('injected')
    expect(counts.entry[0] - before.entry[0]).toBe(2)
    balanced(before)
  })


  it('still repairs different histories when carrier states already match', () => {
    const before = snapshot()
    const sim = mesh.createSimulation(2)
    const counter = new SafeMeshGCounterReplica(0n, 2)
    try {
      counter.appendBump(0, 1n)
      sim.peers[1].gcounterLog = counter.logBytes()
      counter.appendBump(0, 1n)
      sim.peers[0].gcounterLog = counter.logBytes()
    } finally {
      counter.free()
    }
    expect(mesh.convergence(sim).sameRawState).toBe(true)
    const repaired = mesh.runAntiEntropyNow(sim)
    expect(repaired.peers[1].gcounterLog).toEqual(repaired.peers[0].gcounterLog)
    expect(repaired.log.some((entry) => entry.tone === 'anti-entropy')).toBe(true)
    balanced(before)
  })

})
