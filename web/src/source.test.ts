import fc from 'fast-check';
import { expect, test } from 'vitest';
import { around, formatLines, parseLines } from './source';

test('a range survives the round trip through the address', () => {
  fc.assert(
    fc.property(
      fc.integer({ min: 1, max: 100000 }),
      fc.option(fc.integer({ min: 1, max: 100000 }), { nil: null }),
      (start, end) => {
        expect(parseLines(formatLines({ start, end }))).toEqual({ start, end });
      },
    ),
  );
});

test('text that is not a range names no lines', () => {
  for (const text of [null, '', 'x', '0-5', '-5', '1-2-3']) expect(parseLines(text)).toBeNull();
});

test('a single line is a range of one', () => {
  expect(parseLines('7')).toEqual({ start: 7, end: 7 });
});

test('the context around evidence never starts before the first line', () => {
  expect(around(3, 5).start).toBe(1);
  expect(around(100, 120).start).toBeLessThan(100);
  expect(around(100, 120).end).toBeGreaterThan(120);
});
