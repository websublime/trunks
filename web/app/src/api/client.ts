// The one way the console talks to the server. Every request is same-origin and carries the
// session cookie; an error answer becomes an `ApiError` with the server's own message.

import type { ApiErrorResponse } from './types.gen';

export class ApiError extends Error {
  /** HTTP status; 0 when the server could not be reached at all. */
  readonly status: number;
  /** Machine-readable code, when the server gave one. */
  readonly code: string | undefined;

  constructor(status: number, message: string, code?: string) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.code = code;
  }

  /** No session, or one that ended: the person has to sign in. */
  get signedOut(): boolean {
    return this.status === 401;
  }

  /** The server has not been initialized yet (`trunks init-server`). */
  get uninitialized(): boolean {
    return this.status === 403 && this.message === 'server is not initialized';
  }
}

type Method = 'GET' | 'POST' | 'PUT' | 'DELETE';

export async function request<T>(method: Method, path: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = { accept: 'application/json' };
  if (body !== undefined) headers['content-type'] = 'application/json';
  let response: Response;
  try {
    response = await fetch(path, {
      method,
      headers,
      credentials: 'same-origin',
      body: body === undefined ? null : JSON.stringify(body),
    });
  } catch {
    throw new ApiError(0, 'The server could not be reached.');
  }
  if (response.ok) {
    // Read to the end even when nothing is expected, so the request is complete before a
    // caller moves to another page.
    const text = await response.text();
    return (text === '' ? undefined : JSON.parse(text)) as T;
  }
  throw await errorOf(response);
}

async function errorOf(response: Response): Promise<ApiError> {
  const fallback = `${response.status} ${response.statusText}`.trim();
  try {
    const envelope = (await response.json()) as Partial<ApiErrorResponse>;
    if (typeof envelope.error === 'string' && envelope.error !== '') {
      return new ApiError(response.status, envelope.error, envelope.code ?? undefined);
    }
  } catch {
    // not JSON: an answer from something in front of the server
  }
  return new ApiError(response.status, fallback);
}

export const get = <T>(path: string) => request<T>('GET', path);
export const post = <T>(path: string, body?: unknown) => request<T>('POST', path, body);
export const put = <T>(path: string, body?: unknown) => request<T>('PUT', path, body);
export const del = <T>(path: string, body?: unknown) => request<T>('DELETE', path, body);
