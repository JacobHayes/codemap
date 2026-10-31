import { useState } from 'react';
import type { Hidden } from './generated/Answer';
import { AreaQueue } from './components/AreaQueue';
import { Header } from './components/Header';
import { MapView } from './components/MapView';
import { RevisionContext, useServerRevision } from './revision';
import { useAnswer } from './useAnswer';
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
  const areas = useAnswer({ kind: 'areas' }, 'areas');
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
        {state.view === 'areas' ? (
          <AreaQueue state={state} navigate={navigate} />
        ) : (
          <MapView state={state} navigate={navigate} catalog={catalog} onHidden={setHidden} />
        )}
      </main>
    </div>
  );
}
