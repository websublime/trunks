// <trunks-shell> turns the static shell into this viewer's console: it asks the server who
// is here, sends them to sign in or to the "not initialized" page when that is the answer,
// and shows only the navigation they may use. It never decides access: it reflects what
// the API says.

import { ApiError, post } from '../api/client';
import { modeLabel } from '../lib/format';
import { visibleTo, type Audience } from '../lib/nav';
import { hasNoAccess, loadSession, signInUrl, type Session } from '../lib/session';

let current: Promise<Session> | undefined;

/**
 * The viewer's session. Resolves once per page; never resolves when the page is being left
 * for the sign-in or the "not initialized" page.
 */
export function session(): Promise<Session> {
  current ??= (async () => {
    const state = await loadSession();
    if (state.kind === 'ready') return state.session;
    const here = location.pathname + location.search;
    location.replace(state.kind === 'uninitialized' ? '/not-initialized/' : signInUrl(here));
    return new Promise<Session>(() => {});
  })();
  return current;
}

class TrunksShell extends HTMLElement {
  connectedCallback(): void {
    session().then(
      (s) => this.#show(s),
      (e: unknown) => this.#fail(e),
    );
  }

  #show(s: Session): void {
    const enterprise = s.mode === 'enterprise';
    const nothing = hasNoAccess(s);
    for (const item of this.querySelectorAll<HTMLElement>('[data-audience]')) {
      const audience = item.dataset['audience'] as Audience;
      const visible = visibleTo(audience, { enterprise, admin: s.admin });
      // Someone with no grants has only the overview, which explains why.
      item.hidden = !visible || (nothing && item.dataset['href'] !== '/');
    }
    for (const section of this.querySelectorAll<HTMLElement>('[data-section]')) {
      section.hidden = section.querySelector('[data-audience]:not([hidden])') === null;
    }
    this.#text('[data-mode-label]', modeLabel(s.mode));
    this.#text('[data-version]', s.version);
    const note = this.querySelector<HTMLElement>('[data-mode-note]');
    if (note) note.hidden = enterprise;
    const user = this.querySelector<HTMLElement>('[data-user]');
    if (user && s.user) {
      user.hidden = false;
      this.#text('[data-username]', s.user.username);
      this.#text('[data-role]', s.user.role);
      user.querySelector('[data-sign-out]')?.addEventListener('click', () => void signOut());
    }
    document.documentElement.dataset['state'] = 'ready';
  }

  #fail(e: unknown): void {
    const message = e instanceof ApiError && e.status !== 0 ? e.message : undefined;
    if (message) {
      const target = document.querySelector('[data-boot-error]');
      if (target) target.textContent = message;
    }
    document.documentElement.dataset['state'] = 'failed';
  }

  #text(selector: string, text: string): void {
    for (const el of this.querySelectorAll(selector)) el.textContent = text;
  }
}

async function signOut(): Promise<void> {
  try {
    await post('/auth/logout');
  } finally {
    location.assign('/login/');
  }
}

customElements.define('trunks-shell', TrunksShell);
