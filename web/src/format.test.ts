import { expect, test } from 'vitest';
import { amount, compact, counted, exactAmount, plain } from './format';

test('a count is short, with its unit in words', () => {
  expect(amount(303_412, 'lines')).toBe('303k lines');
  expect(amount(1_240_000, 'tokens')).toBe('1.2M tokens');
  expect(amount(812, 'syntax nodes')).toBe('812 syntax nodes');
});

test('one of a thing is singular', () => {
  expect(amount(1, 'lines')).toBe('1 line');
  expect(exactAmount(1, 'syntax nodes')).toBe('1 syntax node');
});

test('exact values keep every digit and group thousands', () => {
  expect(exactAmount(303_412, 'lines')).toBe('303,412 lines');
});

test('compact never claims more precision than two figures below ten of a magnitude', () => {
  expect(compact(1_500)).toBe('1.5k');
  expect(compact(12_400)).toBe('12k');
  expect(compact(999)).toBe('999');
});

test('a ratio keeps two or three significant figures', () => {
  expect(plain(0.3141)).toBe('0.31');
  expect(plain(12.7)).toBe('13');
  expect(plain(0)).toBe('0');
});

test('internal names read as words', async () => {
  const { words } = await import('./format');
  expect(words('excessLines')).toBe('excess lines');
  expect(words('test_to_prod_ratio')).toBe('test to prod ratio');
});

test('a count agrees with its noun', () => {
  expect(counted(1, 'row')).toBe('1 row');
  expect(counted(0, 'row')).toBe('0 rows');
  expect(counted(5, 'column')).toBe('5 columns');
});
