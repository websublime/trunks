// <trunks-overview>: what the gateway holds right now, for this viewer. Counts are of what
// they may use; the facts about the whole installation appear for administrators only,
// because the API sends them to nobody else.

import '@websublime/line-components/button';
import { LitElement, html, nothing, type TemplateResult } from 'lit';

import { ApiError, get } from '../api/client';
import type { Overview, ServerSummary } from '../api/types.gen';
import { bareVersion, clock, modeLabel, uptime } from '../lib/format';
import { page } from '../lib/nav';
import { Poller, REFRESH_SECONDS } from '../lib/poll';
import { hasNoAccess, type Session } from '../lib/session';
import { icon } from './icon';
import { session } from './trunks-shell';

class TrunksOverview extends LitElement {
  static override properties = {
    session: { state: true },
    overview: { state: true },
    failed: { state: true },
    error: { state: true },
    stale: { state: true },
    updatedAt: { state: true },
  };

  declare session: Session | undefined;
  declare overview: Overview | undefined;
  /** Servers whose last connection failed. */
  declare failed: ServerSummary[];
  /** Why nothing could be loaded. */
  declare error: string | undefined;
  /** Why the last refresh failed; the previous data stays on screen. */
  declare stale: string | undefined;
  declare updatedAt: Date | undefined;

  readonly #poller = new Poller(() => void this.#load());

  constructor() {
    super();
    this.failed = [];
  }

  // The console's styles are global: render into the page, not into a shadow root.
  protected override createRenderRoot(): HTMLElement {
    return this;
  }

  override connectedCallback(): void {
    super.connectedCallback();
    void session().then((s) => {
      this.session = s;
      if (!hasNoAccess(s)) this.#poller.start();
    });
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback();
    this.#poller.stop();
  }

  async #load(): Promise<void> {
    try {
      const [overview, servers] = await Promise.all([
        get<Overview>('/api/dashboard/overview'),
        get<ServerSummary[]>('/api/dashboard/servers'),
      ]);
      this.overview = overview;
      this.failed = servers.filter((s) => s.last_error != null);
      this.updatedAt = new Date();
      this.error = undefined;
      this.stale = undefined;
    } catch (e) {
      if (e instanceof ApiError && e.signedOut) {
        // The session ended while the page was open.
        location.reload();
        return;
      }
      const message = e instanceof Error ? e.message : 'Unknown error';
      if (this.overview === undefined) this.error = message;
      else this.stale = message;
    }
  }

  protected override render(): TemplateResult | typeof nothing {
    const s = this.session;
    if (s === undefined) return nothing;
    if (hasNoAccess(s)) return this.#noAccess(s);
    return html`
      <div class="ph">
        <div class="t">
          <h1>Overview</h1>
          <p class="sub">What this gateway holds right now.</p>
        </div>
        <div class="acts">${this.#refreshLine()}</div>
      </div>
      ${this.stale === undefined
        ? nothing
        : html`<div class="alert warning" role="status">
            ${icon('alert')}
            <div class="bd">
              <span class="ttl">Couldn’t refresh</span>
              <span
                >Showing data from ${this.updatedAt ? clock(this.updatedAt) : 'earlier'}. Trying again
                in ${REFRESH_SECONDS} s.</span
              >
            </div>
          </div>`}
      ${this.error !== undefined
        ? this.#failedToLoad(this.error)
        : this.overview === undefined
          ? this.#loading()
          : this.#loaded(s, this.overview)}
    `;
  }

  #refreshLine(): TemplateResult {
    return html`<div class="updated">
      ${icon('clock')}
      <span
        >${this.updatedAt ? `Updated ${clock(this.updatedAt)} · ` : ''}refreshes every
        ${REFRESH_SECONDS} s</span
      >
      <line-button variant="borderless" aria-label="Refresh now" @click=${() => this.#poller.now()}
        >${icon('refresh')}</line-button
      >
    </div>`;
  }

  #loading(): TemplateResult {
    return html`<div class="state" role="status">
      ${icon('spinner', 'ic-lg spin')}
      <span>Loading the overview</span>
    </div>`;
  }

  #failedToLoad(message: string): TemplateResult {
    return html`<div class="alert danger" role="alert">
      ${icon('alert')}
      <div class="bd">
        <span class="ttl">Couldn’t load the overview</span>
        <span>${message}</span>
        <div class="act">
          <line-button @click=${() => this.#poller.now()}>Try again</line-button>
        </div>
      </div>
    </div>`;
  }

  #loaded(s: Session, o: Overview): TemplateResult {
    const c = o.counts;
    return html`
      <section class="panel" aria-labelledby="gw-h">
        <div class="panel-h"><h2 id="gw-h">Gateway</h2></div>
        <div class="grid g3 flush">
          <div class="stat">
            <span class="k">Mode</span>
            <span><span class="bdg accent">${icon('shield')}${modeLabel(o.mode)}</span></span>
          </div>
          <div class="stat sep-l">
            <span class="k">Version</span>
            <span class="mono">${bareVersion(o.version)}</span>
          </div>
          <div class="stat sep-l">
            <span class="k">Uptime</span>
            <span>${uptime(o.uptime_seconds)}</span>
          </div>
        </div>
      </section>
      <section aria-labelledby="ct-h" class="col snug">
        <h2 id="ct-h" class="vh">Counts</h2>
        <div class="grid g4 keep2">
          ${card('Servers', page('/servers/'), c.servers_enabled, c.servers)}
          ${card('Tools', page('/tools/'), c.tools_enabled, c.tools)}
          ${card('Prompts', page('/prompts/'), c.prompts)}
          ${card('Resources', page('/resources/'), c.resources)}
          ${card('Tool groups', page('/groups/'), c.tool_groups)}
          ${s.admin ? card('Pending authorizations', page('/diagnostics/'), c.pending_oauth) : nothing}
          ${o.sessions == null
            ? nothing
            : card('Open upstream sessions', undefined, o.sessions.stateful_active)}
        </div>
      </section>
      ${this.failed.length === 0 ? nothing : this.#failures(this.failed)}
    `;
  }

  #failures(servers: ServerSummary[]): TemplateResult {
    const count = servers.length === 1 ? '1 server' : `${servers.length} servers`;
    const detail = page('/servers/detail/');
    return html`<section class="panel" aria-labelledby="fl-h">
      <div class="panel-h">
        <h2 id="fl-h">Last connection failed</h2>
        <span class="bdg danger">${icon('alert')}${count}</span>
      </div>
      <ul class="list">
        ${servers.map(
          (server) =>
            html`<li>
              <div class="col grow tight">
                <div class="row">
                  <span class="name-t">${server.name}</span>
                  <span class="bdg mono"
                    >${icon(server.transport === 'stdio' ? 'terminal' : 'globe')}${server.transport}</span
                  >
                  ${server.oauth.status === 'pending'
                    ? html`<span class="bdg warning">${icon('clock')}Pending</span>`
                    : nothing}
                </div>
                <div class="err">${icon('alert')}<span>${server.last_error}</span></div>
              </div>
              ${detail === undefined
                ? nothing
                : html`<a class="btn" href="${detail}?name=${encodeURIComponent(server.name)}"
                    >View server</a
                  >`}
            </li>`,
        )}
      </ul>
    </section>`;
  }

  #noAccess(s: Session): TemplateResult {
    const teams = s.user?.teams ?? [];
    return html`
      <div class="ph">
        <div class="t"><h1>Overview</h1></div>
      </div>
      <section class="panel">
        <div class="state">
          ${icon('lock', 'ic-xl')}
          <span class="ttl">You don’t have access to anything yet</span>
          <p>
            Trunks shows only the servers and tool groups you have been granted. Your account has
            no grants, directly or through a team.
          </p>
          <p>
            Ask an administrator of this Trunks instance to grant you a server or a tool group.
          </p>
        </div>
        <div class="grid g3 flush">
          <div class="stat">
            <span class="k">Username</span><span class="mono">${s.user?.username}</span>
          </div>
          <div class="stat sep-l"><span class="k">Role</span><span>${s.user?.role}</span></div>
          <div class="stat sep-l">
            <span class="k">Teams</span>
            <span>${teams.length === 0 ? 'None' : teams.map((t) => t.name).join(', ')}</span>
          </div>
        </div>
      </section>
    `;
  }
}

/** A count card. A link when the console has the page it leads to. */
function card(label: string, href: string | undefined, value: number, total?: number) {
  const body = html`<span class="k">${label}</span>
    <span class="v"
      >${value}${total === undefined ? nothing : html` <span class="of">/ ${total}</span>`}</span
    >
    ${total === undefined ? nothing : html`<span class="x">enabled / total</span>`}`;
  return href === undefined
    ? html`<div class="panel stat">${body}</div>`
    : html`<a class="panel stat card" href=${href}>${body}</a>`;
}

customElements.define('trunks-overview', TrunksOverview);
