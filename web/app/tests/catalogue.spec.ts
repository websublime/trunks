// The read-only catalogue: servers, tools, prompts, resources, tool groups and diagnostics.

import { expect, test, type Page } from '@playwright/test';

import { accessible, ready, watch } from './console';
import { FAKE_UPSTREAM, Server } from './server';

const rows = (page: Page) => page.locator('table.tbl > tbody > tr');
const main = (page: Page) => page.getByRole('main');

test.describe('development', () => {
  let server: Server;

  test.beforeAll(async () => {
    server = await Server.start('development');
    await server.cli('register', '--name', 'files', '--command', FAKE_UPSTREAM, '--description', 'Workspace files');
    await server.cli('register', '--name', 'notes', '--command', FAKE_UPSTREAM, '--session-mode', 'stateful');
    await server.cli('register', '--name', 'search', '--command', FAKE_UPSTREAM);
    await server.cli('disable', 'tool', 'notes__fail');
    const group = async (name: string, definition: object) =>
      server.cli('create', 'group', '-c', await server.file(`${name}.json`, { name, ...definition }));
    await group('triage', {
      description: 'What the support agent may use',
      included_servers: ['files'],
      included_tools: ['notes__echo'],
      excluded_tools: ['files__fail'],
    });
    await group('lookup', { included_servers: ['search'] });
    await server.cli('disable', 'server', 'search');
  });
  test.afterAll(async () => {
    await server.stop();
  });

  test('servers: the list, one server and its tools, prompts and resources', async ({ page }) => {
    const problems = watch(page);
    await page.goto(`${server.url}/servers/`);
    await ready(page);
    await expect(page).toHaveTitle('Servers · Trunks');
    await expect(page.getByRole('navigation', { name: 'Main', exact: true }).getByRole('link', { name: 'Servers' })).toHaveAttribute('aria-current', 'page');
    await expect(main(page).getByText('3 upstream servers. 2 enabled.')).toBeVisible();
    await expect(rows(page)).toHaveCount(3);
    const files = rows(page).filter({ hasText: 'Workspace files' });
    await expect(files).toContainText('stdio');
    await expect(files).toContainText('stateless');
    await expect(files).toContainText('Enabled');
    await expect(rows(page).filter({ hasText: 'search' })).toContainText('Disabled');
    await accessible(page);

    await files.getByRole('link', { name: 'files' }).click();
    await expect(page).toHaveURL(`${server.url}/servers/detail/?name=files`);
    await ready(page);
    await expect(page.getByRole('heading', { level: 1, name: 'files' })).toBeVisible();
    await expect(page.getByRole('navigation', { name: 'Breadcrumb' }).getByRole('link', { name: 'Servers' })).toBeVisible();
    // the detail page keeps "Servers" current in the navigation
    await expect(page.getByRole('navigation', { name: 'Main', exact: true }).getByRole('link', { name: 'Servers' })).toHaveAttribute('aria-current', 'page');
    const reached = page.locator('.panel', { hasText: 'How it is reached' });
    await expect(reached).toContainText('fake-upstream');
    await expect(reached).toContainText('Variable names only');
    await expect(page.locator('.panel', { hasText: 'Summary' })).toContainText('3 of 3 enabled');
    await expect(rows(page)).toHaveCount(3);
    await expect(rows(page).first()).toContainText('files__echo');
    await accessible(page);

    await page.getByRole('link', { name: /^Prompts/ }).last().click();
    await expect(page).toHaveURL(`${server.url}/servers/detail/?name=files&tab=prompts`);
    await expect(rows(page)).toHaveCount(1);
    await expect(rows(page)).toContainText('files__greet');
    await page.getByRole('link', { name: /^Resources/ }).last().click();
    await expect(page).toHaveURL(`${server.url}/servers/detail/?name=files&tab=resources`);
    await expect(rows(page)).toContainText('trunks://');
    await accessible(page);
    expect(problems).toEqual([]);
  });

  test('tools: filters by server and status, and shows the parameters', async ({ page }) => {
    const problems = watch(page);
    await page.goto(`${server.url}/tools/`);
    await ready(page);
    await expect(page).toHaveTitle('Tools · Trunks');
    await expect(main(page).getByText('Showing 9 of 9')).toBeVisible();

    await page.getByLabel('Server').selectOption('notes');
    await expect(page).toHaveURL(`${server.url}/tools/?server=notes`);
    await expect(main(page).getByText('Showing 3 of 3')).toBeVisible();
    await main(page).getByRole('link', { name: /^Disabled/ }).click();
    await expect(page).toHaveURL(`${server.url}/tools/?server=notes&enabled=false`);
    await expect(rows(page)).toHaveCount(1);
    await expect(rows(page)).toContainText('notes__fail');
    await accessible(page);

    await page.goto(`${server.url}/tools/?server=files`);
    await ready(page);
    const toggle = page.getByRole('button', { name: 'Parameters of files__echo' });
    await expect(toggle).toHaveAttribute('aria-expanded', 'false');
    await toggle.click();
    await expect(toggle).toHaveAttribute('aria-expanded', 'true');
    const parameters = page.getByRole('table', { name: 'Parameters of files__echo' });
    await expect(parameters.locator('tbody tr')).toHaveCount(2);
    await expect(parameters).toContainText('delay_ms');
    await expect(parameters).toContainText('integer');
    await accessible(page);

    // a filter that matches nothing says so
    await page.goto(`${server.url}/tools/?server=files&enabled=false`);
    await ready(page);
    await expect(main(page).getByText('No tools match')).toBeVisible();
    expect(problems).toEqual([]);
  });

  test('prompts and resources are listed the same way', async ({ page }) => {
    const problems = watch(page);
    await page.goto(`${server.url}/tools/`);
    await ready(page);
    await main(page).getByRole('link', { name: /^Prompts/ }).click();
    await expect(page).toHaveURL(`${server.url}/prompts/`);
    await ready(page);
    await expect(page).toHaveTitle('Prompts · Trunks');
    await expect(rows(page)).toHaveCount(3);
    await page.getByRole('button', { name: 'Arguments of files__greet' }).click();
    await expect(page.getByRole('table', { name: 'Arguments of files__greet' })).toContainText('name');
    await accessible(page);

    await page.goto(`${server.url}/resources/?server=notes`);
    await ready(page);
    await expect(page).toHaveTitle('Resources · Trunks');
    await expect(rows(page)).toHaveCount(1);
    await expect(rows(page)).toContainText('trunks://res/notes/');
    await expect(rows(page)).toContainText('text/plain');
    await accessible(page);
    expect(problems).toEqual([]);
  });

  test('tool groups: the endpoint, the effective tools and why a group is empty', async ({ page, context }) => {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
    const problems = watch(page);
    await page.goto(`${server.url}/groups/`);
    await ready(page);
    await expect(page).toHaveTitle('Tool groups · Trunks');
    await expect(rows(page)).toHaveCount(2);
    await expect(rows(page).filter({ hasText: 'lookup' })).toContainText('no effective tools');
    await accessible(page);

    await rows(page).getByRole('link', { name: 'triage' }).click();
    await expect(page).toHaveURL(`${server.url}/groups/detail/?name=triage`);
    await ready(page);
    await expect(page.getByRole('heading', { level: 1, name: 'triage' })).toBeVisible();
    const endpoint = `${server.url}/v0/groups/triage/mcp`;
    await expect(page.locator('.mono-box')).toContainText(endpoint);
    await page.getByRole('button', { name: 'Copy URL' }).click();
    await expect(page.getByRole('status').filter({ hasText: 'Copied' })).toHaveCount(1);
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(endpoint);
    await expect(rows(page)).toHaveCount(3);
    await expect(rows(page).filter({ hasText: 'files__echo_headers' })).toContainText('server files');
    await expect(rows(page).filter({ hasText: 'notes__echo' })).toContainText('included tool');
    await expect(page.locator('.panel', { hasText: 'Excluded tools' }).last()).toContainText('files__fail');
    await accessible(page);

    await page.goto(`${server.url}/groups/detail/?name=lookup`);
    await ready(page);
    const warning = page.locator('.alert.warning');
    await expect(warning).toContainText('This group has no effective tools');
    await expect(warning).toContainText('search is disabled');
    await accessible(page);
    expect(problems).toEqual([]);
  });

  test('diagnostics: how the gateway is configured and what is failing', async ({ page }) => {
    const problems = watch(page);
    await page.goto(`${server.url}/diagnostics/`);
    await ready(page);
    await expect(page).toHaveTitle('Diagnostics · Trunks');
    const configuration = page.locator('.panel', { hasText: 'Configuration' }).first();
    await expect(configuration).toContainText('sqlite');
    await expect(configuration).toContainText(new URL(server.url).host);
    await expect(main(page).getByText('Every server answered')).toBeVisible();
    await expect(main(page).getByText('Nothing pending')).toBeVisible();
    await accessible(page);
    expect(problems).toEqual([]);
  });

  test('a name that does not exist says there is nothing there', async ({ page }) => {
    for (const path of ['/servers/detail/?name=nope', '/groups/detail/?name=nope', '/servers/detail/']) {
      await page.goto(server.url + path);
      await ready(page);
      await expect(main(page).getByText('Nothing here')).toBeVisible();
    }
    await accessible(page);
  });

  test('at phone width the tables stack and nothing overflows', async ({ browser }) => {
    const context = await browser.newContext({ viewport: { width: 390, height: 800 }, colorScheme: 'light' });
    const page = await context.newPage();
    for (const path of ['/servers/', '/servers/detail/?name=files', '/tools/', '/groups/detail/?name=triage', '/diagnostics/']) {
      await page.goto(server.url + path);
      await ready(page);
      await expect(page.locator('.ph h1')).toBeVisible();
      const overflow = await page.evaluate(
        () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
      );
      expect(overflow, path).toBeLessThanOrEqual(0);
      await accessible(page);
    }
    await context.close();
  });
});

test.describe('enterprise', () => {
  const RUI = 'rui-token-0123456789';
  let server: Server;
  let admin = '';

  test.beforeAll(async () => {
    server = await Server.start('enterprise');
    admin = (await server.cli('init-server')).match(/Admin access token: (\S+)/)?.[1] ?? '';
    await server.cli('register', '--name', 'files', '--command', FAKE_UPSTREAM);
    await server.cli('register', '--name', 'other', '--command', FAKE_UPSTREAM);
    await server.cli(
      'create',
      'group',
      '-c',
      await server.file('ops.json', { name: 'ops', included_servers: ['other'] }),
    );
    await server.cli('create', 'user', 'rui', '--access-token', RUI);
    await server.cli('grant', '--user', 'rui', '--server', 'files');
  });
  test.afterAll(async () => {
    await server.stop();
  });

  async function signIn(page: Page, token: string): Promise<void> {
    await page.getByLabel('Access token').fill(token);
    await page.getByRole('button', { name: 'Sign in', exact: true }).click();
  }

  test('a user is shown only what they were granted', async ({ page }) => {
    // Opening a page while signed out asks for sign-in and then comes back to it.
    await page.goto(`${server.url}/servers/`);
    await expect(page).toHaveURL(`${server.url}/login/?return_to=${encodeURIComponent('/servers/')}`);
    await signIn(page, RUI);
    await expect(page).toHaveURL(`${server.url}/servers/`);
    await ready(page);

    const navigation = page.getByRole('navigation', { name: 'Main', exact: true });
    await expect(navigation.getByRole('link', { name: 'Servers' })).toBeVisible();
    await expect(navigation.getByRole('link', { name: 'Diagnostics' })).toBeHidden();
    await expect(rows(page)).toHaveCount(1);
    await expect(rows(page)).toContainText('files');
    await accessible(page);

    // A server that exists but was not granted looks exactly like one that does not exist.
    await page.goto(`${server.url}/servers/detail/?name=other`);
    await ready(page);
    await expect(main(page).getByText('Nothing here')).toBeVisible();

    await page.goto(`${server.url}/tools/`);
    await ready(page);
    await expect(page.getByLabel('Server').locator('option')).toHaveText(['All servers', 'files']);
    await expect(main(page).getByText('Showing 3 of 3')).toBeVisible();
    // a filter for a server out of reach lists nothing rather than failing
    await page.goto(`${server.url}/tools/?server=other`);
    await ready(page);
    await expect(rows(page)).toHaveCount(0);

    await page.goto(`${server.url}/groups/`);
    await ready(page);
    await expect(main(page).getByText('You have not been granted any tool group.')).toBeVisible();
    await page.goto(`${server.url}/groups/detail/?name=ops`);
    await ready(page);
    await expect(main(page).getByText('Nothing here')).toBeVisible();

    await page.goto(`${server.url}/diagnostics/`);
    await ready(page);
    await expect(main(page).getByText('You can’t see diagnostics')).toBeVisible();
    await expect(main(page).getByText('sqlite')).toHaveCount(0);
    await accessible(page);
  });

  test('an administrator sees everything, diagnostics included', async ({ page }) => {
    await page.goto(`${server.url}/login/`);
    await signIn(page, admin);
    await ready(page);
    // from here on nothing may be refused
    const problems = watch(page);
    await page.getByRole('navigation', { name: 'Main', exact: true }).getByRole('link', { name: 'Diagnostics' }).click();
    await expect(page).toHaveURL(`${server.url}/diagnostics/`);
    await ready(page);
    await expect(main(page).getByText('Every server answered')).toBeVisible();
    await page.goto(`${server.url}/servers/`);
    await ready(page);
    await expect(rows(page)).toHaveCount(2);
    await page.goto(`${server.url}/groups/detail/?name=ops`);
    await ready(page);
    await expect(rows(page)).toHaveCount(3);
    await accessible(page);
    expect(problems).toEqual([]);
  });
});
