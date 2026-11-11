// <trunks-servers>: the upstream servers this viewer may see.

import { html, type TemplateResult } from 'lit';

import { get } from '../api/client';
import type { ServerSummary } from '../api/types.gen';
import { count } from '../lib/text';
import { DataView, type Heading } from './data-view';
import { icon } from './icon';
import { errorText, nameCell, none, oauthBadge, state, statusBadge, transportBadge } from './parts';

const register = () =>
  html`<a class="btn primary" href="/servers/new/">${icon('plus')}Register server</a>`;

class TrunksServers extends DataView<ServerSummary[]> {
  protected readonly subject = 'servers';

  protected load(): Promise<ServerSummary[]> {
    return get<ServerSummary[]>('/api/dashboard/servers');
  }

  protected heading(servers: ServerSummary[] | undefined): Heading {
    const actions = this.session?.admin ? { actions: register() } : {};
    if (servers === undefined) return { title: 'Servers', ...actions };
    const enabled = servers.filter((s) => s.enabled).length;
    return {
      title: 'Servers',
      sub: `${count(servers.length, 'upstream server')}. ${enabled} enabled.`,
      ...actions,
    };
  }

  protected content(servers: ServerSummary[]): TemplateResult {
    if (servers.length === 0) {
      return html`<section class="panel">
        ${this.session?.admin
          ? state(
              'servers',
              'No servers yet',
              'Register an upstream MCP server to expose its tools through this gateway.',
              register(),
            )
          : state('servers', 'No servers yet', 'An administrator has not granted you any server.')}
      </section>`;
    }
    return html`<section class="panel" aria-labelledby="sv-h">
      <h2 id="sv-h" class="vh">Servers</h2>
      <div class="tblw">
        <table class="tbl stack">
          <thead>
            <tr>
              <th scope="col" class="w-name">Name</th>
              <th scope="col">Transport</th>
              <th scope="col">Session</th>
              <th scope="col" class="num">Tools</th>
              <th scope="col">Authorization</th>
              <th scope="col">Status</th>
              <th scope="col" class="w-error">Last error</th>
            </tr>
          </thead>
          <tbody>
            ${servers.map(
              (s) =>
                html`<tr>
                  <td data-label="Name" class="lead">
                    ${nameCell(s.name, s.description, `/servers/detail/?name=${encodeURIComponent(s.name)}`)}
                  </td>
                  <td data-label="Transport">${transportBadge(s.transport)}</td>
                  <td data-label="Session">${s.session_mode}</td>
                  <td data-label="Tools" class="num">${s.counts.tools}</td>
                  <td data-label="Authorization">${oauthBadge(s.oauth.status)}</td>
                  <td data-label="Status">${statusBadge(s.enabled)}</td>
                  <td data-label="Last error">
                    ${s.last_error == null ? none() : errorText(s.last_error)}
                  </td>
                </tr>`,
            )}
          </tbody>
        </table>
      </div>
    </section>`;
  }
}

customElements.define('trunks-servers', TrunksServers);
