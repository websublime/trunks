// What every page of the console must hold: nothing blocked by the Content-Security-Policy,
// no script error, no failed request, and no accessibility violation.

import AxeBuilder from '@axe-core/playwright';
import { expect, type Page } from '@playwright/test';

/** Collects everything that went wrong in the page. Call before navigating. */
export function watch(page: Page): string[] {
  const problems: string[] = [];
  page.on('console', (message) => {
    if (message.type() === 'error') problems.push(`console: ${message.text()}`);
  });
  page.on('pageerror', (error) => problems.push(`script: ${error.message}`));
  page.on('requestfailed', (request) =>
    problems.push(`request: ${request.method()} ${request.url()} (${request.failure()?.errorText})`),
  );
  return problems;
}

/** The console has finished working out who is looking. */
export async function ready(page: Page): Promise<void> {
  await expect(page.locator('html')).toHaveAttribute('data-state', 'ready');
}

/** WCAG 2.1 A and AA, as far as a tool can tell. */
export async function accessible(page: Page): Promise<void> {
  const results = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze();
  const violations = results.violations.map(
    (v) => `${v.id}: ${v.help} (${v.nodes.map((n) => n.target.join(' ')).join(', ')})`,
  );
  expect(violations).toEqual([]);
}
