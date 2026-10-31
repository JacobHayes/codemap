import type { Query } from './generated/Query';
import type { UrlState } from './urlState';

export type Filter = Extract<Query, { kind: 'tree' }>['filter'];

/** The filter for a state; unset dimensions are left out so the server's defaults apply. */
export function filterOf(state: UrlState): Filter {
  const filter: Filter = { includeGenerated: state.includeGenerated };
  if (state.path !== '') filter.path = state.path;
  if (state.role !== null) filter.role = state.role;
  if (state.concern !== null) filter.concern = state.concern;
  if (state.language !== null) filter.language = state.language;
  if (state.kind !== null) filter.symbolKind = state.kind;
  return filter;
}
