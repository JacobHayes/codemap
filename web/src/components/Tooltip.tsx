import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import type { Catalog, Tree, TreeNode } from '../generated/Answer';
import { exactAmount, percentOf, plain } from '../format';
import type { UrlState } from '../urlState';
import { TouchingAreas, useTouching } from './TouchingAreas';

/** The catalog name of the colour measure that counts lines in open findings. */
const FINDING_SHARE = 'finding_share';
/** The count behind it. */
const FINDING_LINES = 'finding_lines';

export const isFindingMeasure = (name: string | undefined) =>
  name === FINDING_SHARE || name === FINDING_LINES;

export type TooltipProps = {
  tree: Tree;
  state: UrlState;
  catalog: Catalog | null;
  node: TreeNode;
  x: number;
  y: number;
  onDismiss: () => void;
};

function colourLine(tree: Tree, catalog: Catalog | null, node: TreeNode): string | null {
  if (!tree.scale) return null;
  const description =
    catalog?.measures.find((m) => m.name === tree.scale?.measure)?.description ?? 'Colour measure';
  if (node.value === null) return `${description}: no value`;
  if (tree.scale.measure === FINDING_SHARE && node.colorParts) {
    const { numerator, denominator } = node.colorParts;
    return `${description}: ${exactAmount(numerator, 'lines').replace(' lines', '')} of ${exactAmount(denominator, 'lines')} (${percentOf(node.value)})`;
  }
  return `${description}: ${plain(node.value)}`;
}

type BoxProps = {
  x: number;
  y: number;
  title: string;
  lines: string[];
  hint: string;
  extra?: ReactNode;
  onDismiss: () => void;
};

/** The one tooltip (DESIGN, Components): the full name, the exact values and what a click does.
 * Esc dismisses it wherever the focus is. */
export function TooltipBox({ x, y, title, lines, hint, extra, onDismiss }: BoxProps) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onDismiss();
    };
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('keydown', onKey);
    };
  }, [onDismiss]);
  // However long its content, the box stays inside the viewport: once placed it is measured and
  // moved up or down by what spills.
  const box = useRef<HTMLDivElement>(null);
  const [shift, setShift] = useState(0);
  useLayoutEffect(() => {
    const rect = box.current?.getBoundingClientRect();
    if (!rect) return;
    const margin = 8;
    const top = rect.top - shift;
    const bottom = rect.bottom - shift;
    const next = Math.max(Math.min(0, window.innerHeight - margin - bottom), margin - top);
    if (next !== shift) setShift(next);
  });
  // Near the right or lower edge the tooltip opens towards the middle.
  const flipX = x > window.innerWidth / 2;
  const flipY = y > window.innerHeight * 0.6;
  return (
    <div
      role="tooltip"
      id="map-tooltip"
      className="tooltip"
      ref={box}
      style={{
        left: x,
        top: y + shift,
        transform: `translate(${flipX ? 'calc(-100% - 12px)' : '12px'}, ${flipY ? 'calc(-100% - 12px)' : '12px'})`,
      }}
    >
      <div className="tooltip-path">{title}</div>
      {lines.map((line) => (
        <div key={line}>{line}</div>
      ))}
      {extra}
      <div className="tooltip-hint">{hint}</div>
    </div>
  );
}

/** The tooltip of a treemap tile. */
export function Tooltip({ tree, state, catalog, node, x, y, onDismiss }: TooltipProps) {
  const colour = colourLine(tree, catalog, node);
  const touching = useTouching(
    isFindingMeasure(tree.scale?.measure) && node.value !== null && node.value > 0
      ? node.path
      : null,
    state,
  );
  const lines = [exactAmount(node.total, tree.size.unit)];
  if (colour) lines.push(colour);
  if (node.hidden > 0) lines.push(`${node.hidden.toLocaleString('en-US')} deeper items not drawn`);
  return (
    <TooltipBox
      x={x}
      y={y}
      title={node.path === '' ? 'Whole codebase' : node.path}
      lines={lines}
      extra={touching && <TouchingAreas shares={touching} />}
      hint={node.kind === 'directory' ? 'Click to select, double-click to open' : 'Click to select'}
      onDismiss={onDismiss}
    />
  );
}
