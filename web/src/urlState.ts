import { useCallback, useMemo, useSyncExternalStore } from 'react';
import type { Axis } from './generated/Query';

// The query string is the only state store (ARCHITECTURE, Web UI): the view, the drill path, the
// filters, the measures and the selection. Only values that differ from the defaults are written,
// so the stock view is the bare address.

type View = 'map' | 'xtab' | 'areas';

/** The dimensions a cross-tab can lay out, in the order the pickers list them. */
export const AXES: readonly Axis[] = ['role', 'concern', 'language', 'kind', 'path'];

export type UrlState = {
  view: View;
  /** The drill path: the folder the map shows, relative to the root. Empty for the whole codebase. */
  path: string;
  role: string | null;
  concern: string | null;
  language: string | null;
  /** The kind of the innermost symbol a line sits in. */
  kind: string | null;
  /** Whether generated and vendored lines count; the server leaves them out otherwise. */
  includeGenerated: boolean;
  /** The cross-tab's row and column dimensions. */
  rows: Axis;
  cols: Axis;
  /** The size measure's catalog name; the cross-tab's measure too. */
  size: string;
  /** The colour measure's catalog name; null for no colour. */
  color: string | null;
  /** The selected tile's path. */
  selected: string | null;
  /** The area unfolded in the queue, by subject. */
  area: string | null;
};

export const DEFAULT_STATE: UrlState = {
  view: 'map',
  path: '',
  role: null,
  concern: null,
  language: null,
  kind: null,
  includeGenerated: false,
  rows: 'role',
  cols: 'concern',
  size: 'lines',
  color: null,
  selected: null,
  area: null,
};

export const FILTER_DIMENSIONS = ['role', 'concern', 'language', 'kind'] as const;
export type FilterDimension = (typeof FILTER_DIMENSIONS)[number];

/** The catalog's name for a filter dimension. */
export const CATALOG_NAME: Record<FilterDimension, string> = {
  role: 'role',
  concern: 'concern',
  language: 'language',
  kind: 'symbolKind',
};

// Query-string keys, one per field.
const KEYS: Record<keyof UrlState, string> = {
  view: 'view',
  path: 'path',
  role: 'role',
  concern: 'concern',
  language: 'language',
  kind: 'kind',
  includeGenerated: 'include-generated',
  rows: 'rows',
  cols: 'cols',
  size: 'size',
  color: 'color',
  selected: 'sel',
  area: 'area',
};

const viewOf = (text: string | null): View => (text === 'areas' || text === 'xtab' ? text : 'map');

const axisOf = (text: string | null, fallback: Axis): Axis =>
  AXES.find((axis) => axis === text) ?? fallback;

export function parseState(search: string): UrlState {
  const params = new URLSearchParams(search);
  const text = (field: keyof UrlState) => params.get(KEYS[field]);
  return {
    view: viewOf(text('view')),
    path: (text('path') ?? '').replace(/^\/+|\/+$/g, ''),
    role: text('role'),
    concern: text('concern'),
    language: text('language'),
    kind: text('kind'),
    includeGenerated: text('includeGenerated') === 'true',
    rows: axisOf(text('rows'), DEFAULT_STATE.rows),
    cols: axisOf(text('cols'), DEFAULT_STATE.cols),
    size: text('size') ?? DEFAULT_STATE.size,
    color: text('color'),
    selected: text('selected'),
    area: text('area'),
  };
}

// Slashes stay readable in a path.
const encode = (value: string) => encodeURIComponent(value).replaceAll('%2F', '/');

/** The query string for a state, `?` included; empty for the defaults. */
export function serializeState(state: UrlState): string {
  const parts = (Object.keys(KEYS) as (keyof UrlState)[])
    .filter((field) => state[field] !== DEFAULT_STATE[field])
    .map((field) => `${KEYS[field]}=${encode(String(state[field]))}`);
  return parts.length === 0 ? '' : `?${parts.join('&')}`;
}

export const hasFilters = (state: UrlState) =>
  FILTER_DIMENSIONS.some((dimension) => state[dimension] !== null);

const CHANGED = 'codemap:urlchange';

function subscribe(notify: () => void): () => void {
  window.addEventListener('popstate', notify);
  window.addEventListener(CHANGED, notify);
  return () => {
    window.removeEventListener('popstate', notify);
    window.removeEventListener(CHANGED, notify);
  };
}

export type Navigate = (patch: Partial<UrlState>, mode?: 'push' | 'replace') => void;

/** The state in the address, and a function that changes it. A change is a history entry unless
 * it is only a selection moving, which replaces the current one. */
export function useUrlState(): [UrlState, Navigate] {
  const search = useSyncExternalStore(subscribe, () => window.location.search);
  const state = useMemo(() => parseState(search), [search]);
  const navigate = useCallback<Navigate>(
    (patch, mode = 'push') => {
      const next = serializeState({ ...state, ...patch });
      const url = `${window.location.pathname}${next}`;
      if (next === search) return;
      if (mode === 'push') window.history.pushState(null, '', url);
      else window.history.replaceState(null, '', url);
      window.dispatchEvent(new Event(CHANGED));
    },
    [state, search],
  );
  return [state, navigate];
}
