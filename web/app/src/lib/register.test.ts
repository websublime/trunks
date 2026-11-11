import { describe, expect, it } from 'vitest';

import { definition, emptyDraft, nameProblem, problems, redirectUri, safeProviderUrl, type Draft } from './register';

const draft = (changes: Partial<Draft>): Draft => ({ ...emptyDraft(), ...changes });
const ORIGIN = 'https://trunks.example.com';

describe('nameProblem', () => {
  it.each([
    ['github', undefined],
    ['git-hub_2', undefined],
    ['', 'Enter a name.'],
    ['git hub', 'Use letters, digits, hyphens and underscores only.'],
    ['git.hub', 'Use letters, digits, hyphens and underscores only.'],
    ['git__hub', 'A name cannot contain “__”: that separates it from the tool name.'],
    ['github_', 'A name cannot end with “_”.'],
  ])('%j → %j', (name, expected) => {
    expect(nameProblem(name)).toBe(expected);
  });
});

describe('problems', () => {
  it('finds none in a complete local process', () => {
    expect(problems(draft({ name: 'files', command: 'npx', args: ['-y', 'pkg'], env: [{ key: 'A', value: '1' }] }))).toEqual({});
  });

  it('asks for the command of a local process and ignores the remote fields', () => {
    expect(problems(draft({ name: 'files', url: 'nonsense' }))).toEqual({
      command: 'Enter the command that starts the server.',
    });
  });

  it.each([
    ['', false],
    ['mcp.example.com/mcp', false],
    ['ftp://mcp.example.com', false],
    ['javascript:alert(1)', false],
    ['http://127.0.0.1:9000/mcp', true],
    [' https://mcp.example.com/mcp ', true],
  ])('remote URL %j is accepted: %s', (url, accepted) => {
    const found = problems(draft({ name: 'docs', kind: 'remote', url }));
    expect(found.url === undefined).toBe(accepted);
    expect(found.command).toBeUndefined();
  });

  it('wants a name for every variable and header, once', () => {
    expect(problems(draft({ name: 'a', command: 'x', env: [{ key: ' ', value: 'secret' }] })).env).toBe(
      'Give every variable a name.',
    );
    expect(
      problems(draft({ name: 'a', command: 'x', env: [{ key: 'A', value: '1' }, { key: 'A', value: '2' }] })).env,
    ).toBe('A is listed twice.');
    expect(
      problems(draft({ name: 'a', kind: 'remote', url: 'https://x.test', headers: [{ key: '', value: 'v' }] })).headers,
    ).toBe('Give every header a name.');
    // a row left completely empty is not a problem
    expect(problems(draft({ name: 'a', command: 'x', env: [{ key: '', value: '' }] }))).toEqual({});
  });
});

describe('definition', () => {
  it('describes a local process, leaving out what is empty', () => {
    expect(
      definition(
        draft({
          name: ' files ',
          description: ' Shared files ',
          command: ' npx ',
          args: ['-y', '', '@scope/pkg'],
          env: [{ key: 'TOKEN', value: ' s3cret ' }, { key: '', value: '' }],
          mode: 'stateful',
          // remote fields typed before switching kind are not sent
          url: 'https://left.over',
          token: 'left-over',
        }),
        ORIGIN,
      ),
    ).toEqual({
      name: 'files',
      description: 'Shared files',
      session_mode: 'stateful',
      transport: 'stdio',
      command: 'npx',
      args: ['-y', '@scope/pkg'],
      env: { TOKEN: ' s3cret ' },
    });
    expect(definition(draft({ name: 'x', command: 'run' }), ORIGIN)).toEqual({
      name: 'x',
      description: '',
      session_mode: 'stateless',
      transport: 'stdio',
      command: 'run',
    });
  });

  it('sends a bearer token as it is, and no authorization fields with it', () => {
    expect(
      definition(
        draft({ name: 'docs', kind: 'remote', url: 'https://mcp.example.com/mcp', token: ' tok ', clientId: 'ignored' }),
        ORIGIN,
      ),
    ).toEqual({
      name: 'docs',
      description: '',
      session_mode: 'stateless',
      transport: 'streamable_http',
      url: 'https://mcp.example.com/mcp',
      bearer_token: 'tok',
    });
  });

  it('without a token, says where an authorization comes back to', () => {
    expect(
      definition(
        draft({
          name: 'jira',
          kind: 'remote',
          url: 'https://mcp.example.com/mcp',
          headers: [{ key: 'X-Site', value: 'acme' }],
          clientId: ' client ',
          clientSecret: 'shh',
          scopes: 'read, write  admin',
        }),
        ORIGIN,
      ),
    ).toEqual({
      name: 'jira',
      description: '',
      session_mode: 'stateless',
      transport: 'streamable_http',
      url: 'https://mcp.example.com/mcp',
      headers: { 'X-Site': 'acme' },
      oauth_redirect_uri: 'https://trunks.example.com/api/dashboard/oauth/callback',
      oauth_client_id: 'client',
      oauth_client_secret: 'shh',
      oauth_scopes: ['read', 'write', 'admin'],
    });
    expect(redirectUri(ORIGIN)).toBe('https://trunks.example.com/api/dashboard/oauth/callback');
  });
});

describe('safeProviderUrl', () => {
  it.each([
    ['https://idp.example.com/authorize?x=1', 'https://idp.example.com/authorize?x=1'],
    ['http://127.0.0.1:9000/authorize', 'http://127.0.0.1:9000/authorize'],
    ['javascript:alert(1)', undefined],
    ['data:text/html,x', undefined],
    ['not a url', undefined],
  ])('%j → %j', (url, expected) => {
    expect(safeProviderUrl(url)).toBe(expected);
  });
});
