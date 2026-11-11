// What every data view of the console does the same way: wait for the session, load, refresh
// every 15 s while the tab is visible, and show the states of a data region (loading, empty,
// error, no permission, stale data). A view says what to load and how to draw it.

import '@websublime/line-components/button';
import { LitElement, html, nothing, type TemplateResult } from 'lit';

import { ApiError } from '../api/client';
import { takeFlash } from '../lib/flash';
import { clock } from '../lib/format';
import { Poller, REFRESH_SECONDS } from '../lib/poll';
import type { Session } from '../lib/session';
import { icon } from './icon';
import { state } from './parts';
import { session } from './trunks-shell';

export interface Heading {
  title: string;
  /** The title is an identifier: set in monospace. */
  mono?: boolean;
  sub?: string;
  /** `[label, href]` pairs leading to this page. */
  crumbs?: [string, string][];
  /** Buttons next to the refresh line. */
  actions?: TemplateResult;
}

export abstract class DataView<T> extends LitElement {
  static override properties = {
    session: { state: true },
    data: { state: true },
    error: { state: true },
    stale: { state: true },
    updatedAt: { state: true },
    notice: { state: true },
    busy: { state: true },
  };

  declare session: Session | undefined;
  declare data: T | undefined;
  /** Why nothing could be loaded. */
  declare error: Error | undefined;
  /** Why the last refresh failed; the previous data stays on screen. */
  declare stale: string | undefined;
  declare updatedAt: Date | undefined;
  /** The outcome of the last thing the person did here. */
  declare notice: { tone: 'success' | 'danger'; title: string; text?: string } | undefined;
  /** Keys of the changes in flight, so their controls cannot be pressed twice. */
  declare busy: ReadonlySet<string>;

  /** What this view lists, for messages: "servers". */
  protected abstract readonly subject: string;
  protected abstract load(session: Session): Promise<T>;
  protected abstract heading(data: T | undefined, session: Session): Heading;
  protected abstract content(data: T, session: Session): TemplateResult;

  readonly #poller = new Poller(() => void this.#refresh());

  // The console's styles are global: render into the page, not into a shadow root.
  protected override createRenderRoot(): HTMLElement {
    return this;
  }

  constructor() {
    super();
    this.busy = new Set();
  }

  override connectedCallback(): void {
    super.connectedCallback();
    const arrived = takeFlash();
    if (arrived !== undefined) this.notice = { tone: 'success', title: arrived };
    void session().then((s) => {
      this.session = s;
      this.#poller.start();
    });
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback();
    this.#poller.stop();
  }

  /** Loads again now. */
  protected refresh(): void {
    this.#poller.now();
  }

  /**
   * Makes a change and shows its outcome: the data is loaded again either way, and a failure
   * is reported in place with the server's own reason. `key` marks the control as busy.
   */
  protected async change(key: string, run: () => Promise<unknown>, failed: string): Promise<void> {
    if (this.busy.has(key)) return;
    this.busy = new Set(this.busy).add(key);
    try {
      await run();
      if (this.notice?.tone === 'danger') this.notice = undefined;
    } catch (e) {
      if (e instanceof ApiError && e.signedOut) {
        location.reload();
        return;
      }
      this.notice = {
        tone: 'danger',
        title: failed,
        text: `${e instanceof Error ? e.message : 'Unknown error'}. Nothing changed.`,
      };
    } finally {
      await this.#refresh();
      const busy = new Set(this.busy);
      busy.delete(key);
      this.busy = busy;
    }
  }

  async #refresh(): Promise<void> {
    if (this.session === undefined) return;
    try {
      this.data = await this.load(this.session);
      this.updatedAt = new Date();
      this.error = undefined;
      this.stale = undefined;
    } catch (e) {
      if (e instanceof ApiError && e.signedOut) {
        // The session ended while the page was open.
        location.reload();
        return;
      }
      const error = e instanceof Error ? e : new Error('Unknown error');
      const gone = e instanceof ApiError && (e.status === 404 || e.status === 403);
      // Something that disappeared or was taken away is not "stale": show that instead.
      if (this.data === undefined || gone) {
        this.data = undefined;
        this.error = error;
      } else {
        this.stale = error.message;
      }
    }
  }

  protected override render(): TemplateResult | typeof nothing {
    const s = this.session;
    if (s === undefined) return nothing;
    const h = this.heading(this.data, s);
    return html`
      <div class="ph">
        <div class="t">
          ${h.crumbs === undefined
            ? nothing
            : html`<nav class="crumb" aria-label="Breadcrumb">
                ${h.crumbs.map(([label, href]) => html`<a href=${href}>${label}</a>${icon('chevronRight')}`)}
                <span aria-current="page">${h.title}</span>
              </nav>`}
          <h1>${h.mono ? html`<span class="mono">${h.title}</span>` : h.title}</h1>
          ${h.sub === undefined || h.sub === '' ? nothing : html`<p class="sub">${h.sub}</p>`}
        </div>
        <div class="acts">
          <div class="updated">
            ${icon('clock')}
            <span
              >${this.updatedAt ? `Updated ${clock(this.updatedAt)} · ` : ''}refreshes every
              ${REFRESH_SECONDS} s</span
            >
            <line-button variant="borderless" aria-label="Refresh now" @click=${() => this.refresh()}
              >${icon('refresh')}</line-button
            >
          </div>
          ${h.actions ?? nothing}
        </div>
      </div>
      ${this.notice === undefined
        ? nothing
        : html`<div
            class="alert ${this.notice.tone}"
            role=${this.notice.tone === 'danger' ? 'alert' : 'status'}
          >
            ${icon(this.notice.tone === 'danger' ? 'alert' : 'checkCircle')}
            <div class="bd">
              <span class="ttl">${this.notice.title}</span>
              ${this.notice.text === undefined ? nothing : html`<span>${this.notice.text}</span>`}
            </div>
            <button
              type="button"
              class="btn ibtn ghost"
              aria-label="Dismiss"
              title="Dismiss"
              @click=${() => (this.notice = undefined)}
            >
              ${icon('close')}
            </button>
          </div>`}
      ${this.stale === undefined
        ? nothing
        : html`<div class="alert warning" role="status">
            ${icon('alert')}
            <div class="bd">
              <span class="ttl">Couldn’t refresh</span>
              <span
                >Showing data from ${this.updatedAt ? clock(this.updatedAt) : 'earlier'}. Trying
                again in ${REFRESH_SECONDS} s.</span
              >
            </div>
          </div>`}
      ${this.error !== undefined
        ? this.#failed(this.error)
        : this.data === undefined
          ? html`<section class="panel">
              <div class="state" role="status">
                ${icon('spinner', 'ic-lg spin')}<span>Loading ${this.subject}</span>
              </div>
            </section>`
          : this.content(this.data, s)}
    `;
  }

  #failed(error: Error): TemplateResult {
    const status = error instanceof ApiError ? error.status : -1;
    if (status === 404) {
      // The API answers the same for "does not exist" and "not yours to see".
      return html`<section class="panel">
        ${state('search', 'Nothing here', `There is no such entry among the ${this.subject} you can see.`)}
      </section>`;
    }
    if (status === 403) {
      return html`<section class="panel">
        ${state(
          'lock',
          `You can’t see ${this.subject}`,
          'This page is for administrators. Ask one if you need what it shows.',
          html`<a class="btn" href="/">Go to the overview</a>`,
        )}
      </section>`;
    }
    return html`<div class="alert danger" role="alert">
      ${icon('alert')}
      <div class="bd">
        <span class="ttl">Couldn’t load ${this.subject}</span>
        <span>${error.message}</span>
        <div class="act"><line-button @click=${() => this.refresh()}>Try again</line-button></div>
      </div>
    </div>`;
  }
}
