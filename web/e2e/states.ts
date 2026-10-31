import type { APIRequestContext, Page } from '@playwright/test';
import type { ViewState } from '../src/viewState';

export type StateSpec = {
  /** The address that reaches the state. */
  url: (request: APIRequestContext) => Promise<string> | string;
  /** What a person does on the page, when the address alone does not reach it. */
  act?: (page: Page) => Promise<void>;
};

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
