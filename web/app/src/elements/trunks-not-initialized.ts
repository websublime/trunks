// <trunks-not-initialized>: the server is running but nobody has run `trunks init-server`
// yet. The page waits and opens the console when that changes.

import '@websublime/line-components/button';
import { LitElement, html, type TemplateResult } from 'lit';

import { get } from '../api/client';
import type { AuthConfig, ServerMetadata } from '../api/types.gen';
import { Poller, REFRESH_SECONDS } from '../lib/poll';

class TrunksNotInitialized extends LitElement {
  static override properties = { version: { state: true }, checking: { state: true } };

  declare version: string;
  declare checking: boolean;

  readonly #poller = new Poller(() => void this.#check());

  constructor() {
    super();
    this.version = '';
    this.checking = false;
  }

  protected override createRenderRoot(): HTMLElement {
    return this;
  }

  override connectedCallback(): void {
    super.connectedCallback();
    void get<ServerMetadata>('/metadata').then(
      (m) => (this.version = m.version),
      () => undefined,
    );
    this.#poller.start();
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback();
    this.#poller.stop();
  }

  async #check(): Promise<void> {
    this.checking = true;
    try {
      const config = await get<AuthConfig>('/auth/config');
      if (config.mode !== null) return location.replace('/');
    } catch {
      // keep waiting: the next check may reach the server
    } finally {
      this.checking = false;
      document.documentElement.dataset['state'] = 'ready';
    }
  }

  protected override render(): TemplateResult {
    return html`
      <div class="panel wide">
        <div class="col head">
          <h1>The server is not initialized</h1>
          <p>
            The gateway is running but setup has not finished, so there is nothing to show yet.
            An operator completes setup on the host with <code>trunks init-server</code>.
          </p>
        </div>
        <div class="alert info" role="status">
          <div class="bd">
            <span class="ttl">Nothing for you to do here</span>
            <span
              >This page checks again every ${REFRESH_SECONDS} seconds and opens the console when
              the server is ready.</span
            >
          </div>
        </div>
        <line-button ?loading=${this.checking} @click=${() => this.#poller.now()}
          >Check now</line-button
        >
      </div>
      <p class="ver">Trunks ${this.version}</p>
    `;
  }
}

customElements.define('trunks-not-initialized', TrunksNotInitialized);
