import { describe, expect, it } from 'vitest'
import {
  addElement,
  bumpCounter,
  convergence,
  createSimulation,
  removeElement,
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
})
