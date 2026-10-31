import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { App } from './App';

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

test('the shell shows the answer the server gives to the overview query', async () => {
  const fetchMock = vi.fn(() =>
    Promise.resolve(Response.json({ kind: 'placeholder', note: 'from the server' })),
  );
  vi.stubGlobal('fetch', fetchMock);
  render(<App />);
  const overview = await screen.findByText('from the server');
  expect(overview.getAttribute('data-state')).toBe('ready');
  expect(fetchMock).toHaveBeenCalledWith(
    '/api/query',
    expect.objectContaining({ body: JSON.stringify({ kind: 'overview' }) }),
  );
});
