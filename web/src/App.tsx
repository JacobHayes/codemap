import { useEffect, useState } from 'react';
import { ask } from './api';
import type { Answer } from './generated/Answer';

type Load = { state: 'loading' } | { state: 'failed' } | { state: 'ready'; answer: Answer };

function useOverview(): Load {
  const [load, setLoad] = useState<Load>({ state: 'loading' });
  useEffect(() => {
    ask({ kind: 'overview' }).then(
      (answer) => {
        setLoad({ state: 'ready', answer });
      },
      () => {
        setLoad({ state: 'failed' });
      },
    );
  }, []);
  return load;
}

function describe(load: Load): string {
  if (load.state === 'loading') return 'Loading';
  if (load.state === 'failed') return 'The server did not answer';
  return load.answer.note;
}

export function App() {
  const load = useOverview();
  return (
    <div className="app">
      <header className="app-header">
        <h1>codemap</h1>
        <span className="label">code mass and structure</span>
      </header>
      <main className="workspace">
        <section className="panel">
          <div className="label">Overview</div>
          <p data-testid="overview" data-state={load.state}>
            {describe(load)}
          </p>
        </section>
      </main>
    </div>
  );
}
