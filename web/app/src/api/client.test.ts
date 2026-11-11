import { afterEach, describe, expect, it, vi } from 'vitest';

import { ApiError, get, post } from './client';

function answer(status: number, body?: unknown, statusText = ''): Response {
  const text = body === undefined ? null : typeof body === 'string' ? body : JSON.stringify(body);
  return new Response(text, { status, statusText });
}

function serverSays(response: Response) {
  const fetchMock = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) => response);
  vi.stubGlobal('fetch', fetchMock);
  return fetchMock;
}

afterEach(() => vi.unstubAllGlobals());

describe('request', () => {
  it('sends same-origin JSON and returns the parsed body', async () => {
    const fetchMock = serverSays(answer(200, { version: 'v1.2.3' }));
    await expect(post('/auth/token', { access_token: 't' })).resolves.toEqual({ version: 'v1.2.3' });
    const [path, init] = fetchMock.mock.calls[0]!;
    expect(path).toBe('/auth/token');
    expect(init).toMatchObject({
      method: 'POST',
      credentials: 'same-origin',
      body: '{"access_token":"t"}',
      headers: { accept: 'application/json', 'content-type': 'application/json' },
    });
  });

  it('sends no body and no content type on a read', async () => {
    const fetchMock = serverSays(answer(200, []));
    await get('/api/dashboard/servers');
    const init = fetchMock.mock.calls[0]![1]!;
    expect(init.body).toBeNull();
    expect(init.headers).toEqual({ accept: 'application/json' });
  });

  it('treats 204 and an empty 200 as no value', async () => {
    serverSays(answer(204));
    await expect(post('/auth/logout')).resolves.toBeUndefined();
    serverSays(answer(200, ''));
    await expect(get('/x')).resolves.toBeUndefined();
  });

  const errors: [string, Response, Partial<ApiError> & { signedOut: boolean; uninitialized: boolean }][] = [
    [
      'the server’s own message and code',
      answer(400, { error: 'server name is invalid', code: 'invalid_name' }),
      { status: 400, message: 'server name is invalid', code: 'invalid_name', signedOut: false, uninitialized: false },
    ],
    [
      'a missing or ended session',
      answer(401, { error: 'missing access token' }),
      { status: 401, message: 'missing access token', signedOut: true, uninitialized: false },
    ],
    [
      'a server that is not initialized',
      answer(403, { error: 'server is not initialized' }),
      { status: 403, message: 'server is not initialized', signedOut: false, uninitialized: true },
    ],
    [
      'any other refusal is not "uninitialized"',
      answer(403, { error: 'admin access required' }),
      { status: 403, message: 'admin access required', signedOut: false, uninitialized: false },
    ],
    [
      'an answer that is not JSON (a proxy in front of the server)',
      answer(502, '<html>Bad Gateway</html>', 'Bad Gateway'),
      { status: 502, message: '502 Bad Gateway', signedOut: false, uninitialized: false },
    ],
    [
      'JSON without an error message',
      answer(500, {}, 'Internal Server Error'),
      { status: 500, message: '500 Internal Server Error', signedOut: false, uninitialized: false },
    ],
  ];

  it.each(errors)('turns %s into an ApiError', async (_name, response, expected) => {
    serverSays(response);
    const error = await get('/x').then(
      () => undefined,
      (e: unknown) => e,
    );
    expect(error).toBeInstanceOf(ApiError);
    const e = error as ApiError;
    expect({
      status: e.status,
      message: e.message,
      code: e.code,
      signedOut: e.signedOut,
      uninitialized: e.uninitialized,
    }).toEqual({ code: undefined, ...expected });
  });

  it('reports an unreachable server as status 0', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        throw new TypeError('fetch failed');
      }),
    );
    await expect(get('/x')).rejects.toMatchObject({ status: 0, message: 'The server could not be reached.' });
  });
});
