import { expect, test } from './fixtures';
import { evidence } from './states';

test('a file tile opens its source and Close returns to the map', async ({ page }) => {
  await page.goto('/?path=crates/core/src&sel=crates/core/src/measure.rs');
  await expect(page.locator('.treemap')).not.toHaveAttribute('data-tiles', '0');
  await page.getByRole('button', { name: 'View source' }).click();
  await expect(page).toHaveURL(/source=crates\/core\/src\/measure.rs/);
  const code = page.getByRole('table');
  await expect(code).toContainText('pub struct Filter');
  await page.getByRole('button', { name: 'Close' }).click();
  await expect(page).not.toHaveURL(/source=/);
  await expect(page.locator('.treemap')).toBeVisible();
});

test('a long file comes in pages and the address follows them', async ({ page }) => {
  await page.goto('/?source=crates/core/src/semantic.rs');
  const footer = page.locator('.map-foot');
  await expect(footer).toContainText('follow');
  await page.getByRole('link', { name: 'Next lines' }).click();
  await expect(page).toHaveURL(/lines=301-/);
  await expect(page.getByRole('table')).toContainText('301');
  await page.getByRole('link', { name: 'From the top' }).click();
  await expect(page).not.toHaveURL(/lines=/);
});

test('evidence opens its lines highlighted, linked back to the area', async ({ page, request }) => {
  const { path, area } = await evidence(request);
  await page.goto(`/?view=areas&area=${area}`);
  await page.locator('.evidence a', { hasText: path }).first().click();
  await expect(page).toHaveURL(/source=/);
  await expect(page.locator('.code tr.flagged').first()).toBeVisible();
  await page.locator('.source-findings summary').click();
  await page.locator('.source-findings a').first().click();
  await expect(page).toHaveURL(/view=areas/);
});
