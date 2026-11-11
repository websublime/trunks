// <trunks-group>: one tool group — its endpoint, the tools it actually exposes and why each
// one is there.

import { html, nothing, type TemplateResult } from 'lit';

import { ApiError, get } from '../api/client';
import type { GroupSummary, ServerSummary } from '../api/types.gen';
import { flash } from '../lib/flash';
import { deleteGroup } from '../lib/manage';
import { param } from '../lib/text';
import { DataView, type Heading } from './data-view';
import { confirm } from './dialog';
import { icon } from './icon';
import { badge, nameCell, none, state } from './parts';

interface Group {
  group: GroupSummary;
  servers: Map<string, ServerSummary>;
}

class TrunksGroup extends DataView<Group> {
  protected readonly subject = 'tool groups';

  get #name(): string {
    return param('name') ?? '';
  }

  protected async load(): Promise<Group> {
    if (this.#name === '') throw new ApiError(404, 'no group named');
    const [group, servers] = await Promise.all([
      get<GroupSummary>(`/api/dashboard/tool-groups/${encodeURIComponent(this.#name)}`),
      get<ServerSummary[]>('/api/dashboard/servers'),
    ]);
    return { group, servers: new Map(servers.map((s) => [s.name, s])) };
  }

  protected heading(data: Group | undefined): Heading {
    return {
      title: data?.group.name ?? (this.#name === '' ? 'Tool group' : this.#name),
      mono: true,
      crumbs: [['Tool groups', '/groups/']],
      ...(data ? { sub: data.group.description } : {}),
      ...(data && this.session?.admin ? { actions: this.#actions(data.group) } : {}),
    };
  }

  #actions(g: GroupSummary): TemplateResult {
    return html`
      <a class="btn" href="/groups/edit/?name=${encodeURIComponent(g.name)}">${icon('edit')}Edit</a>
      <button type="button" class="btn danger" @click=${() => void this.#delete(g)}>
        ${icon('trash')}Delete
      </button>
    `;
  }

  async #delete(g: GroupSummary): Promise<void> {
    const deleted = await confirm({
      title: `Delete ${g.name}?`,
      body: html`Its endpoint stops answering at once, so clients pointed at
        <span class="mono">${g.streamable_http_endpoint}</span> lose their tools. The servers and tools it
        names are not affected.`,
      action: 'Delete group',
      working: 'Deleting…',
      failed: 'Couldn’t delete',
      run: () => deleteGroup(g.name),
    });
    if (deleted) {
      flash(`Tool group ${g.name} deleted.`);
      location.assign('/groups/');
    }
  }

  protected content({ group: g, servers }: Group): TemplateResult {
    const endpoint = new URL(g.streamable_http_endpoint, location.origin).href;
    const direct = new Set(g.included_tools);
    return html`
      ${g.degraded ? this.#degraded(g, servers) : nothing}
      <section class="panel">
        <div class="panel-h"><h2>Endpoint</h2></div>
        <div class="panel-b col">
          <p class="sub">Point MCP clients here to see only this group’s tools.</p>
          <div class="mono-box">
            <span class="v" title=${endpoint}>${endpoint}</span>
            <trunks-copy always value=${endpoint} label="Copy URL"></trunks-copy>
          </div>
        </div>
      </section>
      <section class="panel" aria-labelledby="ef-h">
        <div class="panel-h">
          <h2 id="ef-h">Effective tools</h2>
          ${badge(g.effective_tools.length)}
        </div>
        ${g.effective_tools.length === 0
          ? state(
              'tools',
              'No effective tools',
              'The included servers expose no enabled tools, or every tool is excluded.',
            )
          : html`<div class="tblw">
              <table class="tbl stack">
                <thead>
                  <tr>
                    <th scope="col">Tool</th>
                    <th scope="col">Source</th>
                  </tr>
                </thead>
                <tbody>
                  ${g.effective_tools.map((tool) => {
                    const server = tool.split('__')[0] ?? '';
                    return html`<tr>
                      <td data-label="Tool" class="lead">${nameCell(tool, '')}</td>
                      <td data-label="Source">
                        ${direct.has(tool)
                          ? 'included tool'
                          : html`server <span class="mono">${server}</span>`}
                      </td>
                    </tr>`;
                  })}
                </tbody>
              </table>
            </div>`}
      </section>
      <div class="grid g3 top">
        ${list('Included servers', g.included_servers, (name) =>
          servers.has(name) ? `/servers/detail/?name=${encodeURIComponent(name)}` : undefined,
        )}
        ${list('Included tools', g.included_tools)} ${list('Excluded tools', g.excluded_tools)}
      </div>
    `;
  }

  /** Says why the group is empty when what the viewer can see explains it. */
  #degraded(g: GroupSummary, servers: Map<string, ServerSummary>): TemplateResult {
    const causes = g.included_servers.flatMap((name) => {
      const s = servers.get(name);
      if (s === undefined) return [`${name} is not registered, or you cannot see it`];
      if (!s.enabled) return [`${name} is disabled`];
      if (s.oauth.status === 'pending') return [`${name} has no tools yet because its authorization is pending`];
      if (s.counts.tools_enabled === 0) return [`${name} has no enabled tools`];
      return [];
    });
    return html`<div class="alert warning" role="status">
      ${icon('warning')}
      <div class="bd">
        <span class="ttl">This group has no effective tools</span>
        <span
          >Clients connected to its endpoint get an empty tool list.
          ${causes.length === 0 ? nothing : `${causes.join('; ')}.`}</span
        >
      </div>
    </div>`;
  }
}

function list(title: string, names: string[], href?: (name: string) => string | undefined) {
  return html`<section class="panel">
    <div class="panel-h">
      <h2>${title}</h2>
      ${badge(names.length)}
    </div>
    <div class="panel-b">
      ${names.length === 0
        ? none('None.')
        : html`<ul class="plain">
            ${names.map((name) => {
              const target = href?.(name);
              return html`<li>
                ${target === undefined
                  ? html`<span class="name-t" title=${name}>${name}</span>`
                  : html`<a class="name" href=${target}>${name}</a>`}
              </li>`;
            })}
          </ul>`}
    </div>
  </section>`;
}

customElements.define('trunks-group', TrunksGroup);
