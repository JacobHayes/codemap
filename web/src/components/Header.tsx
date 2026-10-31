import type { Catalog } from '../generated/Answer';
import {
  FILTER_DIMENSIONS,
  hidesGenerated,
  type FilterDimension,
  type Navigate,
  type UrlState,
} from '../urlState';
import { Link } from './Link';

type HeaderProps = {
  state: UrlState;
  navigate: Navigate;
  catalog: Catalog | null;
  areaCount: number | null;
};

const capital = (word: string) => word.charAt(0).toUpperCase() + word.slice(1);

function Breadcrumb({ state, navigate }: Pick<HeaderProps, 'state' | 'navigate'>) {
  const parts = state.path === '' ? [] : state.path.split('/');
  const crumbs = [
    { name: 'All files', path: '' },
    ...parts.map((name, i) => ({ name, path: parts.slice(0, i + 1).join('/') })),
  ];
  return (
    <nav aria-label="Drill path">
      <ol className="crumbs">
        {crumbs.map((crumb, i) => (
          <li key={crumb.path}>
            <Link
              state={state}
              patch={{ view: 'map', path: crumb.path, selected: null }}
              navigate={navigate}
              current={i === crumbs.length - 1}
            >
              {crumb.name}
            </Link>
          </li>
        ))}
      </ol>
    </nav>
  );
}

function Tabs({ state, navigate, areaCount }: HeaderProps) {
  const tab = (view: UrlState['view'], text: string) => (
    <Link
      state={state}
      patch={{ view }}
      navigate={navigate}
      current={state.view === view}
      className="tab"
    >
      {text}
    </Link>
  );
  return (
    <div className="tabs" role="navigation" aria-label="Views">
      {tab('map', 'Map')}
      {tab('areas', areaCount === null ? 'Areas' : `Areas (${String(areaCount)})`)}
    </div>
  );
}

function FilterBar({ state, navigate, catalog }: Omit<HeaderProps, 'areaCount'>) {
  const dimensions = FILTER_DIMENSIONS.map((name) => ({
    name,
    values: catalog?.dimensions.find((d) => d.name === name)?.values ?? [],
  }));
  return (
    <div className="filters" role="group" aria-label="Filters">
      {dimensions.map(({ name }) => {
        const value = state[name];
        return value === null ? null : (
          <span className="chip" key={name}>
            <span className="label">{name}</span>
            {value}
            <button
              className="ghost chip-remove"
              aria-label={`Remove filter ${name} ${value}`}
              onClick={() => {
                navigate({ [name]: null });
              }}
            >
              ×
            </button>
          </span>
        );
      })}
      {hidesGenerated(state) && state.view === 'map' && (
        <span className="chip" data-default-filter>
          <span className="label">Hiding</span>
          generated and vendored code
          <button
            className="ghost chip-remove"
            aria-label="Show generated and vendored code"
            onClick={() => {
              navigate({ hideGenerated: false, selected: null });
            }}
          >
            ×
          </button>
        </span>
      )}
      <select
        aria-label="Add a filter"
        value=""
        onChange={(e) => {
          const [dimension, value] = e.target.value.split('=') as [FilterDimension, string];
          navigate({ [dimension]: value, selected: null });
        }}
      >
        <option value="">Add a filter</option>
        {dimensions.map(({ name, values }) => (
          <optgroup key={name} label={capital(name)}>
            {values.map((value) => (
              <option key={value} value={`${name}=${value}`}>
                {value}
              </option>
            ))}
          </optgroup>
        ))}
      </select>
    </div>
  );
}

function MeasurePicker({ state, navigate, catalog }: Omit<HeaderProps, 'areaCount'>) {
  const measures = catalog?.measures ?? [];
  return (
    <div className="measures">
      <label className="field">
        <span className="label">Size by</span>
        <select
          value={state.size}
          onChange={(e) => {
            navigate({ size: e.target.value });
          }}
        >
          {measures
            .filter((m) => m.measure.kind === 'sum')
            .map((m) => (
              <option key={m.name} value={m.name}>
                {m.description}
              </option>
            ))}
        </select>
      </label>
      <label className="field">
        <span className="label">Colour by</span>
        <select
          value={state.color ?? ''}
          onChange={(e) => {
            navigate({ color: e.target.value === '' ? null : e.target.value });
          }}
        >
          <option value="">No colour</option>
          {measures.map((m) => (
            <option key={m.name} value={m.name}>
              {m.description}
            </option>
          ))}
        </select>
      </label>
    </div>
  );
}

/** The header: the drill path, the views, the filters and the measures. */
export function Header(props: HeaderProps) {
  return (
    <header className="app-header">
      <div className="header-row">
        <h1>codemap</h1>
        <Breadcrumb state={props.state} navigate={props.navigate} />
        <Tabs {...props} />
      </div>
      <div className="header-row">
        <FilterBar state={props.state} navigate={props.navigate} catalog={props.catalog} />
        {props.state.view === 'map' && (
          <MeasurePicker state={props.state} navigate={props.navigate} catalog={props.catalog} />
        )}
      </div>
    </header>
  );
}
