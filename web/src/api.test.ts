import { afterEach, expect, it, vi } from 'vitest';
import { ask } from './api';

afterEach(() => {
  vi.unstubAllGlobals();
});

it('asks relative to the page, so a reverse proxy can mount the server under a path prefix', async () => {
  const fetchMock = vi.fn(() => Promise.resolve(new Response('{}')));
  vi.stubGlobal('fetch', fetchMock);
  await ask({ kind: 'areas' } as Parameters<typeof ask>[0]);
  const url = (fetchMock.mock.calls[0] as unknown as [string])[0];
  expect(url.startsWith('/')).toBe(false);
  expect(new URL(url, 'https://host/codemap/x/').pathname).toBe('/codemap/x/api/query');
});
