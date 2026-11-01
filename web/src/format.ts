// Numbers in plain words (DESIGN, Principles): a unit always travels with its value.

const EXACT = new Intl.NumberFormat('en-US', { maximumFractionDigits: 2 });
const COMPACT = [
  { from: 1e9, suffix: 'B' },
  { from: 1e6, suffix: 'M' },
  { from: 1e3, suffix: 'k' },
] as const;

/** The unit for a count: "lines" for many, "line" for one. */
function unitFor(value: number, unit: string): string {
  return value === 1 && unit.endsWith('s') ? unit.slice(0, -1) : unit;
}

/** `303412` as "303k": two significant figures below ten of a magnitude, none after. */
export function compact(value: number): string {
  const found = COMPACT.find(({ from }) => Math.abs(value) >= from);
  if (!found) return EXACT.format(Math.round(value));
  const scaled = value / found.from;
  const digits = Math.abs(scaled) < 10 ? 1 : 0;
  return `${scaled.toFixed(digits).replace(/\.0$/, '')}${found.suffix}`;
}

/** A value with its unit in words, short: "303k lines". */
export function amount(value: number, unit: string): string {
  return `${compact(value)} ${unitFor(Math.round(value), unit)}`;
}

/** A value with its unit, exact: "303,412 lines". */
export function exactAmount(value: number, unit: string): string {
  return `${EXACT.format(value)} ${unitFor(value, unit)}`;
}

/** A ratio or other unitless measure: three significant figures at most. */
export function plain(value: number): string {
  if (value === 0) return '0';
  const abs = Math.abs(value);
  if (abs >= 1000) return compact(value);
  return String(Number(value.toPrecision(abs >= 100 ? 3 : 2)));
}

/** A share from 0 to 1 as a whole percent: `0.4` is "40%", and a sliver is "under 1%". */
export const percentOf = (share: number): string =>
  share > 0 && share < 0.005 ? 'under 1%' : `${String(Math.round(share * 100))}%`;

/** A count with its noun: "1 row", "5 rows". */
export const counted = (n: number, noun: string): string =>
  `${String(n)} ${noun}${n === 1 ? '' : 's'}`;

/** An internal name in words: `test_share` and `testShare` become "test share". */
export const words = (name: string): string =>
  name
    .replaceAll('_', ' ')
    .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
    .toLowerCase();
