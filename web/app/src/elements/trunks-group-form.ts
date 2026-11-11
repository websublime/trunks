// <trunks-group-form [edit]>: creates a tool group, or changes one (`?name=`). The choice is
// made server by server: the whole server, or some of its tools.

import '@websublime/line-components/button';
import { LitElement, html, nothing, type TemplateResult } from 'lit';

import { ApiError, get } from '../api/client';
import type { GroupSummary, ServerSummary, Tool } from '../api/types.gen';
import { flash } from '../lib/flash';
import {
  effective,
  emptySelection,
  fromGroup,
  groupNameProblem,
  has,
  missing,
  setServer,
  setTool,
  toGroup,
  type KnownTool,
  type Selection,
} from '../lib/group';
import { createGroup, updateGroup } from '../lib/manage';
import type { Session } from '../lib/session';
import { count, param } from '../lib/text';
import { icon } from './icon';
import { badge, state } from './parts';
import { session } from './trunks-shell';

interface Loaded {
  servers: ServerSummary[];
  tools: Tool[];
}

class TrunksGroupForm extends LitElement {
  static override properties = {
    session: { state: true },
    loaded: { state: true },
    unavailable: { state: true },
    name: { state: true },
    description: { state: true },
    selection: { state: true },
    filter: { state: true },
    problem: { state: true },
    failure: { state: true },
    busy: { state: true },
  };

  declare session: Session | undefined;
  declare loaded: Loaded | undefined;
  /** Why the form could not be opened. */
  declare unavailable: Error | undefined;
  declare name: string;
  declare description: string;
  declare selection: Selection;
  declare filter: string;
  /** What the last attempt to save found wrong: `[field, message]`. */
  declare problem: ['name' | 'tools', string] | undefined;
  declare failure: string | undefined;
  declare busy: boolean;

  constructor() {
    super();
    this.name = '';
    this.description = '';
    this.selection = emptySelection();
    this.filter = '';
    this.busy = false;
  }

  get #editing(): boolean {
    return this.hasAttribute('edit');
  }

  protected override createRenderRoot(): HTMLElement {
    return this;
  }

  override connectedCallback(): void {
    super.connectedCallback();
    void session().then(async (s) => {
      this.session = s;
      if (!s.admin) return;
      try {
        const [servers, tools, group] = await Promise.all([
          get<ServerSummary[]>('/api/dashboard/servers'),
          get<Tool[]>('/api/dashboard/tools'),
          this.#editing
            ? get<GroupSummary>(`/api/dashboard/tool-groups/${encodeURIComponent(param('name') ?? '')}`)
            : undefined,
        ]);
        if (group !== undefined) {
          this.name = group.name;
          this.description = group.description;
          this.selection = fromGroup(group);
        }
        this.loaded = { servers, tools };
      } catch (e) {
        if (e instanceof ApiError && e.signedOut) location.reload();
        this.unavailable = e instanceof Error ? e : new Error('Unknown error');
      }
    });
  }

  async #submit(event: SubmitEvent, tools: KnownTool[]): Promise<void> {
    event.preventDefault();
    if (this.busy) return;
    this.failure = undefined;
    const nameProblem = this.#editing ? undefined : groupNameProblem(this.name.trim());
    this.problem =
      nameProblem !== undefined
        ? ['name', nameProblem]
        : effective(this.selection, tools).length === 0
          ? ['tools', 'Select at least one enabled tool: a group cannot be empty.']
          : undefined;
    if (this.problem !== undefined) {
      await this.updateComplete;
      this.querySelector<HTMLElement>(this.problem[0] === 'name' ? '#g-name' : '#g-filter')?.focus();
      return;
    }
    this.busy = true;
    const group = toGroup(this.name, this.description, this.selection);
    try {
      await (this.#editing ? updateGroup(group) : createGroup(group));
      flash(`Tool group ${group.name} ${this.#editing ? 'saved' : 'created'}.`);
      location.assign(`/groups/detail/?name=${encodeURIComponent(group.name)}`);
    } catch (e) {
      if (e instanceof ApiError && e.signedOut) {
        location.reload();
        return;
      }
      const message = e instanceof Error ? e.message : 'Unknown error';
      this.failure = /[.!?]$/.test(message) ? message : `${message}.`;
      this.busy = false;
      await this.updateComplete;
      this.querySelector<HTMLElement>('#g-failure')?.focus();
    }
  }

  protected override render(): TemplateResult | typeof nothing {
    const s = this.session;
    if (s === undefined) return nothing;
    const editing = this.#editing;
    const title = editing ? (param('name') ?? 'Tool group') : 'New tool group';
    const back = editing && this.name !== '' ? `/groups/detail/?name=${encodeURIComponent(this.name)}` : '/groups/';
    return html`
      <div class="ph">
        <div class="t">
          <nav class="crumb" aria-label="Breadcrumb">
            <a href="/groups/">Tool groups</a>${icon('chevronRight')}
            ${editing
              ? html`<a href=${back}>${title}</a>${icon('chevronRight')}<span aria-current="page">Edit</span>`
              : html`<span aria-current="page">New</span>`}
          </nav>
          <h1>${editing ? html`Edit <span class="mono">${title}</span>` : title}</h1>
          <p class="sub">A named selection of tools with its own MCP endpoint.</p>
        </div>
      </div>
      ${!s.admin
        ? html`<section class="panel">
            ${state(
              'lock',
              'You can’t change tool groups',
              'Only administrators create and change tool groups.',
              html`<a class="btn" href="/groups/">Back to tool groups</a>`,
            )}
          </section>`
        : this.unavailable !== undefined
          ? this.unavailable instanceof ApiError && this.unavailable.status === 404
            ? html`<section class="panel">
                ${state('search', 'Nothing here', 'There is no tool group of that name.')}
              </section>`
            : html`<div class="alert danger" role="alert">
                ${icon('alert')}
                <div class="bd">
                  <span class="ttl">Couldn’t open the form</span><span>${this.unavailable.message}</span>
                </div>
              </div>`
          : this.loaded === undefined
            ? html`<section class="panel">
                <div class="state" role="status">${icon('spinner', 'ic-lg spin')}<span>Loading</span></div>
              </section>`
            : this.#form(this.loaded, back)}
    `;
  }

  #form({ servers, tools }: Loaded, back: string): TemplateResult {
    const editing = this.#editing;
    const known: KnownTool[] = tools.map((t) => ({ name: t.name, server: t.server, enabled: t.enabled }));
    const exposed = effective(this.selection, known).length;
    const gone = missing(
      this.selection,
      servers.map((s) => s.name),
      known,
    );
    const goneCount = gone.servers.length + gone.included.length + gone.excluded.length;
    const needle = this.filter.trim().toLowerCase();
    const nameProblem = this.problem?.[0] === 'name' ? this.problem[1] : undefined;
    const toolsProblem = this.problem?.[0] === 'tools' ? this.problem[1] : undefined;
    return html`<form class="form" novalidate @submit=${(e: SubmitEvent) => void this.#submit(e, known)}>
      ${this.failure === undefined
        ? nothing
        : html`<div class="alert danger" role="alert" id="g-failure" tabindex="-1">
            ${icon('alert')}
            <div class="bd">
              <span class="ttl">Couldn’t ${editing ? 'save' : 'create'} the group</span>
              <span>${this.failure} Nothing changed.</span>
            </div>
          </div>`}
      <div class="col roomy narrow">
        ${editing
          ? nothing
          : html`<div class="fld">
              <label for="g-name">Name <span class="req">(required)</span></label>
              <div class="inp mono ${nameProblem === undefined ? '' : 'invalid'}">
                <input
                  id="g-name"
                  type="text"
                  spellcheck="false"
                  autocomplete="off"
                  autocapitalize="off"
                  aria-required="true"
                  aria-invalid=${nameProblem === undefined ? 'false' : 'true'}
                  aria-describedby=${nameProblem === undefined ? 'g-name-h' : 'g-name-e g-name-h'}
                  .value=${this.name}
                  @input=${(e: InputEvent) => (this.name = (e.target as HTMLInputElement).value)}
                />
              </div>
              ${nameProblem === undefined
                ? nothing
                : html`<div class="ferr" id="g-name-e">${icon('alert')}${nameProblem}</div>`}
              <div class="hint" id="g-name-h">
                Part of the endpoint:
                <span class="mono">/v0/groups/${this.name.trim() === '' ? 'name' : this.name.trim()}/mcp</span>. It
                cannot be changed later.
              </div>
            </div>`}
        <div class="fld">
          <label for="g-desc">Description</label>
          <div class="inp ta">
            <textarea
              id="g-desc"
              rows="2"
              .value=${this.description}
              @input=${(e: InputEvent) => (this.description = (e.target as HTMLTextAreaElement).value)}
            ></textarea>
          </div>
        </div>
      </div>
      <section class="panel" aria-labelledby="g-tools-h">
        <div class="panel-h">
          <h2 id="g-tools-h">Tools</h2>
          <span role="status">${badge(`${count(exposed, 'effective tool')}`, exposed === 0 ? 'warning' : 'accent')}</span>
        </div>
        <div class="panel-b col roomy">
          <p class="hint">
            Include a whole server to expose every enabled tool it has, also the ones it gains later, and
            untick the ones to leave out. Or leave the server out and tick tools one by one.
          </p>
          <div class="fld w-filter">
            <label for="g-filter">Filter tools</label>
            <div class="inp ${toolsProblem === undefined ? '' : 'invalid'}">
              ${icon('search')}
              <input
                id="g-filter"
                type="search"
                autocomplete="off"
                spellcheck="false"
                placeholder="Part of a name"
                aria-describedby=${toolsProblem === undefined ? nothing : 'g-tools-e'}
                .value=${this.filter}
                @input=${(e: InputEvent) => (this.filter = (e.target as HTMLInputElement).value)}
              />
            </div>
            ${toolsProblem === undefined
              ? nothing
              : html`<div class="ferr" id="g-tools-e" role="alert">${icon('alert')}${toolsProblem}</div>`}
          </div>
          ${servers.length === 0
            ? state('servers', 'No servers yet', 'Register a server first: a group selects among its tools.')
            : servers.map((server) =>
                this.#server(
                  server,
                  tools.filter((t) => t.server === server.name),
                  needle,
                ),
              )}
          ${goneCount === 0 ? nothing : this.#gone(gone)}
        </div>
      </section>
      <div class="row">
        <line-button variant="primary" type="submit" ?loading=${this.busy}
          >${editing ? 'Save changes' : 'Create group'}</line-button
        >
        <a class="btn" href=${back}>Cancel</a>
      </div>
    </form>`;
  }

  #server(server: ServerSummary, tools: Tool[], needle: string): TemplateResult | typeof nothing {
    const whole = this.selection.servers.has(server.name);
    const shown = needle === '' ? tools : tools.filter((t) => t.name.toLowerCase().includes(needle));
    if (needle !== '' && shown.length === 0) return nothing;
    const ticked = tools.filter((t) => has(this.selection, t.name)).length;
    return html`<fieldset class="pick">
      <legend class="vh">Server ${server.name}</legend>
      <div class="pick-h">
        <span class="name-t">${server.name}</span>
        ${server.enabled ? nothing : badge('Disabled', '', 'minusCircle')}
        <span class="small lo">${whole ? `${ticked} of ${tools.length} tools` : `${ticked} of ${tools.length} tools picked`}</span>
        <label class="ck end">
          <input
            type="checkbox"
            .checked=${whole}
            @change=${(e: Event) =>
              (this.selection = setServer(this.selection, server.name, (e.target as HTMLInputElement).checked))}
          />
          Whole server<span class="vh"> ${server.name}</span>
        </label>
      </div>
      ${shown.length === 0
        ? html`<p class="hint">This server has no tools right now.</p>`
        : html`<ul class="pick-l">
            ${shown.map((tool) => {
              const on = has(this.selection, tool.name);
              // A disabled tool cannot be picked by name; inside a whole server it is simply
              // not exposed until it is enabled again.
              const locked = !tool.enabled && !whole && !on;
              return html`<li>
                <label class="ck">
                  <input
                    type="checkbox"
                    .checked=${on}
                    ?disabled=${locked}
                    @change=${(e: Event) =>
                      (this.selection = setTool(this.selection, tool.name, (e.target as HTMLInputElement).checked))}
                  />
                  <span class="mono">${tool.name}</span>
                  ${tool.enabled ? nothing : badge('Disabled', '', 'minusCircle')}
                </label>
                ${tool.description === '' ? nothing : html`<span class="desc" title=${tool.description}>${tool.description}</span>`}
              </li>`;
            })}
          </ul>`}
    </fieldset>`;
  }

  /** Names the definition still holds for things the gateway no longer has. */
  #gone(gone: ReturnType<typeof missing>): TemplateResult {
    const row = (name: string, what: string, drop: () => void) =>
      html`<li>
        <span class="mono">${name}</span><span class="small lo">${what}</span>
        <button type="button" class="btn borderless" @click=${drop}>Remove<span class="vh"> ${name}</span></button>
      </li>`;
    const s = this.selection;
    const less = (set: ReadonlySet<string>, name: string) => new Set([...set].filter((n) => n !== name));
    return html`<div class="alert warning" role="status">
      ${icon('warning')}
      <div class="bd">
        <span class="ttl">This group names things the gateway no longer has</span>
        <span
          >A missing server or included tool has to be removed before the group can be saved; a missing
          exclusion is harmless.</span
        >
        <ul class="gone">
          ${gone.servers.map((n) =>
            row(n, 'server, not registered', () => (this.selection = { ...s, servers: less(s.servers, n) })),
          )}
          ${gone.included.map((n) =>
            row(n, 'included tool, not found', () => (this.selection = { ...s, included: less(s.included, n) })),
          )}
          ${gone.excluded.map((n) =>
            row(n, 'excluded tool, not found', () => (this.selection = { ...s, excluded: less(s.excluded, n) })),
          )}
        </ul>
      </div>
    </div>`;
  }
}

customElements.define('trunks-group-form', TrunksGroupForm);
