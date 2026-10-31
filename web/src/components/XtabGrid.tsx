import type { CSSProperties, KeyboardEvent } from 'react';
import type { Axis } from '../generated/Query';
import type { Xtab, XtabCell } from '../generated/Answer';
import { compact, exactAmount } from '../format';
import type { Swatch } from '../palette';
import type { Navigate, UrlState } from '../urlState';

export const AXIS_NAMES: Record<Axis, string> = {
  role: 'Role',
  concern: 'Concern',
  language: 'Language',
  kind: 'Symbol kind',
  path: 'Path',
};

/** Rows and columns drawn at most: a grid of paths can have thousands of intersections, and the
 * DOM draws every one. The largest by total come first; the answer's totals still count the rest. */
export const XTAB_ROWS_MAX = 60;
export const XTAB_COLS_MAX = 24;

/** A cell the pointer or the keyboard is on, and where. */
export type CellPoint = { row: string; col: string; value: number; x: number; y: number };

/** The cell's dimension values as filters; a path value drills there. */
function filterPatch(axis: Axis, value: string): Partial<UrlState> {
  return axis === 'path' ? { path: value } : { [axis]: value };
}

type GridProps = {
  xtab: Xtab;
  swatches: Swatch[];
  neutral: Swatch;
  navigate: Navigate;
  onPoint: (point: CellPoint | null) => void;
  /** Goes up one level of the drill path, if there is one. */
  onUp: () => void;
};

type CellProps = {
  cell: XtabCell | undefined;
  rows: [number, number];
  names: [string, string];
  unit: string;
  paint: Swatch;
  onPick: () => void;
  onPoint: GridProps['onPoint'];
};

function Cell({ cell, rows, names, unit, paint, onPick, onPoint }: CellProps) {
  const point = (el: HTMLElement) => {
    if (!cell) return;
    const box = el.getBoundingClientRect();
    onPoint({
      row: names[0],
      col: names[1],
      value: cell.value,
      x: box.left + box.width / 2,
      y: box.bottom,
    });
  };
  return (
    <td className="cell" style={{ background: paint.css, color: paint.inkCss }}>
      {cell && (
        <button
          data-row={rows[0]}
          data-col={rows[1]}
          aria-label={`${compact(cell.value)}, ${exactAmount(cell.value, unit)} in ${names[0]} and ${names[1]}. Filter to these`}
          onClick={onPick}
          onMouseEnter={(e) => {
            point(e.currentTarget);
          }}
          onMouseLeave={() => {
            onPoint(null);
          }}
          onFocus={(e) => {
            point(e.currentTarget);
          }}
          onBlur={() => {
            onPoint(null);
          }}
        >
          {compact(cell.value)}
        </button>
      )}
    </td>
  );
}

const STEPS: Record<string, [number, number]> = {
  ArrowUp: [-1, 0],
  ArrowDown: [1, 0],
  ArrowLeft: [0, -1],
  ArrowRight: [0, 1],
};

/** Arrows move to the next cell that has a value in that direction; Backspace goes up. */
function keyDown(e: KeyboardEvent<HTMLTableElement>, onUp: () => void) {
  const from = e.target;
  if (!(from instanceof HTMLButtonElement)) return;
  if (e.key === 'Backspace') {
    e.preventDefault();
    onUp();
    return;
  }
  const step = STEPS[e.key];
  if (!step) return;
  e.preventDefault();
  let row = Number(from.dataset['row']);
  let col = Number(from.dataset['col']);
  for (;;) {
    row += step[0];
    col += step[1];
    const next = e.currentTarget.querySelector<HTMLButtonElement>(
      `[data-row="${String(row)}"][data-col="${String(col)}"]`,
    );
    if (next) {
      next.focus();
      return;
    }
    const inside = row >= 0 && col >= 0 && row < XTAB_ROWS_MAX && col < XTAB_COLS_MAX;
    if (!inside) return;
  }
}

function HeaderRow({ xtab, cols }: { xtab: Xtab; cols: Xtab['cols'] }) {
  return (
    <tr>
      <th scope="col">{`${AXIS_NAMES[xtab.rowAxis]} by ${AXIS_NAMES[xtab.colAxis]}`}</th>
      {cols.map((col) => (
        <th scope="col" className="num" key={col.name}>
          {col.name}
        </th>
      ))}
      <th scope="col" className="num total">
        Total
      </th>
    </tr>
  );
}

/** The grid: a row per dimension value, a cell per pair, totals in the margins. */
export function XtabGrid({ xtab, swatches, neutral, navigate, onPoint, onUp }: GridProps) {
  const unit = xtab.measure.unit;
  const rows = xtab.rows.slice(0, XTAB_ROWS_MAX);
  const cols = xtab.cols.slice(0, XTAB_COLS_MAX);
  const at = new Map(xtab.cells.map((cell) => [`${String(cell.row)}:${String(cell.col)}`, cell]));
  return (
    <table
      className="xtab-grid"
      style={{ '--cols': cols.length } as CSSProperties}
      onKeyDown={(e) => {
        keyDown(e, onUp);
      }}
    >
      <caption className="visually-hidden">
        {`${xtab.measure.measure} by ${AXIS_NAMES[xtab.rowAxis]} and ${AXIS_NAMES[xtab.colAxis]}`}
      </caption>
      <thead>
        <HeaderRow xtab={xtab} cols={cols} />
      </thead>
      <tbody>
        {rows.map((row, r) => (
          <tr key={row.name}>
            <th scope="row" className="row-name">
              {row.name}
            </th>
            {cols.map((col, c) => {
              const cell = at.get(`${String(r)}:${String(c)}`);
              return (
                <Cell
                  key={col.name}
                  cell={cell}
                  rows={[r, c]}
                  names={[row.name, col.name]}
                  unit={unit}
                  paint={(cell && swatches[cell.colorStep]) || neutral}
                  onPoint={onPoint}
                  onPick={() => {
                    onPoint(null);
                    navigate({
                      ...filterPatch(xtab.rowAxis, row.name),
                      ...filterPatch(xtab.colAxis, col.name),
                      selected: null,
                    });
                  }}
                />
              );
            })}
            <td className="num total">{compact(row.total)}</td>
          </tr>
        ))}
      </tbody>
      <tfoot>
        <tr>
          <th scope="row">Total</th>
          {cols.map((col) => (
            <td className="num total" key={col.name}>
              {compact(col.total)}
            </td>
          ))}
          <td className="num total">{compact(xtab.total)}</td>
        </tr>
      </tfoot>
    </table>
  );
}
