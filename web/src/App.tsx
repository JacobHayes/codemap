import { useState } from 'react';
import type { Hidden } from './generated/Answer';
import { AreaQueue } from './components/AreaQueue';
import { Header } from './components/Header';
import { MapView } from './components/MapView';
import { SourceView } from './components/SourceView';
import { XtabView } from './components/XtabView';
import { RevisionContext, useServerRevision } from './revision';
import { useAnswer } from './useAnswer';
import { filterOf } from './query';
import { useUrlState } from './urlState';

export function App() {
  const revision = useServerRevision();
  return (
    <RevisionContext value={revision}>
      <Shell />
    </RevisionContext>
  );
}

function Shell() {
  const [state, navigate] = useUrlState();
  const catalogLoad = useAnswer({ kind: 'catalog' }, 'catalog');
  const catalog = catalogLoad.state === 'ready' ? catalogLoad.answer : null;
  // The tab counts what the view lists before the drill path narrows it.
  const areas = useAnswer(
    { kind: 'areas', filter: { ...filterOf({ ...state, path: '' }), includeGenerated: false } },
    'areas',
  );
  const [hidden, setHidden] = useState<Hidden | null>(null);
  return (
    <div className="app">
      <Header
        state={state}
        navigate={navigate}
        catalog={catalog}
        hidden={hidden}
        areaCount={areas.state === 'ready' ? areas.answer.matching : null}
      />
      <main className="workspace">
        {state.source !== null && (
          <SourceView state={state} navigate={navigate} path={state.source} />
        )}
        {state.source === null && state.view === 'areas' && (
          <AreaQueue state={state} navigate={navigate} />
        )}
        {state.source === null && state.view === 'map' && (
          <MapView state={state} navigate={navigate} catalog={catalog} onHidden={setHidden} />
        )}
        {state.source === null && state.view === 'xtab' && (
          <XtabView state={state} navigate={navigate} catalog={catalog} onHidden={setHidden} />
        )}
      </main>
    </div>
  );
}
