// <trunks-sign-in>: the enterprise sign-in. An access token is exchanged for a session
// cookie and then forgotten: it is never stored in the browser.

import '@websublime/line-components/button';
import { LitElement, html, nothing, type TemplateResult } from 'lit';

import { ApiError, get, post } from '../api/client';
import type { AuthConfig, ServerMetadata } from '../api/types.gen';
import { loadSession, safeReturnTo } from '../lib/session';
import { icon } from './icon';

class TrunksSignIn extends LitElement {
  static override properties = {
    ready: { state: true },
    oidc: { state: true },
    version: { state: true },
    shown: { state: true },
    busy: { state: true },
    invalid: { state: true },
  };

  declare ready: boolean;
  declare oidc: boolean;
  declare version: string;
  /** The token is displayed as text. */
  declare shown: boolean;
  declare busy: boolean;
  /** Message under the field after a refused sign-in. */
  declare invalid: string | undefined;

  constructor() {
    super();
    this.ready = false;
    this.oidc = false;
    this.version = '';
    this.shown = false;
    this.busy = false;
  }

  protected override createRenderRoot(): HTMLElement {
    return this;
  }

  get #returnTo(): string {
    return safeReturnTo(new URLSearchParams(location.search).get('return_to'));
  }

  override connectedCallback(): void {
    super.connectedCallback();
    void this.#start();
  }

  async #start(): Promise<void> {
    try {
      const state = await loadSession();
      if (state.kind === 'uninitialized') return location.replace('/not-initialized/');
      // Already signed in, or a server that needs no sign-in: nothing to do here.
      if (state.kind === 'ready') return location.replace(this.#returnTo);
      this.oidc = state.oidc;
      this.version = state.version;
    } catch {
      // The form still works if the server comes back; show it with what is known.
      const [config, meta] = await Promise.allSettled([
        get<AuthConfig>('/auth/config'),
        get<ServerMetadata>('/metadata'),
      ]);
      this.oidc = config.status === 'fulfilled' && config.value.oidc;
      this.version = meta.status === 'fulfilled' ? meta.value.version : '';
    }
    this.ready = true;
    document.documentElement.dataset['state'] = 'ready';
    await this.updateComplete;
    this.querySelector<HTMLInputElement>('#token')?.focus();
  }

  async #submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const field = this.querySelector<HTMLInputElement>('#token');
    if (field === null || this.busy) return;
    const token = field.value.trim();
    if (token === '') {
      this.invalid = 'Enter your access token.';
      field.focus();
      return;
    }
    this.busy = true;
    try {
      await post('/auth/token', { access_token: token });
      field.value = '';
      location.replace(this.#returnTo);
    } catch (e) {
      // The same message for an unknown and a revoked token: the server tells them apart no
      // more than this page does.
      this.invalid =
        e instanceof ApiError && e.status === 401
          ? 'That token is not valid. Check it and try again.'
          : e instanceof Error
            ? e.message
            : 'Sign-in failed.';
      field.value = '';
      this.busy = false;
      await this.updateComplete;
      field.focus();
    }
  }

  protected override render(): TemplateResult | typeof nothing {
    if (!this.ready) return nothing;
    const describedBy = this.invalid === undefined ? 'token-hint' : 'token-error token-hint';
    const sso = `/auth/login?return_to=${encodeURIComponent(this.#returnTo)}`;
    return html`
      <div class="panel">
        <div class="col head">
          <div class="row">
            ${logo()}<strong>Trunks</strong><span class="bdg accent">Enterprise</span>
          </div>
          <h1>Sign in</h1>
        </div>
        <form class="form close" novalidate @submit=${(e: SubmitEvent) => void this.#submit(e)}>
          <div class="fld">
            <label for="token">Access token</label>
            <div class="inp mono ${this.invalid === undefined ? '' : 'invalid'}">
              <input
                id="token"
                name="token"
                type=${this.shown ? 'text' : 'password'}
                autocomplete="off"
                spellcheck="false"
                aria-invalid=${this.invalid === undefined ? 'false' : 'true'}
                aria-describedby=${describedBy}
              />
              <span class="sfx">
                <button
                  type="button"
                  class="btn ibtn ghost"
                  aria-label="Show token"
                  aria-pressed=${this.shown ? 'true' : 'false'}
                  title="Show token"
                  @click=${() => (this.shown = !this.shown)}
                >
                  ${icon('eye')}
                </button>
              </span>
            </div>
            ${this.invalid === undefined
              ? nothing
              : html`<div class="ferr" id="token-error" role="alert">
                  ${icon('alert')}${this.invalid}
                </div>`}
            <div class="hint" id="token-hint">Paste the token an administrator gave you.</div>
          </div>
          <line-button class="wide" variant="primary" type="submit" ?loading=${this.busy}
            >Sign in</line-button
          >
        </form>
        ${this.oidc
          ? html`<div class="row or"><hr class="sep" /><span>or</span><hr class="sep" /></div>
              <a class="btn wide" href=${sso}>${icon('external')}Sign in with SSO</a>`
          : nothing}
      </div>
      <p class="ver">Trunks ${this.version}</p>
    `;
  }
}

function logo(): TemplateResult {
  return html`<svg class="logo" width="24" height="24" viewBox="0 0 24 24" aria-hidden="true">
    <defs>
      <linearGradient id="trunks-mark" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" stop-color="#12a594"></stop>
        <stop offset="1" stop-color="#6e56cf"></stop>
      </linearGradient>
    </defs>
    <path
      d="M12 2.5 4 5.5v6c0 4.6 3.4 8.4 8 9.9 4.6-1.5 8-5.3 8-9.9v-6z"
      fill="none"
      stroke="url(#trunks-mark)"
      stroke-width="1.75"
      stroke-linejoin="round"
    ></path>
    <path
      d="M12 17.5v-9M12 12l-3-2.5M12 10.5l3-2.5M12 14l2.5-2"
      fill="none"
      stroke="url(#trunks-mark)"
      stroke-width="1.75"
      stroke-linecap="round"
    ></path>
  </svg>`;
}

customElements.define('trunks-sign-in', TrunksSignIn);
