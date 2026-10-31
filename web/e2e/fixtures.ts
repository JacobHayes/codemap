import { test as base } from '@playwright/test';
import { expectSound } from './structure';

export { expect } from '@playwright/test';

/** Playwright's `test`, whose page is checked for soundness when the test is over, wherever the
 * test left it. */
export const test = base.extend({
  page: async ({ page }, use) => {
    await use(page);
    await expectSound(page);
  },
});
