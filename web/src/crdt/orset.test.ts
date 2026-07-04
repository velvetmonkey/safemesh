import { describe, expect, it } from 'vitest'
import { addDelta, applyORSetDelta, bottomORSet, observedTokens, readORSet, removeDelta } from './orset'

describe('OR-Set mirror', () => {
  it('keeps a concurrent fresh add visible after an observed-token remove', () => {
    const first = addDelta('radio', 'p0-1')
    const concurrent = addDelta('radio', 'p1-2')
    const removerView = applyORSetDelta(bottomORSet(), first)
    const remove = removeDelta(observedTokens(removerView, 'radio'))

    const merged = [first, remove, concurrent].reduce(applyORSetDelta, bottomORSet())

    expect(readORSet(merged)).toEqual(['radio'])
    expect(merged.tombstones).toEqual({ 'p0-1': true })
  })

  it('removes an element when all observed add tokens are tombstoned', () => {
    const state = [addDelta('water', 'p0-1'), removeDelta(['p0-1'])].reduce(
      applyORSetDelta,
      bottomORSet(),
    )

    expect(readORSet(state)).toEqual([])
  })
})
