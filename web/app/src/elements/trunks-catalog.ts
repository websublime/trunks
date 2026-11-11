// <trunks-catalog kind="tools|prompts|resources">: everything the gateway exposes to this
// viewer, by upstream server. Filters are query parameters, so a filtered list can be
// linked to and survives a reload.

import { html, nothing, type TemplateResult } from 'lit';

import { get } from '../api/client';
import type { Overview, Prompt, Resource, ServerSummary, Tool } from '../api/types.gen';
import { promptArguments, toolParameters, type Parameter } from '../lib/schema';
import { setEntity } from '../lib/manage';
import { middle, param, withParams } from '../lib/text';
import { DataView, type Heading } from './data-view';
import { icon } from './icon';
import { badge, nameCell, state, statusBadge, toggle } from './parts';

type Kind = 'tools' | 'prompts' | 'resources';

interface Entry {
  name: string;
  shown: string;
  description: string;
  server: string;
  enabled: boolean;
  parameters: Parameter[];
  /** Mime type, for resources. */
  mime: string;
}

interface Catalog {
  entries: Entry[];
  servers: string[];
  counts: Record<Kind, number>;
}

const PAGE = 50;
const KINDS: Kind[] = ['tools', 'prompts', 'resources'];
const LABEL: Record<Kind, [string, string, string]> = {
  tools: ['Tools', 'Tool', 'Every tool the gateway exposes, by upstream server.'],
  prompts: ['Prompts', 'Prompt', 'Every prompt the gateway exposes, by upstream server.'],
  resources: ['Resources', 'Resource', 'Every resource the gateway exposes, by upstream server.'],
};

class TrunksCatalog extends DataView<Catalog> {
  static override properties = {
    ...DataView.properties,
    open: { state: true },
    limit: { state: true },
  };

  /** Names of the rows whose parameters are shown. */
  declare open: Set<string>;
  declare limit: number;

  constructor() {
    super();
    this.open = new Set();
    this.limit = PAGE;
  }

  get #kind(): Kind {
    const kind = this.getAttribute('kind');
    return KINDS.find((k) => k === kind) ?? 'tools';
  }

  protected get subject(): string {
    return this.#kind;
  }

  protected async load(): Promise<Catalog> {
    const kind = this.#kind;
    const [items, servers, overview] = await Promise.all([
      get<(Tool | Prompt | Resource)[]>(`/api/dashboard/${kind}`),
      get<ServerSummary[]>('/api/dashboard/servers'),
      get<Overview>('/api/dashboard/overview'),
    ]);
    return {
      entries: items.map((item) => entry(kind, item)),
      servers: servers.map((s) => s.name),
      counts: {
        tools: overview.counts.tools,
        prompts: overview.counts.prompts,
        resources: overview.counts.resources,
      },
    };
  }

  protected heading(): Heading {
    const [title, , sub] = LABEL[this.#kind];
    return { title, sub };
  }

  protected content(catalog: Catalog): TemplateResult {
    const kind = this.#kind;
    const [, singular] = LABEL[kind];
    const server = param('server');
    const enabled = param('enabled');
    const ofServer = catalog.entries.filter((e) => server === undefined || e.server === server);
    const shownAll = ofServer.filter(
      (e) => enabled === undefined || e.enabled === (enabled === 'true'),
    );
    const shown = shownAll.slice(0, this.limit);
    const chip = (label: string, value: string | undefined, n: number) =>
      html`<a
        class="chip"
        href=${withParams({ enabled: value })}
        aria-current=${enabled === value ? 'true' : nothing}
        >${label} <span class="n">${n}</span></a
      >`;
    return html`
      <nav class="tabs" aria-label="Catalogue">
        ${KINDS.map(
          (k) =>
            html`<a class="tab" href="/${k}/" aria-current=${k === kind ? 'page' : nothing}
              >${LABEL[k][0]} ${badge(catalog.counts[k])}</a
            >`,
        )}
      </nav>
      <div class="filters">
        <div class="fld">
          <label for="by-server">Server</label>
          <div class="inp">
            <select
              id="by-server"
              @change=${(e: Event) =>
                location.assign(
                  withParams({ server: (e.target as HTMLSelectElement).value || undefined }),
                )}
            >
              <option value="" ?selected=${server === undefined}>All servers</option>
              ${catalog.servers.map(
                (s) => html`<option value=${s} ?selected=${s === server}>${s}</option>`,
              )}
            </select>
          </div>
        </div>
        <div class="fld">
          <span class="lb" id="by-enabled">Status</span>
          <div class="chips" role="group" aria-labelledby="by-enabled">
            ${chip('All', undefined, ofServer.length)}
            ${chip('Enabled', 'true', ofServer.filter((e) => e.enabled).length)}
            ${chip('Disabled', 'false', ofServer.filter((e) => !e.enabled).length)}
          </div>
        </div>
      </div>
      <section class="panel">
        ${shownAll.length === 0
          ? state(
              kind,
              catalog.entries.length === 0 ? `No ${kind} yet` : `No ${kind} match`,
              catalog.entries.length === 0
                ? `No server you can use exposes ${kind}.`
                : 'Change the filters to see more.',
            )
          : html`<div class="tblw">
                <table class="tbl stack">
                  <thead>
                    <tr>
                      <th scope="col">${singular}</th>
                      <th scope="col">Server</th>
                      ${kind === 'resources'
                        ? html`<th scope="col">Type</th>`
                        : html`<th scope="col" class="num">
                            ${kind === 'tools' ? 'Parameters' : 'Arguments'}
                          </th>`}
                      <th scope="col" class="end">${this.session?.admin ? 'Enabled' : 'Status'}</th>
                    </tr>
                  </thead>
                  <tbody>
                    ${shown.map((e) => this.#row(kind, singular, e))}
                  </tbody>
                </table>
              </div>
              <div class="more">
                <span>Showing ${shown.length} of ${shownAll.length}</span>
                ${shown.length < shownAll.length
                  ? html`<line-button @click=${() => (this.limit += PAGE)}>Show more</line-button>`
                  : nothing}
              </div>`}
      </section>
    `;
  }

  #row(kind: Kind, singular: string, e: Entry): TemplateResult {
    const expandable = kind !== 'resources' && e.parameters.length > 0;
    const open = this.open.has(e.name);
    const expand = () => {
      const next = new Set(this.open);
      if (open) next.delete(e.name);
      else next.add(e.name);
      this.open = next;
    };
    const what = kind === 'tools' ? 'Parameters' : 'Arguments';
    return html`<tr class=${open ? 'open' : ''}>
        <td data-label=${singular} class="lead">
          <div class="row snug">
            ${expandable
              ? html`<button
                  type="button"
                  class="exp-btn"
                  aria-expanded=${open ? 'true' : 'false'}
                  aria-label=${`${what} of ${e.name}`}
                  @click=${expand}
                >
                  ${icon('chevronRight', 'chev')}
                </button>`
              : html`<span class="exp-btn" aria-hidden="true"></span>`}
            ${nameCell(e.name, e.description, undefined, e.shown)}
          </div>
        </td>
        <td data-label="Server">
          <a class="name" href="/servers/detail/?name=${encodeURIComponent(e.server)}">${e.server}</a>
        </td>
        ${kind === 'resources'
          ? html`<td data-label="Type">${e.mime === '' ? html`<span class="lo">—</span>` : badge(e.mime, '', undefined, true)}</td>`
          : html`<td data-label=${what} class="num">${e.parameters.length}</td>`}
        <td data-label=${this.session?.admin ? 'Enabled' : 'Status'} class="act">
          ${this.session?.admin
            ? toggle(
                e.enabled,
                `Enabled: ${e.name}`,
                (on) =>
                  void this.change(
                    e.name,
                    () => setEntity(kind, e.name, on),
                    `Couldn’t ${on ? 'enable' : 'disable'} ${e.name}`,
                  ),
                this.busy.has(e.name),
              )
            : statusBadge(e.enabled)}
        </td>
      </tr>
      ${open
        ? html`<tr>
            <td colspan="4" class="detail">
              <table class="params">
                <caption class="vh">
                  ${what} of ${e.name}
                </caption>
                <thead>
                  <tr>
                    <th scope="col">${kind === 'tools' ? 'Parameter' : 'Argument'}</th>
                    <th scope="col">Type</th>
                    <th scope="col">Required</th>
                    <th scope="col">Description</th>
                  </tr>
                </thead>
                <tbody>
                  ${e.parameters.map(
                    (p) =>
                      html`<tr>
                        <td data-label=${kind === 'tools' ? 'Parameter' : 'Argument'} class="mono">
                          ${p.name}
                        </td>
                        <td data-label="Type" class="mono">${p.type}</td>
                        <td data-label="Required">${p.required ? 'Required' : 'Optional'}</td>
                        <td data-label="Description" class="wrap-any">
                          ${p.description === '' ? html`<span class="lo">—</span>` : p.description}
                        </td>
                      </tr>`,
                  )}
                </tbody>
              </table>
            </td>
          </tr>`
        : nothing}`;
  }
}

function entry(kind: Kind, item: Tool | Prompt | Resource): Entry {
  if (kind === 'resources') {
    const r = item as Resource;
    return {
      name: r.uri,
      shown: middle(r.uri),
      description: r.name === '' ? r.description : r.name,
      server: r.server,
      enabled: r.enabled,
      parameters: [],
      mime: r.mime_type ?? '',
    };
  }
  const named = item as Tool | Prompt;
  return {
    name: named.name,
    shown: named.name,
    description: named.description,
    server: named.server,
    enabled: named.enabled,
    parameters:
      kind === 'tools'
        ? toolParameters((item as Tool).input_schema)
        : promptArguments((item as Prompt).arguments),
    mime: '',
  };
}

customElements.define('trunks-catalog', TrunksCatalog);
