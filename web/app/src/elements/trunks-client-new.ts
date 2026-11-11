// <trunks-client-new>: creates an MCP client and shows its token, once.

import '@websublime/line-components/button';
import { LitElement, html, nothing, type TemplateResult } from 'lit';

import { ApiError, get } from '../api/client';
import type { Owner, ServerSummary, Team, User } from '../api/types.gen';
import { identityNameProblem } from '../lib/access';
import { flash } from '../lib/flash';
import { createClient } from '../lib/manage';
import type { Session } from '../lib/session';
import { showToken } from './dialog';
import { icon } from './icon';
import { state, textField } from './parts';
import { CLIENT_TOKEN_USE } from './trunks-clients';
import { session } from './trunks-shell';

type OwnerKind = 'none' | 'user' | 'team';

interface Loaded {
  servers: string[];
  /** Users a client can be given to: everyone for an administrator, otherwise only oneself. */
  users: string[];
  /** Teams a client can be given to: all for an administrator, otherwise the ones maintained. */
  teams: string[];
}

class TrunksClientNew extends LitElement {
  static override properties = {
    session: { state: true },
    loaded: { state: true },
    unavailable: { state: true },
    name: { state: true },
    description: { state: true },
    ownerKind: { state: true },
    ownerName: { state: true },
    limited: { state: true },
    allowed: { state: true },
    problems: { state: true },
    failure: { state: true },
    busy: { state: true },
  };

  declare session: Session | undefined;
  declare loaded: Loaded | undefined;
  declare unavailable: string | undefined;
  declare name: string;
  declare description: string;
  declare ownerKind: OwnerKind;
  declare ownerName: string;
  /** Whether the client is limited to the servers ticked. */
  declare limited: boolean;
  declare allowed: ReadonlySet<string>;
  declare problems: { name?: string; owner?: string; servers?: string };
  declare failure: string | undefined;
  declare busy: boolean;

  constructor() {
    super();
    this.name = '';
    this.description = '';
    this.ownerKind = 'user';
    this.ownerName = '';
    this.limited = false;
    this.allowed = new Set();
    this.problems = {};
    this.busy = false;
  }

  protected override createRenderRoot(): HTMLElement {
    return this;
  }

  override connectedCallback(): void {
    super.connectedCallback();
    void session().then(async (s) => {
      this.session = s;
      const me = s.user;
      if (s.mode !== 'enterprise' || me === undefined) return;
      try {
        const [servers, users, teams] = await Promise.all([
          get<ServerSummary[]>('/api/dashboard/servers'),
          s.admin ? get<User[]>('/api/v0/users') : undefined,
          s.admin ? get<Team[]>('/api/v0/teams') : undefined,
        ]);
        this.ownerName = me.username;
        this.loaded = {
          servers: servers.map((server) => server.name),
          users: users?.map((u) => u.username) ?? [me.username],
          teams: teams?.map((t) => t.name) ?? me.teams.filter((t) => t.role === 'maintainer').map((t) => t.name),
        };
      } catch (e) {
        if (e instanceof ApiError && e.signedOut) location.reload();
        this.unavailable = e instanceof Error ? e.message : 'Unknown error';
      }
    });
  }

  #owner(): Owner | undefined {
    return this.ownerKind === 'none' ? undefined : { kind: this.ownerKind, name: this.ownerName };
  }

  async #submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (this.busy) return;
    this.failure = undefined;
    const name = this.name.trim();
    const problems: TrunksClientNew['problems'] = {};
    const nameProblem = identityNameProblem(name);
    if (nameProblem !== undefined) problems.name = nameProblem;
    if (this.ownerKind !== 'none' && this.ownerName === '') problems.owner = 'Choose the owner.';
    if (this.limited && this.allowed.size === 0) problems.servers = 'Tick at least one server.';
    this.problems = problems;
    const first = problems.name ? '#c-name' : problems.owner ? '#c-owner' : problems.servers ? '#c-servers input' : '';
    if (first !== '') {
      await this.updateComplete;
      this.querySelector<HTMLElement>(first)?.focus();
      return;
    }
    this.busy = true;
    const owner = this.#owner();
    try {
      const created = await createClient({
        name,
        description: this.description.trim(),
        // An ownerless client reaches nothing unless told: "every server" is the explicit `*`.
        allow_list: this.limited ? [...this.allowed].sort() : owner === undefined ? ['*'] : [],
        ...(owner === undefined ? {} : { owner }),
      });
      await showToken(created.name, created.access_token ?? '', CLIENT_TOKEN_USE);
      flash(`MCP client ${created.name} created.`);
      location.assign('/clients/');
    } catch (e) {
      if (e instanceof ApiError && e.signedOut) {
        location.reload();
        return;
      }
      const message = e instanceof Error ? e.message : 'Unknown error';
      if (e instanceof ApiError && e.status === 400 && message.includes('already exists')) {
        this.problems = { name: 'An MCP client of this name already exists.' };
        this.busy = false;
        await this.updateComplete;
        this.querySelector<HTMLElement>('#c-name')?.focus();
        return;
      }
      this.failure = /[.!?]$/.test(message) ? message : `${message}.`;
      this.busy = false;
      await this.updateComplete;
      this.querySelector<HTMLElement>('#c-failure')?.focus();
    }
  }

  protected override render(): TemplateResult | typeof nothing {
    const s = this.session;
    if (s === undefined) return nothing;
    return html`
      <div class="ph">
        <div class="t">
          <nav class="crumb" aria-label="Breadcrumb">
            <a href="/clients/">MCP clients</a>${icon('chevronRight')}<span aria-current="page">New</span>
          </nav>
          <h1>New MCP client</h1>
          <p class="sub">Creating a client gives you its access token once.</p>
        </div>
      </div>
      ${s.mode !== 'enterprise'
        ? html`<section class="panel">
            ${state(
              'info',
              'Only in enterprise mode',
              'In development mode MCP clients connect without a token.',
              html`<a class="btn" href="/">Go to the overview</a>`,
            )}
          </section>`
        : this.unavailable !== undefined
          ? html`<div class="alert danger" role="alert">
              ${icon('alert')}
              <div class="bd"><span class="ttl">Couldn’t open the form</span><span>${this.unavailable}</span></div>
            </div>`
          : this.loaded === undefined
            ? html`<section class="panel">
                <div class="state" role="status">${icon('spinner', 'ic-lg spin')}<span>Loading</span></div>
              </section>`
            : this.#form(s, this.loaded)}
    `;
  }

  #form(s: Session, loaded: Loaded): TemplateResult {
    const kinds: [OwnerKind, string][] = s.admin
      ? [
          ['user', 'A user'],
          ['team', 'A team'],
          ['none', 'Nobody'],
        ]
      : loaded.teams.length > 0
        ? [
            ['user', 'Me'],
            ['team', 'A team I maintain'],
          ]
        : [['user', 'Me']];
    const candidates = this.ownerKind === 'team' ? loaded.teams : loaded.users;
    const ownerless = this.ownerKind === 'none';
    const pick = (kind: OwnerKind) => {
      this.ownerKind = kind;
      const names = kind === 'team' ? loaded.teams : loaded.users;
      this.ownerName = kind === 'user' && s.user ? s.user.username : (names[0] ?? '');
    };
    return html`<form class="form narrow" novalidate @submit=${(e: SubmitEvent) => void this.#submit(e)}>
      ${this.failure === undefined
        ? nothing
        : html`<div class="alert danger" role="alert" id="c-failure" tabindex="-1">
            ${icon('alert')}
            <div class="bd">
              <span class="ttl">Couldn’t create the client</span><span>${this.failure} Nothing changed.</span>
            </div>
          </div>`}
      ${textField({
        id: 'c-name',
        label: 'Name',
        value: this.name,
        input: (v) => (this.name = v),
        problem: this.problems.name,
        required: true,
        mono: true,
        hint: 'One per agent or application. No spaces.',
      })}
      <div class="fld">
        <label for="c-desc">Description</label>
        <div class="inp ta">
          <textarea
            id="c-desc"
            rows="2"
            .value=${this.description}
            @input=${(e: InputEvent) => (this.description = (e.target as HTMLTextAreaElement).value)}
          ></textarea>
        </div>
      </div>
      <fieldset class="fs">
        <legend>Owner</legend>
        ${kinds.length === 1
          ? html`<p>You (<span class="mono">${this.ownerName}</span>).</p>`
          : html`<div class="row wide-gap">
              ${kinds.map(
                ([kind, label]) =>
                  html`<label class="ck">
                    <input type="radio" name="c-kind" .checked=${this.ownerKind === kind} @change=${() => pick(kind)} />
                    ${label}
                  </label>`,
              )}
            </div>`}
        ${ownerless || (this.ownerKind === 'user' && !s.admin)
          ? nothing
          : html`<div class="fld w-filter">
              <label for="c-owner">${this.ownerKind === 'team' ? 'Team' : 'User'}</label>
              <div class="inp mono ${this.problems.owner === undefined ? '' : 'invalid'}">
                <select
                  id="c-owner"
                  aria-invalid=${this.problems.owner === undefined ? 'false' : 'true'}
                  @change=${(e: Event) => (this.ownerName = (e.target as HTMLSelectElement).value)}
                >
                  ${candidates.map((name) => html`<option value=${name} ?selected=${name === this.ownerName}>${name}</option>`)}
                </select>
              </div>
              ${this.problems.owner === undefined
                ? nothing
                : html`<div class="ferr">${icon('alert')}${this.problems.owner}</div>`}
            </div>`}
        <p class="hint">
          ${ownerless
            ? 'A client without an owner reaches exactly the servers chosen below, whatever anyone was granted.'
            : 'The client can use what its owner can, tool groups included, and loses it when the owner does.'}
        </p>
      </fieldset>
      <fieldset class="fs" id="c-servers">
        <legend>Can reach</legend>
        <label class="ck">
          <input type="radio" name="c-reach" .checked=${!this.limited} @change=${() => (this.limited = false)} />
          ${ownerless ? 'Every server' : 'Everything its owner can use'}
        </label>
        <label class="ck">
          <input type="radio" name="c-reach" .checked=${this.limited} @change=${() => (this.limited = true)} />
          Only these servers${ownerless ? '' : ', as far as its owner can use them'}
        </label>
        ${this.limited
          ? html`${loaded.servers.length === 0
                ? html`<p class="hint">There are no servers you can see yet.</p>`
                : html`<ul class="pick-l bordered">
                    ${loaded.servers.map(
                      (name) =>
                        html`<li>
                          <label class="ck">
                            <input
                              type="checkbox"
                              .checked=${this.allowed.has(name)}
                              @change=${(e: Event) => {
                                const next = new Set(this.allowed);
                                if ((e.target as HTMLInputElement).checked) next.add(name);
                                else next.delete(name);
                                this.allowed = next;
                              }}
                            />
                            <span class="mono">${name}</span>
                          </label>
                        </li>`,
                    )}
                  </ul>`}
              ${this.problems.servers === undefined
                ? nothing
                : html`<div class="ferr" role="alert">${icon('alert')}${this.problems.servers}</div>`}
              ${ownerless
                ? nothing
                : html`<p class="hint">Limited to servers, the client gets nothing through its owner’s tool groups.</p>`}`
          : nothing}
      </fieldset>
      <div class="row">
        <line-button variant="primary" type="submit" ?loading=${this.busy}>Create client</line-button>
        <a class="btn" href="/clients/">Cancel</a>
      </div>
    </form>`;
  }
}

customElements.define('trunks-client-new', TrunksClientNew);
