import { describe, expect, it } from 'vitest'
import { readORSet } from '../crdt'
import { GUIDE_SCENES, buildGuidedSimulation, createGuidedBaseline } from './guide'
import { convergence } from './simulation'

describe('guided timeline replay', () => {
  it('builds every authored scene without random packet loss', () => {
    for (let index = 0; index < GUIDE_SCENES.length; index += 1) {
      const sim = buildGuidedSimulation(index)
      expect(sim.dropRate).toBe(0)
      expect(sim.peers).toHaveLength(4)
      expect(sim.now).toBeGreaterThanOrEqual(0)
    }
  })

  it('keeps the scripted duplicate, reorder, and drop visible in the log', () => {
    const duplicateScene = buildGuidedSimulation(2)
    expect(duplicateScene.log.some((entry) => entry.technical.includes('duplicated'))).toBe(true)

    const reorderScene = buildGuidedSimulation(3)
    expect(reorderScene.log.some((entry) => entry.technical.includes('reordered'))).toBe(true)

    const dropScene = buildGuidedSimulation(4)
    expect(dropScene.log.some((entry) => entry.technical.includes('operator dropped'))).toBe(true)
  })

  it('is deterministic for timeline jumps', () => {
    const first = buildGuidedSimulation(8, 300)
    const second = buildGuidedSimulation(8, 300)

    expect(second).toEqual(first)
  })

  it('ends with the authored modeled state converged', () => {
    const sim = buildGuidedSimulation(GUIDE_SCENES.length - 1)
    const status = convergence(sim)

    expect(status.converged).toBe(true)
    expect(status.gcounterValue).toBe(1)
    expect(status.orsetElements).toEqual(['freezer', 'vaccine'])
    expect(sim.peers.every((peer) => readORSet(peer.orset).join('|') === 'freezer|vaccine')).toBe(true)
  })

  it('starts from a quiet four-replica baseline', () => {
    const sim = createGuidedBaseline()

    expect(convergence(sim).converged).toBe(true)
    expect(sim.log).toHaveLength(0)
    expect(sim.queue).toHaveLength(0)
  })
})
