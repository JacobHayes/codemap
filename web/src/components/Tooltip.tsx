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

/** The one tooltip (DESIGN, Components): the full path, the exact values and what a click does.
 * Esc dismisses it wherever the focus is. */
export function Tooltip({ tree, catalog, node, x, y, onDismiss }: TooltipProps) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onDismiss();
    };
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('keydown', onKey);
    };
  }, [onDismiss]);
  const colour = colourLine(tree, catalog, node);
  // Near the right or lower edge the tooltip opens towards the middle.
  const flipX = x > window.innerWidth / 2;
  const flipY = y > window.innerHeight * 0.6;
  const isFolder = node.kind === 'directory';
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
      <div className="tooltip-path">{node.path === '' ? 'Whole codebase' : node.path}</div>
      <div>{exactAmount(node.total, tree.size.unit)}</div>
      {colour && <div>{colour}</div>}
      {node.hidden > 0 && (
        <div>{`${node.hidden.toLocaleString('en-US')} deeper items not drawn`}</div>
      )}
      <div className="tooltip-hint">
        {isFolder ? 'Click to select, double-click to open' : 'Click to select'}
      </div>
    </div>
  );
}
