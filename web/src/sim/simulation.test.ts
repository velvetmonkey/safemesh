import { describe, expect, it } from 'vitest'
import {
  addElement,
  bumpCounter,
  convergence,
  createSimulation,
  dropNextPacket,
  duplicateNextPacket,
  removeElement,
  reorderQueue,
  runAntiEntropyNow,
  setAntiEntropyMs,
  setDropRate,
  setPartitioned,
  tick,
} from './simulation'

describe('mesh simulation', () => {
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
