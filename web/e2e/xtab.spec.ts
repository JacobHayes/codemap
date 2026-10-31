import { expect, test } from './fixtures';

test('a cell filters to both of its values and the totals follow', async ({ page }) => {
  await page.goto('/?view=xtab');
  const grid = page.getByRole('table');
  await expect(grid).toBeVisible();
  await expect(grid.getByRole('row').last()).toContainText('Total');
  await grid.getByRole('button').first().click();
  const params = new URL(page.url()).searchParams;
  expect(params.get('role')).not.toBeNull();
  expect(params.get('concern')).not.toBeNull();
  expect(params.get('view')).toBe('xtab');
  await expect(grid.getByRole('row')).toHaveCount(3);
});

test('the pickers choose the dimensions and the address remembers them', async ({ page }) => {
  await page.goto('/?view=xtab');
  await page.getByLabel('Rows').selectOption('language');
  await page.getByLabel('Columns').selectOption('role');
  await expect(page).toHaveURL(/rows=language/);
  await expect(page).toHaveURL(/cols=role/);
  await expect(page.getByRole('table')).toContainText('Language by Role');
});

test('the keyboard moves between cells, shows the exact value and goes up a level', async ({
  page,
}) => {
  await page.goto('/?view=xtab&path=crates&rows=path&cols=language');
  const cells = page.getByRole('table').getByRole('button');
  await cells.first().focus();
  await expect(page.getByRole('tooltip')).toContainText('lines');
  await page.keyboard.press('Escape');
  await expect(page.getByRole('tooltip')).toHaveCount(0);
  const first = await page.evaluate(() => document.activeElement?.getAttribute('aria-label'));
  await page.keyboard.press('ArrowDown');
  const moved = await page.evaluate(() => document.activeElement?.getAttribute('aria-label'));
  expect(moved).not.toBe(first);
  await page.keyboard.press('Backspace');
  await expect.poll(() => new URL(page.url()).searchParams.get('path')).toBeNull();
});
