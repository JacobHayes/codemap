import fc from 'fast-check';
import { expect, test } from 'vitest';
import { DEFAULT_STATE, parseState, serializeState, type UrlState } from './urlState';

const word = fc.stringMatching(/^[a-z][a-z0-9_.-]{0,8}$/);
const stateArb: fc.Arbitrary<UrlState> = fc.record({
  view: fc.constantFrom('map', 'areas'),
  path: fc.array(word, { maxLength: 4 }).map((parts) => parts.join('/')),
  role: fc.option(word, { nil: null }),
  concern: fc.option(word, { nil: null }),
  language: fc.option(word, { nil: null }),
  hideGenerated: fc.boolean(),
  size: word,
  color: fc.option(word, { nil: null }),
  selected: fc.option(word, { nil: null }),
  area: fc.option(word, { nil: null }),
});

test('the stock view is the bare address', () => {
  expect(serializeState(DEFAULT_STATE)).toBe('');
  expect(parseState('')).toEqual(DEFAULT_STATE);
});

test('a state survives the round trip through the address', () => {
  fc.assert(
    fc.property(stateArb, (state) => {
      expect(parseState(serializeState(state))).toEqual(state);
    }),
  );
});

test('only values that differ from the defaults are written', () => {
  const state = { ...DEFAULT_STATE, role: 'test', size: 'tokens' };
  expect(Array.from(new URLSearchParams(serializeState(state)).keys()).sort()).toEqual([
    'role',
    'size',
  ]);
});

test('hiding generated code is the default and only showing it is written', () => {
  expect(parseState('').hideGenerated).toBe(true);
  expect(serializeState({ ...DEFAULT_STATE, hideGenerated: false })).toBe('?hide-generated=false');
});

test('slashes in a drill path stay readable', () => {
  expect(serializeState({ ...DEFAULT_STATE, path: 'crates/core/src' })).toBe(
    '?path=crates/core/src',
  );
});
