// What the "Register a server" form holds, the checks it makes before asking the server,
// and the definition it sends. The server validates again; these checks only save a round
// trip and put the message next to the field.

import type { RegisterServerInput, SessionMode } from '../api/types.gen';

export interface Pair {
  key: string;
  value: string;
}

export interface Draft {
  name: string;
  description: string;
  kind: 'local' | 'remote';
  mode: SessionMode;
  // local process
  command: string;
  args: string[];
  env: Pair[];
  // remote server
  url: string;
  token: string;
  headers: Pair[];
  clientId: string;
  clientSecret: string;
  scopes: string;
}

export type Field = 'name' | 'command' | 'env' | 'url' | 'headers';

export const emptyDraft = (): Draft => ({
  name: '',
  description: '',
  kind: 'local',
  mode: 'stateless',
  command: '',
  args: [],
  env: [],
  url: '',
  token: '',
  headers: [],
  clientId: '',
  clientSecret: '',
  scopes: '',
});

/** Where the provider sends the browser back after an upstream authorization. */
export const redirectUri = (origin: string) => `${origin}/api/dashboard/oauth/callback`;

/** The server's rule for a server name, with the reason when it is not met. */
export function nameProblem(name: string): string | undefined {
  if (name === '') return 'Enter a name.';
  if (!/^[a-zA-Z0-9_-]+$/.test(name)) return 'Use letters, digits, hyphens and underscores only.';
  if (name.includes('__')) return 'A name cannot contain “__”: that separates it from the tool name.';
  if (name.endsWith('_')) return 'A name cannot end with “_”.';
  return undefined;
}

function pairsProblem(pairs: Pair[], what: string): string | undefined {
  const used = pairs.filter((p) => p.key.trim() !== '' || p.value !== '');
  if (used.some((p) => p.key.trim() === '')) return `Give every ${what} a name.`;
  const names = used.map((p) => p.key.trim());
  const twice = names.find((n, i) => names.indexOf(n) !== i);
  return twice === undefined ? undefined : `${twice} is listed twice.`;
}

/** The fields that need attention, each with what to do about it. */
export function problems(d: Draft): Partial<Record<Field, string>> {
  const found: Partial<Record<Field, string>> = {};
  const set = (field: Field, problem: string | undefined) => {
    if (problem !== undefined) found[field] = problem;
  };
  set('name', nameProblem(d.name.trim()));
  if (d.kind === 'local') {
    set('command', d.command.trim() === '' ? 'Enter the command that starts the server.' : undefined);
    set('env', pairsProblem(d.env, 'variable'));
  } else {
    let url: URL | undefined;
    try {
      url = new URL(d.url.trim());
    } catch {
      url = undefined;
    }
    set(
      'url',
      url === undefined || (url.protocol !== 'https:' && url.protocol !== 'http:')
        ? 'Enter a full URL that starts with https:// or http://.'
        : undefined,
    );
    set('headers', pairsProblem(d.headers, 'header'));
  }
  return found;
}

const record = (pairs: Pair[]) =>
  Object.fromEntries(pairs.filter((p) => p.key.trim() !== '').map((p) => [p.key.trim(), p.value]));

/** The definition `POST /api/v0/servers` takes. Call only when `problems` found none. */
export function definition(d: Draft, origin: string): RegisterServerInput {
  const common = { name: d.name.trim(), description: d.description.trim(), session_mode: d.mode };
  if (d.kind === 'local') {
    const args = d.args.filter((a) => a !== '');
    const env = record(d.env);
    return {
      ...common,
      transport: 'stdio',
      command: d.command.trim(),
      ...(args.length > 0 ? { args } : {}),
      ...(Object.keys(env).length > 0 ? { env } : {}),
    };
  }
  const headers = record(d.headers);
  const scopes = d.scopes.split(/[\s,]+/).filter((s) => s !== '');
  const token = d.token.trim();
  return {
    ...common,
    transport: 'streamable_http',
    url: d.url.trim(),
    ...(Object.keys(headers).length > 0 ? { headers } : {}),
    // With a token the server is expected to accept it; without one, a server that asks for
    // authorization starts the provider flow, which needs to know where to come back to.
    ...(token !== ''
      ? { bearer_token: token }
      : {
          oauth_redirect_uri: redirectUri(origin),
          ...(d.clientId.trim() !== '' ? { oauth_client_id: d.clientId.trim() } : {}),
          ...(d.clientSecret !== '' ? { oauth_client_secret: d.clientSecret } : {}),
          ...(scopes.length > 0 ? { oauth_scopes: scopes } : {}),
        }),
  };
}

/** Only http(s) addresses are ever put in a link. */
export function safeProviderUrl(url: string): string | undefined {
  try {
    const parsed = new URL(url);
    return parsed.protocol === 'https:' || parsed.protocol === 'http:' ? parsed.href : undefined;
  } catch {
    return undefined;
  }
}
