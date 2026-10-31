import type { APIRequestContext, Page } from '@playwright/test';
import type { ViewState } from '../src/viewState';

export type StateSpec = {
  /** The address that reaches the state. */
  url: (request: APIRequestContext) => Promise<string> | string;
  /** What a person does on the page, when the address alone does not reach it. */
  act?: (page: Page) => Promise<void>;
};

type Located = { path: string; startLine: number; endLine: number; area: string };

/** A finding's evidence in the served project, preferring one near the top of its file so the
 * first page of the file shows it. */
export async function evidence(request: APIRequestContext): Promise<Located> {
  const ask = async (data: object): Promise<unknown> =>
    (await request.post('/api/query', { data })).json();
  const { areas } = (await ask({ kind: 'areas', top: 20 })) as { areas: { subject: string }[] };
  const found: Located[] = [];
  for (const { subject } of areas) {
    const detail = (await ask({ kind: 'area', subject })) as {
      findings: { area: string; evidence: Omit<Located, 'area'>[] }[];
    };
    found.push(...detail.findings.flatMap((f) => f.evidence.map((e) => ({ ...e, area: f.area }))));
  }
  const chosen = found.find((e) => e.endLine < 250) ?? found[0];
  if (chosen === undefined) throw new Error('the served project has no evidence to open');
  return chosen;
}

// One entry per view state, keyed by the app's own list of them: a new state does not compile
// until it is pictured here.
export const STATES: Record<ViewState, StateSpec> = {
  'treemap-root': { url: () => '/' },
  'treemap-drilled': { url: () => '/?path=crates/core' },
  'treemap-selected': { url: () => '/?sel=crates' },
  'treemap-filtered': { url: () => '/?role=test' },
  'treemap-generated-shown': { url: () => '/?include-generated=true' },
  'treemap-colour': { url: () => '/?color=handler_lines' },
  'treemap-ratio-colour': { url: () => '/?color=test_to_prod_ratio' },
  'treemap-tooltip': {
    url: () => '/',
    act: async (page) => {
      await page.getByRole('group', { name: /^Treemap/ }).focus();
      await page.keyboard.press('ArrowRight');
    },
  },
  // Nothing in a codemap checkout is vendored Python.
  'treemap-empty': { url: () => '/?role=vendored&language=python' },
  xtab: { url: () => '/?view=xtab' },
  'xtab-by-path': { url: () => '/?view=xtab&rows=path&cols=language' },
  'xtab-empty': { url: () => '/?view=xtab&role=vendored&language=python' },
  'source-file': {
    url: async (request) => `/?source=${(await evidence(request)).path}`,
  },
  'source-range': {
    url: async (request) => {
      const { path, startLine, endLine } = await evidence(request);
      return `/?source=${path}&lines=${String(Math.max(1, startLine - 20))}-${String(endLine + 20)}`;
    },
  },
  'area-queue': { url: () => '/?view=areas' },
  'area-expanded': {
    url: async (request) => {
      const response = await request.post('/api/query', { data: { kind: 'areas', top: 1 } });
      const answer = (await response.json()) as { areas: { subject: string }[] };
      const subject = answer.areas[0]?.subject;
      if (subject === undefined) throw new Error('the served project has no areas to unfold');
      return `/?view=areas&area=${subject}`;
    },
  },
};
