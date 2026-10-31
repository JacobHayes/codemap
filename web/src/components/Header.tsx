import type { Catalog, Hidden } from '../generated/Answer';
import { compact } from '../format';
import {
  AXES,
  CATALOG_NAME,
  FILTER_DIMENSIONS,
  type FilterDimension,
  type Navigate,
  type UrlState,
} from '../urlState';
import { Link } from './Link';
import { AXIS_NAMES } from './XtabGrid';

type HeaderProps = {
  state: UrlState;
  navigate: Navigate;
  catalog: Catalog | null;
  areaCount: number | null;
  /** What the view's answer left out by default, if anything. */
  hidden: Hidden | null;
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
              patch={{
                view: state.view === 'xtab' ? 'xtab' : 'map',
                path: crumb.path,
                selected: null,
                source: null,
                lines: null,
              }}
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
      patch={{ view, source: null, lines: null }}
      navigate={navigate}
      current={state.source === null && state.view === view}
      className="tab"
    >
      {text}
    </Link>
  );
  return (
    <div className="tabs" role="navigation" aria-label="Views">
      {tab('map', 'Map')}
      {tab('xtab', 'Cross-tab')}
      {tab('areas', areaCount === null ? 'Areas' : `Areas (${String(areaCount)})`)}
    </div>
  );
}

function PopulationChip({
  state,
  navigate,
  hidden,
}: Pick<HeaderProps, 'state' | 'navigate' | 'hidden'>) {
  if (hidden !== null) {
    return (
      <span className="chip" data-default-filter>
        <span className="label">Hiding</span>
        {`${compact(hidden.lines)} lines of ${hidden.roles.join(' and ')} code`}
        <button
          className="ghost chip-remove"
          aria-label={`Include ${hidden.roles.join(' and ')} code`}
          onClick={() => {
            navigate({ includeGenerated: true, selected: null });
          }}
        >
          ×
        </button>
      </span>
    );
  }
  // The area queue has no population of its own to include them in.
  if (!state.includeGenerated || state.role !== null || state.view === 'areas') return null;
  return (
    <span className="chip" data-default-filter>
      <span className="label">Including</span>
      generated and vendored code
      <button
        className="ghost chip-remove"
        aria-label="Leave out generated and vendored code"
        onClick={() => {
          navigate({ includeGenerated: false, selected: null });
        }}
      >
        ×
      </button>
    </span>
  );
}

function FilterBar({ state, navigate, catalog, hidden }: Omit<HeaderProps, 'areaCount'>) {
  const dimensions = FILTER_DIMENSIONS.map((name) => ({
    name,
    values: catalog?.dimensions.find((d) => d.name === CATALOG_NAME[name])?.values ?? [],
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
      <PopulationChip state={state} navigate={navigate} hidden={hidden} />
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

function AxisPicker({ state, navigate }: Pick<HeaderProps, 'state' | 'navigate'>) {
  return (
    <>
      {(['rows', 'cols'] as const).map((field) => (
        <label className="field" key={field}>
          <span className="label">{field === 'rows' ? 'Rows' : 'Columns'}</span>
          <select
            value={state[field]}
            onChange={(e) => {
              navigate({ [field]: e.target.value });
            }}
          >
            {AXES.map((axis) => (
              <option key={axis} value={axis}>
                {AXIS_NAMES[axis]}
              </option>
            ))}
          </select>
        </label>
      ))}
    </>
  );
}

function MeasurePicker({
  state,
  navigate,
  catalog,
}: Pick<HeaderProps, 'state' | 'navigate' | 'catalog'>) {
  const measures = catalog?.measures ?? [];
  const crossTab = state.view === 'xtab';
  return (
    <div className="measures">
      {crossTab && <AxisPicker state={state} navigate={navigate} />}
      <label className="field">
        <span className="label">{crossTab ? 'Measure' : 'Size by'}</span>
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
      {!crossTab && (
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
      )}
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
      {props.state.source === null && (
        <div className="header-row">
          <FilterBar
            state={props.state}
            navigate={props.navigate}
            catalog={props.catalog}
            hidden={props.hidden}
          />
          {props.state.view !== 'areas' && (
            <MeasurePicker state={props.state} navigate={props.navigate} catalog={props.catalog} />
          )}
        </div>
      )}
    </header>
  );
}
