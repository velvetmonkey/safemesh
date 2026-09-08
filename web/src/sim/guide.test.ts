import { describe, expect, it } from 'vitest'
import { readORSet } from './simulation'
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

  it('starts guided recovery steps with visible work still to do', () => {
    const dropRecover = stepIndex('drop', 'recover')
    const dropStart = buildScenarioSimulation('drop', dropRecover, 0)

    expect(convergence(dropStart).converged).toBe(false)
    expect(dropStart.peers[3].gcounter[0]).toBe(0)
    expect(dropStart.queue.some((packet) => packet.phase === 'repair' && packet.to === 3)).toBe(true)

    const partitionHeal = stepIndex('partition', 'heal')
    const partitionStart = buildScenarioSimulation('partition', partitionHeal, 0)

    expect(partitionStart.partitioned).toBe(false)
    expect(convergence(partitionStart).converged).toBe(false)
    expect(partitionStart.queue.length).toBeGreaterThan(0)

    const partitionMid = buildScenarioSimulation('partition', partitionHeal, 500)
    expect(convergence(partitionMid).converged).toBe(false)
    expect(partitionMid.queue.length).toBeGreaterThan(0)
  })

  it('marks reordered and repair packets for the UI', () => {
    const reorderStep = buildScenarioSimulation('reorder', stepIndex('reorder', 'reorder'), 0)

    expect(reorderStep.queue.length).toBeGreaterThan(0)
    expect(reorderStep.queue.every((packet) => packet.phase === 'reordered')).toBe(true)

    const dropRecover = buildScenarioSimulation('drop', stepIndex('drop', 'recover'), 0)
    expect(dropRecover.queue.some((packet) => packet.phase === 'repair')).toBe(true)
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

function stepIndex(scenarioId: string, stepId: string): number {
  const scenario = GUIDE_SCENARIOS.find((candidate) => candidate.id === scenarioId)
  if (!scenario) throw new Error(`missing scenario ${scenarioId}`)
  const index = scenario.steps.findIndex((step) => step.id === stepId)
  if (index < 0) throw new Error(`missing step ${scenarioId}/${stepId}`)
  return index
}
