import { expect, test } from './fixtures';

test('the page loads the shell and draws the map the server answers', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('banner')).toBeVisible();
  await expect(page.locator('.map')).toHaveAttribute('data-state', 'ready');
  await expect(page.locator('.treemap')).not.toHaveAttribute('data-tiles', '0');
});
