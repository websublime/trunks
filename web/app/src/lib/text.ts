// How long names and counts are written.

/** `1 server`, `3 servers`. */
export function count(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}

/**
 * Shortens a URI or URL in the middle, keeping the scheme and host and the last segment:
 * `trunks://res/files/projec…/summary-report.md`. Lists show this; the full value is in the
 * title and is what gets copied.
 */
export function middle(text: string, max = 44): string {
  if (text.length <= max) return text;
  const tail = text.slice(text.lastIndexOf('/', text.length - 2));
  const keepTail = tail.length < max / 2 ? tail : text.slice(-Math.floor(max / 2));
  const head = text.slice(0, Math.max(8, max - keepTail.length - 1));
  return `${head}…${keepTail}`;
}

/** The value of a query parameter of the current page, or `undefined`. */
export function param(name: string): string | undefined {
  const value = new URLSearchParams(location.search).get(name);
  return value === null || value === '' ? undefined : value;
}

/** The current page's address with these query parameters set (or removed when undefined). */
export function withParams(changes: Record<string, string | undefined>): string {
  const params = new URLSearchParams(location.search);
  for (const [key, value] of Object.entries(changes)) {
    if (value === undefined) params.delete(key);
    else params.set(key, value);
  }
  const query = params.toString();
  return query === '' ? location.pathname : `${location.pathname}?${query}`;
}
