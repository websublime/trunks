// A one-line message that survives one navigation: "Server github deleted." shown on the
// list the deletion returned to. Kept in this tab's session storage and read once; when the
// storage is unavailable the message is simply not shown.

const KEY = 'trunks.flash';

export function flash(message: string): void {
  try {
    sessionStorage.setItem(KEY, message);
  } catch {
    // no storage: the page it leads to speaks for itself
  }
}

/** The pending message, removed as it is read. */
export function takeFlash(): string | undefined {
  try {
    const message = sessionStorage.getItem(KEY);
    sessionStorage.removeItem(KEY);
    return message === null || message === '' ? undefined : message;
  } catch {
    return undefined;
  }
}
