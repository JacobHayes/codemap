import { cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import type { Query } from './generated/Query';
import { RevisionContext } from './revision';
import { useAnswer } from './useAnswer';

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const totals = { files: 1, lines: 1, tokens: 1, nodes: 1, symbols: 1 };

// Each request is answered by hand, so a test decides which one comes back first.
function server() {
  const pending: { body: string; reply: (kind: string) => void }[] = [];
  vi.stubGlobal(
    'fetch',
    vi.fn(
      (_url: string, init: { body: string }) =>
        new Promise<Response>((resolve) => {
          pending.push({
            body: init.body,
            reply: (kind) => {
              resolve(Response.json({ schema: 'codemap.answer/v1', kind, totals }));
            },
          });
        }),
    ),
  );
  return pending;
}

test('an answer is shown only for the query that asked for it', async () => {
  const pending = server();
  const first = { kind: 'catalog' } as Query;
  const second = { kind: 'overview' } as Query;
  const { result, rerender } = renderHook(
    ({ query, kind }: { query: Query; kind: 'catalog' | 'overview' }) => useAnswer(query, kind),
    { initialProps: { query: first, kind: 'catalog' as 'catalog' | 'overview' } },
  );
  rerender({ query: second, kind: 'overview' });
  await waitFor(() => {
    expect(pending).toHaveLength(2);
  });
  // The earlier query answers last: its answer must not be shown for the later one.
  pending[1]?.reply('overview');
  await waitFor(() => {
    expect(result.current.state).toBe('ready');
  });
  pending[0]?.reply('catalog');
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(result.current.state === 'ready' && result.current.answer.kind).toBe('overview');
});

test('a query that changes goes back to loading until its own answer arrives', async () => {
  const pending = server();
  const { result, rerender } = renderHook(
    ({ query }: { query: Query }) => useAnswer(query, 'overview'),
    {
      initialProps: { query: { kind: 'overview' } as Query },
    },
  );
  pending[0]?.reply('overview');
  await waitFor(() => {
    expect(result.current.state).toBe('ready');
  });
  rerender({ query: { kind: 'catalog' } });
  expect(result.current.state).toBe('loading');
});

test('a new index revision asks the same query again and keeps the answer meanwhile', async () => {
  const pending = server();
  let revision = 1;
  const { result, rerender } = renderHook(() => useAnswer({ kind: 'overview' }, 'overview'), {
    wrapper: ({ children }) => <RevisionContext value={revision}>{children}</RevisionContext>,
  });
  pending[0]?.reply('overview');
  await waitFor(() => {
    expect(result.current.state).toBe('ready');
  });
  revision = 2;
  rerender();
  await waitFor(() => {
    expect(pending).toHaveLength(2);
  });
  expect(result.current.state).toBe('ready');
});
