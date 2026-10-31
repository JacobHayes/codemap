import { useEffect } from 'react';
import type { Catalog, Tree, TreeNode } from '../generated/Answer';
import { exactAmount, plain } from '../format';

export type TooltipProps = {
  tree: Tree;
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
  return node.value === null ? `${description}: no value` : `${description}: ${plain(node.value)}`;
}

type BoxProps = {
  x: number;
  y: number;
  title: string;
  lines: string[];
  hint: string;
  onDismiss: () => void;
};

/** The one tooltip (DESIGN, Components): the full name, the exact values and what a click does.
 * Esc dismisses it wherever the focus is. */
export function TooltipBox({ x, y, title, lines, hint, onDismiss }: BoxProps) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onDismiss();
    };
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('keydown', onKey);
    };
  }, [onDismiss]);
  // Near the right or lower edge the tooltip opens towards the middle.
  const flipX = x > window.innerWidth / 2;
  const flipY = y > window.innerHeight * 0.6;
  return (
    <div
      role="tooltip"
      id="map-tooltip"
      className="tooltip"
      style={{
        left: x,
        top: y,
        transform: `translate(${flipX ? 'calc(-100% - 12px)' : '12px'}, ${flipY ? 'calc(-100% - 12px)' : '12px'})`,
      }}
    >
      <div className="tooltip-path">{title}</div>
      {lines.map((line) => (
        <div key={line}>{line}</div>
      ))}
      <div className="tooltip-hint">{hint}</div>
    </div>
  );
}

/** The tooltip of a treemap tile. */
export function Tooltip({ tree, catalog, node, x, y, onDismiss }: TooltipProps) {
  const colour = colourLine(tree, catalog, node);
  const lines = [exactAmount(node.total, tree.size.unit)];
  if (colour) lines.push(colour);
  if (node.hidden > 0) lines.push(`${node.hidden.toLocaleString('en-US')} deeper items not drawn`);
  return (
    <TooltipBox
      x={x}
      y={y}
      title={node.path === '' ? 'Whole codebase' : node.path}
      lines={lines}
      hint={node.kind === 'directory' ? 'Click to select, double-click to open' : 'Click to select'}
      onDismiss={onDismiss}
    />
  );
}
