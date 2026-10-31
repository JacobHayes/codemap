import { useState } from 'react';
import { Treemap, type Pointed } from '../canvas/treemap/Treemap';
import type { Catalog, Tree } from '../generated/Answer';
import type { Query } from '../generated/Query';
import { exactAmount } from '../format';
import { useAnswer } from '../useAnswer';
import { hasFilters, type Navigate, type UrlState } from '../urlState';
import { viewStateOf } from '../viewState';
import { Legend } from './Legend';
import { Tooltip } from './Tooltip';

/** The tree query for a state; unset fields are left out so the server's defaults apply. */
function treeQuery(state: UrlState): Query {
  const filter: Extract<Query, { kind: 'tree' }>['filter'] = {};
  if (state.path !== '') filter.path = state.path;
  if (state.role !== null) filter.role = state.role;
  if (state.concern !== null) filter.concern = state.concern;
  if (state.language !== null) filter.language = state.language;
  const query: Extract<Query, { kind: 'tree' }> = {
    kind: 'tree',
    hierarchy: 'path',
    size: state.size,
    filter,
  };
  if (state.color !== null) query.color = state.color;
  return query;
}

const parentOf = (path: string) => path.split('/').slice(0, -1).join('/');

type MapProps = {
  state: UrlState;
  navigate: Navigate;
  catalog: Catalog | null;
};

function SelectionBar({
  tree,
  state,
  navigate,
}: { tree: Tree } & Pick<MapProps, 'state' | 'navigate'>) {
  const node = tree.nodes.find((n) => n.id > 0 && n.path === state.selected);
  if (!node) return <p className="selection muted">Nothing selected</p>;
  return (
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
    </p>
  );
}

function Empty({ state, navigate }: Pick<MapProps, 'state' | 'navigate'>) {
  return (
    <div className="empty">
      <p>No lines match these filters.</p>
      {hasFilters(state) && (
        <button
          onClick={() => {
            navigate({ role: null, concern: null, language: null, selected: null });
          }}
        >
          Clear filters
        </button>
      )}
    </div>
  );
}

/** The map: the treemap, its tooltip, the selection and the colour legend. */
export function MapView({ state, navigate, catalog }: MapProps) {
  const load = useAnswer(treeQuery(state), 'tree');
  const [pointed, setPointed] = useState<Pointed | null>(null);
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
              navigate({ path: tree.nodes[id]?.path ?? state.path, selected: null });
            }}
            onUp={() => {
              if (state.path !== '') navigate({ path: parentOf(state.path), selected: state.path });
            }}
          />
        )}
        {pointed && node && (
          <Tooltip
            tree={tree}
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
