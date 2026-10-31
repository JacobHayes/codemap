import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { App } from './App';

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test('the shell shows the answer the server gives to the overview query', async () => {
  const fetchMock = vi.fn(() =>
    Promise.resolve(
      Response.json({
        schema: 'codemap.answer/v1',
        kind: 'overview',
        totals: { files: 3, lines: 1200, tokens: 0, nodes: 0, symbols: 0 },
      }),
    ),
  );
  vi.stubGlobal('fetch', fetchMock);
  render(<App />);
  const overview = await screen.findByText('3 files, 1,200 lines');
  expect(overview.getAttribute('data-state')).toBe('ready');
  expect(fetchMock).toHaveBeenCalledWith(
    '/api/query',
    expect.objectContaining({ body: JSON.stringify({ kind: 'overview' }) }),
  );
});
