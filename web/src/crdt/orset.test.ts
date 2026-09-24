import { describe, expect, it, vi } from 'vitest'
import { addDelta, applyORSetDelta, bottomORSet, mergeORSet, observedTokens, readORSet, removeDelta, sameORSet } from './orset'

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

it.each(['__proto__', 'constructor', 'toString'])('preserves and removes prototype-named token %s', (token) => {
  const added = applyORSetDelta(bottomORSet(), addDelta('water', token))
  expect(readORSet(added)).toEqual(['water'])
  expect(observedTokens(added, 'water')).toEqual([token])
  const removed = applyORSetDelta(bottomORSet(), removeDelta([token]))
  expect(Object.hasOwn(removed.tombstones, token)).toBe(true)
  expect(readORSet(mergeORSet(added, removed))).toEqual([])
  expect(readORSet(mergeORSet(removed, added))).toEqual([])
  expect(readORSet(applyORSetDelta(removed, addDelta('water', token)))).toEqual([])
})

it.each(['token', '__proto__'])('refuses conflicting values for token %s in either order', (token) => {
  const radio = applyORSetDelta(bottomORSet(), addDelta('radio', token))
  const water = applyORSetDelta(bottomORSet(), addDelta('water', token))
  expect(() => mergeORSet(radio, water)).toThrow(/conflicting/)
  expect(() => mergeORSet(water, radio)).toThrow(/conflicting/)
  expect(() => applyORSetDelta(radio, addDelta('water', token))).toThrow(/conflicting/)
  expect(() => applyORSetDelta(water, addDelta('radio', token))).toThrow(/conflicting/)
  const removed = applyORSetDelta(radio, removeDelta([token]))
  expect(() => mergeORSet(removed, water)).toThrow(/conflicting/)
  expect(() => mergeORSet(water, removed)).toThrow(/conflicting/)
  expect(readORSet(radio)).toEqual(['radio'])
  expect(readORSet(water)).toEqual(['water'])
  expect(mergeORSet(radio, radio)).toEqual(radio)
  expect(applyORSetDelta(radio, addDelta('radio', token))).toEqual(radio)
  const fresh = applyORSetDelta(bottomORSet(), addDelta('water', 'fresh'))
  const merged = mergeORSet(radio, fresh)
  expect(mergeORSet(fresh, radio)).toEqual(merged)
  expect(mergeORSet(merged, fresh)).toEqual(merged)
  expect(readORSet(merged)).toEqual(['radio', 'water'])
})

// Explicit collators reproduce different runtime locales without depending on host ICU defaults.
it.each(['en', 'sv'])('uses Rust string ordering under %s collation', (locale) => {
  const collator = new Intl.Collator(locale)
  const localeCompare = vi.spyOn(String.prototype, 'localeCompare').mockImplementation(function (this: string, other) {
    return collator.compare(String(this), other)
  })
  try {
    const elements = ['z', 'ä', 'A', 'a', '\u{10000}', '\uE000', '', 'aa', 'é', 'e\u0301']
    const state = elements.reduce((state, element) => applyORSetDelta(state, addDelta(element, element)), bottomORSet())
    expect(readORSet(state)).toEqual(['', 'A', 'a', 'aa', 'e\u0301', 'z', 'ä', 'é', '\uE000', '\u{10000}'])
    const reversed = {
      adds: Object.fromEntries(Object.entries(state.adds).reverse()),
      tombstones: Object.fromEntries(['é', 'e\u0301'].map(token => [token, true as const])),
    }
    expect(sameORSet(state, { ...reversed, tombstones: {} })).toBe(true)
    expect(sameORSet({ ...state, tombstones: { 'e\u0301': true, 'é': true } }, reversed)).toBe(true)
    expect(localeCompare).not.toHaveBeenCalled()
  } finally {
    localeCompare.mockRestore()
  }
})

const fullwidthTildeGrinningFaceTokens = {
  input: ['peer-\u{1F600}-1', 'peer-\uFF5E-1'],
  expected: ['peer-\uFF5E-1', 'peer-\u{1F600}-1'],
}

it('removeDelta orders U+FF5E FULLWIDTH TILDE before U+1F600 GRINNING FACE tokens', () => {
  const { input, expected } = fullwidthTildeGrinningFaceTokens
  expect(removeDelta([...input, input[0]])).toEqual({ kind: 'orset.remove', tokens: expected })
})

it('observedTokens orders U+FF5E FULLWIDTH TILDE before U+1F600 GRINNING FACE tokens', () => {
  const { input, expected } = fullwidthTildeGrinningFaceTokens
  const state = [
    ...input.map(token => addDelta('radio', token)),
    addDelta('radio', 'removed'),
    addDelta('water', 'other-element'),
    removeDelta(['removed']),
  ].reduce(applyORSetDelta, bottomORSet())
  expect(observedTokens(state, 'radio')).toEqual(expected)
})
