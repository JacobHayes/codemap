import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { type Page } from '@playwright/test';
import { expect, test } from './fixtures';
import { STATES } from './states';
import { expectSound } from './structure';
import type { ViewState } from '../src/viewState';

// The walk visits every view state at two widths in both themes and checks each one. With
// CODEMAP_SHOTS set (`mise run screenshots`) it also saves a picture per visit.
const WIDTHS = [1440, 1024];
const THEMES = ['light', 'dark'] as const;
const HEIGHT = 900;
const shotsDir = process.env['CODEMAP_SHOTS'];

async function settle(page: Page): Promise<void> {
  await page.evaluate(
    () =>
      new Promise((resolve) => {
        requestAnimationFrame(() => {
          requestAnimationFrame(resolve);
        });
      }),
  );
}

for (const [name, spec] of Object.entries(STATES) as [ViewState, (typeof STATES)[ViewState]][]) {
  test(`view state ${name} is sound at every width and theme`, async ({ page, request }) => {
    const url = await spec.url(request);
    for (const colorScheme of THEMES) {
      for (const width of WIDTHS) {
        await page.setViewportSize({ width, height: HEIGHT });
        await page.emulateMedia({ colorScheme });
        await page.goto(url);
        // The view's own marker: an expanded area has a ready evidence panel inside it too.
        await expect(page.locator('main [data-view][data-state="ready"]')).toBeVisible();
        if (name.startsWith('treemap') && name !== 'treemap-empty') {
          await expect(page.locator('.treemap')).not.toHaveAttribute('data-tiles', '0');
        }
        await page.evaluate(() => document.fonts.ready);
        await spec.act?.(page);
        await expect(page.locator('main [data-view]')).toHaveAttribute('data-view', name);
        await settle(page);
        await expectSound(page);
        if (shotsDir !== undefined) {
          mkdirSync(shotsDir, { recursive: true });
          const file = `${name}-${String(width)}-${colorScheme}.png`;
          await page.screenshot({ path: join(shotsDir, file) });
        }
      }
    }
  });
}
