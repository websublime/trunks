// <trunks-server>: one upstream server — what it is, how it is reached (names only, never
// values) and the tools, prompts and resources of it this viewer may use.

import { html, nothing, type TemplateResult } from 'lit';

import { ApiError, get } from '../api/client';
import type { ServerDetail } from '../api/types.gen';
import { flash } from '../lib/flash';
import { deleteServer, setEntity, setServer, type EntityKind } from '../lib/manage';
import { count, middle, param, withParams } from '../lib/text';
import { DataView, type Heading } from './data-view';
import { confirm } from './dialog';
import { icon } from './icon';
import {
  badge,
  errorText,
  nameCell,
  none,
  oauthBadge,
  state,
  statusBadge,
  toggle,
  transportBadge,
} from './parts';

type Tab = 'tools' | 'prompts' | 'resources';
const TABS: Tab[] = ['tools', 'prompts', 'resources'];

class TrunksServer extends DataView<ServerDetail> {
  protected readonly subject = 'servers';

  get #name(): string {
    return param('name') ?? '';
  }

  get #tab(): Tab {
    const tab = param('tab');
    return TABS.find((t) => t === tab) ?? 'tools';
  }

  protected load(): Promise<ServerDetail> {
    if (this.#name === '') return Promise.reject(new ApiError(404, 'no server named'));
    return get<ServerDetail>(`/api/dashboard/servers/${encodeURIComponent(this.#name)}`);
  }

  protected heading(server: ServerDetail | undefined): Heading {
    return {
      title: server?.name ?? (this.#name === '' ? 'Server' : this.#name),
      mono: true,
      crumbs: [['Servers', '/servers/']],
      ...(server ? { sub: server.description } : {}),
      ...(server && this.session?.admin ? { actions: this.#actions(server) } : {}),
    };
  }

  /** What an administrator can do to the whole server. */
  #actions(s: ServerDetail): TemplateResult {
    return html`
      ${toggle(
        s.enabled,
        `Enabled: ${s.name}`,
        (on) =>
          void this.change(`server`, () => setServer(s.name, on), `Couldn’t ${on ? 'enable' : 'disable'} ${s.name}`),
        this.busy.has('server'),
      )}
      <button type="button" class="btn danger" @click=${() => void this.#delete(s)}>
        ${icon('trash')}Delete
      </button>
    `;
  }

  async #delete(s: ServerDetail): Promise<void> {
    const holds = `${count(s.counts.tools, 'tool')}, ${count(s.counts.prompts, 'prompt')} and ${count(s.counts.resources, 'resource')}`;
    const deleted = await confirm({
      title: `Delete ${s.name}?`,
      body: html`Its ${holds} leave the gateway at once, and its stored credentials are removed.
        Grants and tool groups that name <span class="mono">${s.name}</span> are kept and apply
        again if a server of that name is registered.`,
      action: 'Delete server',
      working: 'Deleting…',
      failed: 'Couldn’t delete',
      run: () => deleteServer(s.name),
    });
    if (deleted) {
      flash(`Server ${s.name} deleted.`);
      location.assign('/servers/');
    }
  }

  protected content(s: ServerDetail): TemplateResult {
    const reach = s.config_summary;
    const names = (keys: string[] | null | undefined = []) =>
      keys === null || keys.length === 0
        ? none('None')
        : html`<div class="tags">${keys.map((k) => badge(k, '', undefined, true))}</div>`;
    return html`
      <div class="row">
        ${statusBadge(s.enabled)} ${transportBadge(s.transport)} ${badge(s.session_mode)}
        ${oauthBadge(s.oauth.status)}
      </div>
      <div class="grid g2 top">
        <section class="panel">
          <div class="panel-h"><h2>Summary</h2></div>
          <div class="panel-b">
            <dl class="kv">
              <dt>Description</dt>
              <dd>${s.description === '' ? none('None') : s.description}</dd>
              <dt>Transport</dt>
              <dd>${transportBadge(s.transport)}</dd>
              <dt>Session mode</dt>
              <dd>${s.session_mode}</dd>
              <dt>Status</dt>
              <dd>${statusBadge(s.enabled)}</dd>
              <dt>Authorization</dt>
              <dd>${oauthBadge(s.oauth.status)}</dd>
              <dt>Tools</dt>
              <dd>${s.counts.tools_enabled} of ${s.counts.tools} enabled</dd>
              <dt>Last error</dt>
              <dd>${s.last_error == null ? none('None') : errorText(s.last_error)}</dd>
            </dl>
          </div>
        </section>
        <section class="panel">
          <div class="panel-h"><h2>How it is reached</h2></div>
          <div class="panel-b">
            ${reach.url != null
              ? html`<dl class="kv">
                    <dt>URL</dt>
                    <dd class="nowrap">${monoBox(reach.url, 'Copy URL')}</dd>
                    <dt>Headers</dt>
                    <dd>${names(reach.header_keys)}</dd>
                  </dl>
                  <p class="hint with-icon">
                    ${icon('lock')}Header names only. Values are never shown.
                  </p>`
              : html`<dl class="kv">
                    <dt>Command</dt>
                    <dd class="nowrap">${monoBox(reach.command ?? '', 'Copy command')}</dd>
                    <dt>Arguments</dt>
                    <dd>${names(reach.args)}</dd>
                    <dt>Environment</dt>
                    <dd>${names(reach.env_keys)}</dd>
                  </dl>
                  <p class="hint with-icon">
                    ${icon('lock')}Variable names only. Values are never sent to the browser.
                  </p>`}
          </div>
        </section>
      </div>
      <nav class="tabs" aria-label="Capabilities">
        ${this.#tabLink('tools', 'Tools', s.tools.length)}
        ${this.#tabLink('prompts', 'Prompts', s.prompts.length)}
        ${this.#tabLink('resources', 'Resources', s.resources.length)}
      </nav>
      <section class="panel">${this.#entries(s)}</section>
    `;
  }

  #tabLink(tab: Tab, label: string, n: number): TemplateResult {
    const href = withParams({ tab: tab === 'tools' ? undefined : tab });
    return html`<a class="tab" href=${href} aria-current=${this.#tab === tab ? 'page' : nothing}
      >${label} ${badge(n)}</a
    >`;
  }

  #entries(s: ServerDetail): TemplateResult {
    const tab = this.#tab;
    const rows =
      tab === 'tools'
        ? s.tools.map((t) => ({ name: t.name, shown: t.name, description: t.description, enabled: t.enabled }))
        : tab === 'prompts'
          ? s.prompts.map((p) => ({ name: p.name, shown: p.name, description: p.description, enabled: p.enabled }))
          : s.resources.map((r) => ({
              name: r.uri,
              shown: middle(r.uri),
              description: r.name === '' ? r.description : r.name,
              enabled: r.enabled,
            }));
    const label = tab === 'tools' ? 'Tool' : tab === 'prompts' ? 'Prompt' : 'Resource';
    const kind: EntityKind = tab;
    const admin = this.session?.admin === true;
    if (rows.length === 0) {
      return state(
        tab,
        `No ${tab}`,
        tab === 'tools'
          ? 'This server exposes no tools you can use.'
          : `This server exposes no ${tab}, or you were granted only some of its tools.`,
      );
    }
    return html`<div class="tblw">
      <table class="tbl stack">
        <thead>
          <tr>
            <th scope="col">${label}</th>
            <th scope="col" class="end">${admin ? 'Enabled' : 'Status'}</th>
          </tr>
        </thead>
        <tbody>
          ${rows.map(
            (r) =>
              html`<tr>
                <td data-label=${label} class="lead">${nameCell(r.name, r.description, undefined, r.shown)}</td>
                <td data-label=${admin ? 'Enabled' : 'Status'} class="act">
                  ${admin
                    ? toggle(
                        r.enabled,
                        `Enabled: ${r.name}`,
                        (on) =>
                          void this.change(
                            r.name,
                            () => setEntity(kind, r.name, on),
                            `Couldn’t ${on ? 'enable' : 'disable'} ${r.name}`,
                          ),
                        this.busy.has(r.name),
                      )
                    : statusBadge(r.enabled)}
                </td>
              </tr>`,
          )}
        </tbody>
      </table>
    </div>`;
  }
}

function monoBox(value: string, label: string): TemplateResult {
  return html`<div class="mono-box grow">
    <span class="v" title=${value}>${value}</span>
    <trunks-copy always value=${value} label=${label}></trunks-copy>
  </div>`;
}

customElements.define('trunks-server', TrunksServer);
