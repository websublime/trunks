// Managing the gateway from the console: registering servers (a local process, a remote
// server, one that asks for authorization), switching things on and off, deleting, and tool
// groups. Everything is checked against the real server.

import { expect, test, type Page } from '@playwright/test';

import { accessible, ready, watch } from './console';
import { FAKE_UPSTREAM, Server, startUpstream } from './server';

const rows = (page: Page) => page.locator('table.tbl > tbody > tr');
const main = (page: Page) => page.getByRole('main');
const submit = (page: Page, name: string) => page.getByRole('button', { name, exact: true }).click();

test.describe('development', () => {
  test.describe.configure({ mode: 'serial' });

  let server: Server;
  let open: Awaited<ReturnType<typeof startUpstream>>;
  let guarded: Awaited<ReturnType<typeof startUpstream>>;

  test.beforeAll(async () => {
    [server, open, guarded] = await Promise.all([
      Server.start('development'),
      startUpstream('open'),
      startUpstream('oauth'),
    ]);
  });
  test.afterAll(async () => {
    open.stop();
    guarded.stop();
    await server.stop();
  });

  test('a local process is registered from the form, and its secrets stay out of the page', async ({ page }) => {
    const problems = watch(page);
    await page.goto(`${server.url}/servers/`);
    await ready(page);
    await expect(main(page).getByText('No servers yet')).toBeVisible();
    await main(page).getByRole('link', { name: 'Register server' }).first().click();
    await expect(page).toHaveURL(`${server.url}/servers/new/`);
    await ready(page);
    await expect(page).toHaveTitle('Register a server · Trunks');
    await accessible(page);

    // Nothing filled in: the form says what is missing, next to each field.
    await submit(page, 'Register server');
    await expect(main(page).getByText('2 fields need attention.')).toBeVisible();
    await expect(page.getByLabel('Name (required)')).toBeFocused();
    await expect(page.getByLabel('Name (required)')).toHaveAttribute('aria-invalid', 'true');
    await expect(main(page).getByText('Enter a name.')).toBeVisible();
    await expect(main(page).getByText('Enter the command that starts the server.')).toBeVisible();
    await accessible(page);

    await page.getByLabel('Name (required)').fill('files');
    await page.getByLabel('Description').fill('Workspace files');
    await page.getByLabel('Command (required)').fill(FAKE_UPSTREAM);
    await page.getByRole('button', { name: 'Add argument' }).click();
    await expect(page.getByLabel('Argument 1', { exact: true })).toBeFocused();
    await page.getByLabel('Argument 1', { exact: true }).fill('--verbose');
    await page.getByRole('button', { name: 'Add variable' }).click();
    await page.getByLabel('Name of variable 1').fill('API_KEY');
    const secret = page.getByLabel('Value of variable 1');
    await secret.fill('s3cret-value');
    await expect(secret).toHaveAttribute('type', 'password');
    await page.getByRole('button', { name: 'Show value' }).click();
    await expect(secret).toHaveAttribute('type', 'text');
    await page.getByLabel('Session mode').selectOption('stateful');
    await accessible(page);

    await submit(page, 'Register server');
    await expect(page).toHaveURL(`${server.url}/servers/detail/?name=files`);
    await ready(page);
    await expect(page.getByRole('status').filter({ hasText: 'Server files registered.' })).toBeVisible();
    await expect(rows(page)).toHaveCount(3);
    const reached = page.locator('.panel', { hasText: 'How it is reached' });
    await expect(reached).toContainText('--verbose');
    await expect(reached).toContainText('API_KEY');
    await expect(page.locator('.panel', { hasText: 'Summary' })).toContainText('stateful');
    // the value went to the server and is not sent back
    await expect(page.locator('body')).not.toContainText('s3cret-value');
    await accessible(page);
    // the message is shown once: not again after a reload
    await page.reload();
    await ready(page);
    await expect(page.getByText('Server files registered.')).toHaveCount(0);
    expect(problems).toEqual([]);
  });

  test('a taken name, a bad URL and an unreachable server are explained where they happen', async ({ page }) => {
    await page.goto(`${server.url}/servers/new/`);
    await ready(page);
    await page.getByLabel('Name (required)').fill('files');
    await page.getByLabel('Command (required)').fill(FAKE_UPSTREAM);
    await submit(page, 'Register server');
    await expect(main(page).getByText('A server of this name is already registered.')).toBeVisible();
    await expect(page.getByLabel('Name (required)')).toBeFocused();

    await page.getByLabel('Name (required)').fill('bad__name');
    await page.getByRole('button', { name: 'Remote server' }).click();
    await expect(page.getByRole('button', { name: 'Remote server' })).toHaveAttribute('aria-pressed', 'true');
    await page.getByLabel('URL (required)').fill('mcp.example.com/mcp');
    await submit(page, 'Register server');
    await expect(main(page).getByText('2 fields need attention.')).toBeVisible();
    await expect(main(page).getByText('A name cannot contain “__”')).toBeVisible();
    await expect(main(page).getByText('Enter a full URL that starts with https:// or http://.')).toBeVisible();
    await accessible(page);

    await page.getByLabel('Name (required)').fill('dead');
    await page.getByLabel('URL (required)').fill('http://127.0.0.1:9/mcp');
    await submit(page, 'Register server');
    const failure = page.getByRole('alert').filter({ hasText: 'Couldn’t register dead' });
    await expect(failure).toBeVisible();
    await expect(failure).toContainText('Nothing was stored.');
    await expect(failure).toBeFocused();
    // what was typed is still there
    await expect(page.getByLabel('URL (required)')).toHaveValue('http://127.0.0.1:9/mcp');
    await accessible(page);
    await page.goto(`${server.url}/servers/`);
    await ready(page);
    await expect(rows(page)).toHaveCount(1);
  });

  test('a remote server is registered by its address', async ({ page }) => {
    const problems = watch(page);
    await page.goto(`${server.url}/servers/new/`);
    await ready(page);
    await page.getByLabel('Name (required)').fill('docs');
    await page.getByRole('button', { name: 'Remote server' }).click();
    await page.getByLabel('URL (required)').fill(open.url);
    await page.getByRole('button', { name: 'Add header' }).click();
    await page.getByLabel('Name of header 1').fill('X-Site');
    await page.getByLabel('Value of header 1').fill('acme-internal');
    await accessible(page);
    await submit(page, 'Register server');
    await expect(page).toHaveURL(`${server.url}/servers/detail/?name=docs`);
    await ready(page);
    const reached = page.locator('.panel', { hasText: 'How it is reached' });
    await expect(reached).toContainText(open.url);
    await expect(reached).toContainText('X-Site');
    await expect(page.locator('body')).not.toContainText('acme-internal');
    await expect(main(page).getByText('streamable_http').first()).toBeVisible();
    expect(problems).toEqual([]);
  });

  test('a server that asks for authorization is registered once the provider approves', async ({ page, context }) => {
    const problems = watch(page);
    await page.goto(`${server.url}/servers/new/`);
    await ready(page);
    await page.getByLabel('Name (required)').fill('guarded');
    await page.getByRole('button', { name: 'Remote server' }).click();
    await page.getByLabel('URL (required)').fill(guarded.url);
    // The redirect URI to give a provider that needs one registered by hand.
    await page.getByText('Authorization with your own OAuth client').click();
    await expect(main(page).getByText(`${server.url}/api/dashboard/oauth/callback`)).toBeVisible();
    await submit(page, 'Register server');

    const heading = page.getByRole('heading', { name: 'Authorization required' });
    await expect(heading).toBeVisible();
    await expect(heading).toBeFocused();
    await expect(main(page).getByText('until then nothing is stored')).toBeVisible();
    await accessible(page);

    // Backing out returns to the form with everything still filled in; nothing was registered.
    await page.getByRole('button', { name: 'Cancel' }).click();
    await expect(page.getByLabel('Name (required)')).toHaveValue('guarded');
    await expect(page.getByLabel('URL (required)')).toHaveValue(guarded.url);
    await submit(page, 'Register server');
    await expect(heading).toBeVisible();

    const provider = context.waitForEvent('page');
    await page.getByRole('link', { name: 'Open provider' }).click();
    await expect(main(page).getByText('Waiting for authorization…')).toBeVisible();
    await accessible(page);
    // The fake provider approves at once and sends the browser back to Trunks.
    const popup = await provider;
    await expect(popup.getByRole('heading', { name: 'Authorization received' })).toBeVisible();
    await popup.close();

    const done = page.getByRole('status').filter({ hasText: 'guarded is authorized and registered' });
    await expect(done).toBeVisible({ timeout: 15_000 });
    await expect(done).toBeFocused();
    await expect(done).toContainText('Trunks found 1 tool, 0 prompts and 0 resources.');
    await accessible(page);
    await page.getByRole('link', { name: 'View server' }).click();
    await expect(page).toHaveURL(`${server.url}/servers/detail/?name=guarded`);
    await ready(page);
    await expect(main(page).getByText('Authorized').first()).toBeVisible();
    await expect(rows(page)).toContainText('guarded__search');
    expect(problems).toEqual([]);
  });

  test('switches act at once; deleting asks first', async ({ page }) => {
    const problems = watch(page);
    await page.goto(`${server.url}/servers/detail/?name=files`);
    await ready(page);
    const tool = page.getByRole('switch', { name: 'Enabled: files__fail' });
    await expect(tool).toHaveAttribute('aria-checked', 'true');
    await tool.click();
    await expect(tool).toHaveAttribute('aria-checked', 'false');
    await expect(page.locator('.panel', { hasText: 'Summary' })).toContainText('2 of 3 enabled');
    expect(await server.cli('list', 'tools')).toMatch(/files__fail.*\n?.*disabled/i);

    // the same switch in the catalogue
    await page.goto(`${server.url}/tools/?server=files`);
    await ready(page);
    const again = page.getByRole('switch', { name: 'Enabled: files__fail' });
    await expect(again).toHaveAttribute('aria-checked', 'false');
    await again.click();
    await expect(again).toHaveAttribute('aria-checked', 'true');
    await accessible(page);

    // a prompt and a resource
    await page.goto(`${server.url}/servers/detail/?name=files&tab=prompts`);
    await ready(page);
    const prompt = page.getByRole('switch', { name: 'Enabled: files__greet' });
    await prompt.click();
    await expect(prompt).toHaveAttribute('aria-checked', 'false');
    await page.goto(`${server.url}/servers/detail/?name=files&tab=resources`);
    await ready(page);
    const resource = page.getByRole('switch', { name: /^Enabled: trunks:\/\/res\/files\// });
    await resource.click();
    await expect(resource).toHaveAttribute('aria-checked', 'false');

    // the whole server
    const whole = page.getByRole('switch', { name: 'Enabled: files', exact: true });
    await whole.click();
    await expect(whole).toHaveAttribute('aria-checked', 'false');
    await expect(main(page).getByText('Disabled').first()).toBeVisible();
    await whole.click();
    await expect(whole).toHaveAttribute('aria-checked', 'true');
    await accessible(page);

    // Delete: a dialog, with the safe button focused; Esc backs out.
    await page.getByRole('button', { name: 'Delete' }).click();
    const dialog = page.getByRole('alertdialog', { name: 'Delete files?' });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByRole('button', { name: 'Cancel' })).toBeFocused();
    await expect(dialog).toContainText('3 tools, 1 prompt and 1 resource');
    await accessible(page);
    await page.keyboard.press('Escape');
    await expect(dialog).toBeHidden();
    await expect(page.getByRole('heading', { level: 1, name: 'files' })).toBeVisible();

    await page.getByRole('button', { name: 'Delete' }).click();
    await dialog.getByRole('button', { name: 'Delete server' }).click();
    await expect(page).toHaveURL(`${server.url}/servers/`);
    await ready(page);
    await expect(page.getByRole('status').filter({ hasText: 'Server files deleted.' })).toBeVisible();
    await expect(rows(page).filter({ hasText: 'files' })).toHaveCount(0);
    await expect(rows(page)).toHaveCount(2);
    await accessible(page);
    expect(problems).toEqual([]);
  });

  test('tool groups are created, changed and deleted', async ({ page }) => {
    const problems = watch(page);
    await page.goto(`${server.url}/groups/`);
    await ready(page);
    await main(page).getByRole('link', { name: 'New group' }).first().click();
    await expect(page).toHaveURL(`${server.url}/groups/new/`);
    await ready(page);
    await expect(page).toHaveTitle('New tool group · Trunks');
    await expect(page.getByLabel('Filter tools')).toBeVisible();
    await accessible(page);

    await submit(page, 'Create group');
    await expect(main(page).getByText('Enter a name.')).toBeVisible();
    await page.getByLabel('Name (required)').fill('triage');
    await page.getByLabel('Description').fill('What the support agent may use');
    await submit(page, 'Create group');
    await expect(main(page).getByText('Select at least one enabled tool')).toBeVisible();
    await accessible(page);

    // the whole of one server, minus one tool; and one tool of another
    await page.getByLabel('Whole server docs').check();
    await expect(main(page).getByText('3 effective tools')).toBeVisible();
    await page.getByLabel('docs__fail').uncheck();
    await page.getByLabel('guarded__search').check();
    await expect(main(page).getByText('3 effective tools')).toBeVisible();
    await page.getByLabel('Filter tools').fill('guard');
    await expect(page.getByLabel('docs__echo', { exact: true })).toHaveCount(0);
    await expect(page.getByLabel('guarded__search')).toBeChecked();
    await page.getByLabel('Filter tools').fill('');
    await accessible(page);

    await submit(page, 'Create group');
    await expect(page).toHaveURL(`${server.url}/groups/detail/?name=triage`);
    await ready(page);
    await expect(page.getByRole('status').filter({ hasText: 'Tool group triage created.' })).toBeVisible();
    await expect(rows(page)).toHaveCount(3);
    await expect(page.locator('.panel', { hasText: 'Excluded tools' }).last()).toContainText('docs__fail');
    await expect(page.locator('.panel', { hasText: 'Included tools' }).last()).toContainText('guarded__search');

    await main(page).getByRole('link', { name: 'Edit' }).click();
    await expect(page).toHaveURL(`${server.url}/groups/edit/?name=triage`);
    await ready(page);
    await expect(page.getByLabel('Description')).toHaveValue('What the support agent may use');
    await expect(page.getByLabel('Whole server docs')).toBeChecked();
    await expect(page.getByLabel('docs__fail')).not.toBeChecked();
    await expect(page.getByLabel('Name (required)')).toHaveCount(0);
    await accessible(page);
    await page.getByLabel('Whole server docs').uncheck();
    await page.getByLabel('docs__echo', { exact: true }).check();
    await expect(main(page).getByText('2 effective tools')).toBeVisible();
    await submit(page, 'Save changes');
    await expect(page).toHaveURL(`${server.url}/groups/detail/?name=triage`);
    await ready(page);
    await expect(page.getByRole('status').filter({ hasText: 'Tool group triage saved.' })).toBeVisible();
    await expect(rows(page)).toHaveCount(2);
    expect(await server.cli('get', 'group', 'triage')).toContain('docs__echo');

    await page.getByRole('button', { name: 'Delete' }).click();
    const dialog = page.getByRole('alertdialog', { name: 'Delete triage?' });
    await expect(dialog).toContainText('/v0/groups/triage/mcp');
    await dialog.getByRole('button', { name: 'Delete group' }).click();
    await expect(page).toHaveURL(`${server.url}/groups/`);
    await ready(page);
    await expect(page.getByRole('status').filter({ hasText: 'Tool group triage deleted.' })).toBeVisible();
    await expect(main(page).getByText('No tool groups yet')).toBeVisible();
    expect(problems).toEqual([]);
  });

  test('the forms work at phone width', async ({ browser }) => {
    const context = await browser.newContext({ viewport: { width: 390, height: 800 }, colorScheme: 'light' });
    const page = await context.newPage();
    for (const path of ['/servers/new/', '/groups/new/', '/servers/detail/?name=docs']) {
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
    await server.cli('create', 'user', 'rui', '--access-token', RUI);
    await server.cli('grant', '--user', 'rui', '--server', 'files');
  });
  test.afterAll(async () => {
    await server.stop();
  });

  async function signIn(page: Page, token: string): Promise<void> {
    await page.goto(`${server.url}/login/`);
    await page.getByLabel('Access token').fill(token);
    await page.getByRole('button', { name: 'Sign in', exact: true }).click();
    // the sign-in page is "ready" too: wait until it has moved on
    await expect(page).toHaveURL(`${server.url}/`);
    await ready(page);
  }

  test('a user gets no controls, and the server refuses the changes anyway', async ({ page }) => {
    await signIn(page, RUI);
    await page.goto(`${server.url}/servers/`);
    await ready(page);
    await expect(rows(page)).toHaveCount(1);
    await expect(main(page).getByRole('link', { name: 'Register server' })).toHaveCount(0);

    await page.goto(`${server.url}/servers/detail/?name=files`);
    await ready(page);
    await expect(rows(page)).toHaveCount(3);
    await expect(page.getByRole('switch')).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Delete' })).toHaveCount(0);
    await page.goto(`${server.url}/tools/`);
    await ready(page);
    await expect(rows(page)).toHaveCount(3);
    await expect(page.getByRole('switch')).toHaveCount(0);

    await page.goto(`${server.url}/servers/new/`);
    await ready(page);
    await expect(main(page).getByText('You can’t register servers')).toBeVisible();
    await expect(page.getByLabel('Name (required)')).toHaveCount(0);
    await accessible(page);
    await page.goto(`${server.url}/groups/new/`);
    await ready(page);
    await expect(main(page).getByText('You can’t change tool groups')).toBeVisible();

    // Hiding the controls is a courtesy; the refusal is the server's.
    const origin = { origin: server.url };
    expect((await page.request.post(`${server.url}/api/v0/servers/files/disable`, { headers: origin })).status()).toBe(403);
    expect((await page.request.delete(`${server.url}/api/v0/servers/files`, { headers: origin })).status()).toBe(403);
    expect(
      (
        await page.request.post(`${server.url}/api/v0/tool-groups`, {
          headers: origin,
          data: { name: 'mine', included_servers: ['files'] },
        })
      ).status(),
    ).toBe(403);
  });

  test('an administrator manages with a browser session', async ({ page }) => {
    await signIn(page, admin);
    const problems = watch(page);
    await page.goto(`${server.url}/servers/detail/?name=files`);
    await ready(page);
    const tool = page.getByRole('switch', { name: 'Enabled: files__fail' });
    await tool.click();
    await expect(tool).toHaveAttribute('aria-checked', 'false');
    await page.goto(`${server.url}/servers/`);
    await ready(page);
    await expect(main(page).getByRole('link', { name: 'Register server' })).toBeVisible();
    await accessible(page);
    expect(problems).toEqual([]);
  });
});
