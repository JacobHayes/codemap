import { hierarchy, treemap, treemapSquarify, type HierarchyRectangularNode } from 'd3-hierarchy';
import type { Scale, Tree } from '../../generated/Answer';
import { amount } from '../../format';

// The server decides sizes, totals and colour steps; the layout only places them (ARCHITECTURE,
// Web UI). A node's id is its position in `nodes`, and a parent precedes its children.

/** Text drawn in a tile: a name, and for a tile without drawn children its value under it. */
type Label = { node: number; name: string; value: string | null; x: number; y: number };

/** Plain data. Index 0 is the tree's root; a node's parent has a smaller index. Rects are in CSS px. */
export type Scene = {
  width: number;
  height: number;
  count: number;
  nodeId: Int32Array;
  parent: Int32Array;
  /** Nothing is drawn inside this tile: a leaf, or a directory aggregated at the depth limit. */
  isTile: Uint8Array;
  /** The tile stands for a directory or a file, so opening it shows its contents or its source. */
  canOpen: Uint8Array;
  colorStep: Int16Array; // -1: no value, drawn neutral
  x0: Float64Array;
  y0: Float64Array;
  x1: Float64Array;
  y1: Float64Array;
  /** Display name per tile; a collapsed directory chain is its joined path. */
  name: string[];
  labels: Label[];
  scale: Scale | null;
};

// Label metrics: the 12px floor (DESIGN, Typography) in IBM Plex Mono, whose advance is 0.6em.
export const LABEL_PX = 12;
export const CHAR_PX = LABEL_PX * 0.6;
export const LABEL_FONT = `500 ${String(LABEL_PX)}px "IBM Plex Mono", ui-monospace, monospace`;
const LABEL_PAD_PX = 6;
const LINE_PX = 16;
const NAME_CHARS_MIN = 6;
// Tiles under this size are not drawn; pointing at their place means the group around them.
const VISIBLE_PX = 3;
// Space between tiles: small between leaves, more between groups; a group also pads its content,
// and gets a header band when its name fits.
const GUTTER_TILE_PX = 2;
const GUTTER_GROUP_PX = 6;
const GROUP_PAD_PX = 6;
const BAND_PX = 24;
const BAND_MIN_PX = 2 * BAND_PX;

// A directory chain (each with one directory child) is one item: `index` is the deepest directory
// and `label` the joined path, so the chain takes one header band.
type Item = { index: number; label: string; mass: number; children?: Item[] };

const sizeOf = (input: Tree, i: number) => {
  const s = input.nodes[i]?.size ?? 0;
  return Number.isFinite(s) && s > 0 ? s : 0;
};

// Nodes with nothing under them take no area, so they are left out before chains are collapsed.
function massOf(input: Tree): Float64Array {
  const mass = new Float64Array(input.nodes.length);
  for (let i = input.nodes.length - 1; i >= 0; i--) {
    mass[i] = (mass[i] ?? 0) + sizeOf(input, i);
    const parent = input.nodes[i]?.parent;
    if (parent != null) mass[parent] = (mass[parent] ?? 0) + (mass[i] ?? 0);
  }
  return mass;
}

function build(input: Tree): Item | null {
  const items: Item[] = [];
  const mass = massOf(input);
  input.nodes.forEach((n, index) => {
    const item: Item = { index, label: n.name, mass: mass[index] ?? 0 };
    items.push(item);
    const parent = n.parent === null ? undefined : items[n.parent];
    if (parent && item.mass > 0) (parent.children ??= []).push(item);
  });
  const root = items[0];
  return root && root.mass > 0 ? collapseChains(root) : null;
}

function collapseChains(root: Item): Item {
  const pending = [root];
  for (let item = pending.pop(); item; item = pending.pop()) {
    for (
      let only = item.children?.[0];
      item.children?.length === 1 && only?.children;
      only = item.children[0]
    ) {
      item.index = only.index;
      item.label = `${item.label}/${only.label}`;
      item.children = only.children;
    }
    pending.push(...(item.children ?? []));
  }
  return root;
}

// Cells a character takes in a mono face: wide and non-Latin glyphs are counted as two, to be safe.
const cellsOf = (ch: string) => ((ch.codePointAt(0) ?? 0) > 0x7f && ch !== '…' ? 2 : 1);

/** Cells a label takes. */
export const labelCells = (text: string) => Array.from(text).reduce((n, ch) => n + cellsOf(ch), 0);

// A joined path keeps its end (the directory itself), so it is cut from the left.
function fitLabel(name: string, widthPx: number): string | null {
  const cells = Math.floor((widthPx - 2 * LABEL_PAD_PX) / CHAR_PX);
  if (cells < NAME_CHARS_MIN) return null;
  if (labelCells(name) <= cells) return name;
  const joined = name.includes('/');
  let text = '';
  for (const ch of joined ? Array.from(name).reverse() : name) {
    if (labelCells(text) + cellsOf(ch) > cells - 1) break;
    text = joined ? ch + text : text + ch;
  }
  return joined ? `…${text}` : `${text}…`;
}

type Placed = HierarchyRectangularNode<Item>;
const wide = (n: Placed) => n.x1 - n.x0 >= NAME_CHARS_MIN * CHAR_PX + 2 * LABEL_PAD_PX;
const bandFor = (n: Placed) => (wide(n) && n.y1 - n.y0 >= BAND_MIN_PX ? BAND_PX : GROUP_PAD_PX);

/** Squarified layout of the tree into width x height. */
export function layout(input: Tree, width: number, height: number): Scene {
  const root = build(input);
  const tiers = hierarchy<Item>(root ?? { index: 0, label: '', mass: 0 }, (d) => d.children)
    .sum((d) => (d.children ? 0 : d.mass))
    .sort((a, b) => (b.value ?? 0) - (a.value ?? 0) || a.data.index - b.data.index);
  const placed = treemap<Item>()
    .tile(treemapSquarify)
    .size([width, height])
    .paddingInner((n) => (n.children?.some((c) => c.children) ? GUTTER_GROUP_PX : GUTTER_TILE_PX))
    .paddingOuter(GROUP_PAD_PX)
    .paddingTop(bandFor)(tiers);
  return toScene(input, placed, width, height, root === null);
}

function toScene(
  input: Tree,
  placed: Placed,
  width: number,
  height: number,
  empty: boolean,
): Scene {
  // Breadth-first, so parents precede children. A tile with no area, or too small to see, is
  // dropped (and with it everything inside).
  const kept = empty
    ? []
    : placed
        .descendants()
        .filter((n) => n.x1 - n.x0 >= VISIBLE_PX && n.y1 - n.y0 >= VISIBLE_PX && n.data.mass > 0);
  const slot = new Map<Placed, number>(kept.map((n, i) => [n, i]));
  const keptCount = kept.length;
  const scene: Scene = {
    width,
    height,
    count: keptCount,
    nodeId: new Int32Array(keptCount),
    parent: new Int32Array(keptCount),
    isTile: new Uint8Array(keptCount),
    canOpen: new Uint8Array(keptCount),
    colorStep: new Int16Array(keptCount),
    x0: new Float64Array(keptCount),
    y0: new Float64Array(keptCount),
    x1: new Float64Array(keptCount),
    y1: new Float64Array(keptCount),
    name: [],
    labels: [],
    scale: input.scale,
  };
  kept.forEach((n, i) => {
    const src = input.nodes[n.data.index];
    scene.nodeId[i] = src?.id ?? -1;
    scene.parent[i] = n.parent ? (slot.get(n.parent) ?? -1) : -1;
    scene.isTile[i] = n.children ? 0 : 1;
    scene.canOpen[i] = src?.kind === 'directory' || src?.kind === 'file' ? 1 : 0;
    scene.colorStep[i] = src?.colorStep ?? -1;
    scene.x0[i] = n.x0;
    scene.y0[i] = n.y0;
    scene.x1[i] = n.x1;
    scene.y1[i] = n.y1;
    scene.name.push(n.data.label);
    const label = labelFor(n, i, amount(src?.total ?? 0, input.size.unit));
    if (label) scene.labels.push(label);
  });
  return scene;
}

// A group's name sits in its header band. A tile shows its name and its value in two lines when
// both fit; a short wide tile shows its name alone; a smaller one shows nothing.
function labelFor(n: Placed, node: number, value: string): Label | null {
  const x = n.x0 + LABEL_PAD_PX;
  if (n.children) {
    const name = bandFor(n) === BAND_PX ? fitLabel(n.data.label, n.x1 - n.x0) : null;
    return name === null ? null : { node, name, value: null, x, y: n.y0 + BAND_PX / 2 };
  }
  const height = n.y1 - n.y0;
  const name = height >= LINE_PX + LABEL_PAD_PX ? fitLabel(n.data.label, n.x1 - n.x0) : null;
  if (name === null) return null;
  const y = n.y0 + LABEL_PAD_PX + LINE_PX / 2;
  const valueFits = labelCells(value) <= Math.floor((n.x1 - n.x0 - 2 * LABEL_PAD_PX) / CHAR_PX);
  const twoLines = height >= 2 * LINE_PX + LABEL_PAD_PX && valueFits;
  return { node, name, value: twoLines ? value : null, x, y };
}
