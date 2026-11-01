import { useState } from 'react';
import { Treemap, type Pointed } from '../canvas/treemap/Treemap';
import type { Catalog, Hidden, Tree } from '../generated/Answer';
import type { Query } from '../generated/Query';
import { exactAmount } from '../format';
import { filterOf } from '../query';
import { useAnswer } from '../useAnswer';
import { useReportHidden } from '../useReportHidden';
import { type Navigate, type UrlState } from '../urlState';
import { viewStateOf } from '../viewState';
import { Empty } from './Empty';
import { Legend } from './Legend';
import { isFindingMeasure, Tooltip } from './Tooltip';
import { TouchingAreas, useTouching } from './TouchingAreas';

/** The tree query for a state; unset fields are left out so the server's defaults apply. */
function treeQuery(state: UrlState): Query {
  const query: Extract<Query, { kind: 'tree' }> = {
    kind: 'tree',
    hierarchy: 'path',
    size: state.size,
    filter: filterOf(state),
  };
  if (state.color !== null) query.color = state.color;
  return query;
}

const parentOf = (path: string) => path.split('/').slice(0, -1).join('/');

type MapProps = {
  state: UrlState;
  navigate: Navigate;
  catalog: Catalog | null;
  /** Tells the filter bar what the answer left out. */
  onHidden: (hidden: Hidden | null) => void;
};

function SelectionBar({
  tree,
  state,
  navigate,
}: { tree: Tree } & Pick<MapProps, 'state' | 'navigate'>) {
  const node = tree.nodes.find((n) => n.id > 0 && n.path === state.selected);
  const touching = useTouching(
    isFindingMeasure(tree.scale?.measure) && node && node.value !== null && node.value > 0
      ? node.path
      : null,
    state,
  );
  // Reserved whether or not something is selected, so choosing a tile never moves the map.
  const listed = isFindingMeasure(tree.scale?.measure);
  if (!node) {
    return (
      <div className={listed ? 'selection-block reserved' : 'selection-block'}>
        <p className="selection muted">Nothing selected</p>
      </div>
    );
  }
  return (
    <div className={listed ? 'selection-block reserved' : 'selection-block'}>
      <p className="selection" aria-live="polite">
        <span className="label">Selected</span>
        <span className="mono selection-path">{node.path}</span>
        <span>{exactAmount(node.total, tree.size.unit)}</span>
        {node.kind === 'directory' && (
          <button
            onClick={() => {
              navigate({ path: node.path, selected: null });
            }}
          >
            Open
          </button>
        )}
        {node.kind === 'file' && (
          <button
            onClick={() => {
              navigate({ source: node.path, lines: null });
            }}
          >
            View source
          </button>
        )}
      </p>
      {touching && <TouchingAreas shares={touching} link={{ state, navigate }} />}
    </div>
  );
}

/** The map: the treemap, its tooltip, the selection and the colour legend. */
export function MapView({ state, navigate, catalog, onHidden }: MapProps) {
  const load = useAnswer(treeQuery(state), 'tree');
  const [pointed, setPointed] = useState<Pointed | null>(null);
  const hidden = load.state === 'ready' ? load.answer.hidden : null;
  useReportHidden(hidden, onHidden);
  if (load.state !== 'ready') {
    return (
      <section className="map" data-state={load.state}>
        <p className="muted notice">
          {load.state === 'loading' ? 'Loading' : 'The server did not answer'}
        </p>
      </section>
    );
  }
  const tree = load.answer;
  const empty = tree.nodes.length === 0;
  const node = pointed ? tree.nodes[pointed.nodeId] : undefined;
  const selectedId = tree.nodes.find((n) => n.id > 0 && n.path === state.selected)?.id ?? null;
  const colourDef = catalog?.measures.find((m) => m.name === state.color);
  const view = viewStateOf(state, {
    empty,
    tooltip: node !== undefined,
    colourIsRatio: colourDef?.measure.kind === 'ratio',
  });
  return (
    <section className="map" data-state="ready" data-view={view}>
      <div className="map-canvas">
        {empty ? (
          <Empty state={state} navigate={navigate} />
        ) : (
          <Treemap
            tree={tree}
            selectedId={selectedId}
            onPoint={setPointed}
            onSelect={(id) => {
              navigate({ selected: tree.nodes[id ?? -1]?.path ?? null }, 'replace');
            }}
            onDrill={(id) => {
              const drilled = tree.nodes[id];
              if (drilled?.kind === 'file') navigate({ source: drilled.path, lines: null });
              else navigate({ path: drilled?.path ?? state.path, selected: null });
            }}
            onUp={() => {
              if (state.path !== '') navigate({ path: parentOf(state.path), selected: state.path });
            }}
          />
        )}
        {pointed && node && (
          <Tooltip
            tree={tree}
            state={state}
            catalog={catalog}
            node={node}
            x={pointed.x}
            y={pointed.y}
            onDismiss={() => {
              setPointed(null);
            }}
          />
        )}
      </div>
      {!empty && (
        <footer className="map-foot">
          <SelectionBar tree={tree} state={state} navigate={navigate} />
          {tree.scale && <Legend scale={tree.scale} catalog={catalog} />}
        </footer>
      )}
    </section>
  );
}
