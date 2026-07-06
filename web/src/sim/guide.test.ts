import { describe, expect, it } from 'vitest'
import { readORSet } from '../crdt'
import { GUIDE_SCENARIOS, buildScenarioSimulation, createScenarioBaseline } from './guide'
import { convergence } from './simulation'

describe('guided scenario replay', () => {
  it('builds every authored step without random packet loss', () => {
    for (const scenario of GUIDE_SCENARIOS) {
      for (let index = 0; index < scenario.steps.length; index += 1) {
        const sim = buildScenarioSimulation(scenario.id, index)
        expect(sim.dropRate).toBe(0)
        expect(sim.peers).toHaveLength(4)
        expect(sim.now).toBeGreaterThanOrEqual(0)
      }
    }
  })

  it('is deterministic for timeline jumps inside every scenario', () => {
    for (const scenario of GUIDE_SCENARIOS) {
      const lastStep = scenario.steps.length - 1
      const first = buildScenarioSimulation(scenario.id, lastStep, 180)
      const second = buildScenarioSimulation(scenario.id, lastStep, 180)

      expect(second).toEqual(first)
    }
  })

  it('ends every scenario with the same visible modeled state', () => {
    for (const scenario of GUIDE_SCENARIOS) {
      const sim = buildScenarioSimulation(scenario.id, scenario.steps.length - 1)
      const status = convergence(sim)

      expect(status.converged, scenario.id).toBe(true)
      expect(status.gcounterValue, scenario.id).toBe(1)
      expect(status.orsetElements, scenario.id).toEqual(['vaccine'])
      expect(sim.peers.every((peer) => readORSet(peer.orset).join('|') === 'vaccine'), scenario.id).toBe(true)
    }
  })

  it('keeps duplicate, reorder, drop, and partition evidence visible in the log', () => {
    const duplicate = finalScenarioLog('duplicate')
    expect(duplicate.some((entry) => entry.technical.includes('duplicated'))).toBe(true)

    const reorder = finalScenarioLog('reorder')
    expect(reorder.some((entry) => entry.technical.includes('reordered'))).toBe(true)

    const drop = finalScenarioLog('drop')
    expect(drop.some((entry) => entry.technical.includes('operator dropped'))).toBe(true)
    expect(drop.some((entry) => entry.technical.includes('anti-entropy'))).toBe(true)

    const partition = finalScenarioLog('partition')
    expect(partition.some((entry) => entry.technical.includes('partition enabled'))).toBe(true)
    expect(partition.some((entry) => entry.technical.includes('partition healed'))).toBe(true)
  })

  it('starts from a quiet four-replica baseline', () => {
    const sim = createScenarioBaseline()

    expect(convergence(sim).converged).toBe(true)
    expect(sim.log).toHaveLength(0)
    expect(sim.queue).toHaveLength(0)
  })
})

function finalScenarioLog(scenarioId: string) {
  const scenario = GUIDE_SCENARIOS.find((candidate) => candidate.id === scenarioId)
  if (!scenario) throw new Error(`missing scenario ${scenarioId}`)
  return buildScenarioSimulation(scenario.id, scenario.steps.length - 1).log
}
