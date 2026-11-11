// <trunks-clients>: the MCP clients this person may see — their own and their teams'; all
// of them for an administrator.

import { html, nothing, type TemplateResult } from 'lit';

import { get } from '../api/client';
import type { McpClient } from '../api/types.gen';
import { deleteClient, rotateClientToken } from '../lib/manage';
import type { Session } from '../lib/session';
import { count } from '../lib/text';
import { DataView, type Heading } from './data-view';
import { confirm, showToken } from './dialog';
import { icon } from './icon';
import { badge, nameCell, none, state, subject } from './parts';

/** How to give a client its token, wherever one is shown. */
export const CLIENT_TOKEN_USE =
  'Configure it in the MCP client application as the bearer token for this gateway’s MCP endpoint.';

const create = () => html`<a class="btn primary" href="/clients/new/">${icon('plus')}New client</a>`;

class TrunksClients extends DataView<McpClient[]> {
  protected readonly subject = 'MCP clients';
  protected override readonly enterprise = true;

  protected load(): Promise<McpClient[]> {
    return get<McpClient[]>('/api/v0/clients');
  }

  protected heading(clients: McpClient[] | undefined, s: Session): Heading {
    return {
      title: 'MCP clients',
      sub:
        clients === undefined || clients.length === 0
          ? 'Agents and applications that connect to this gateway with their own access token.'
          : `${count(clients.length, 'client')}${s.admin ? '' : ': yours and your teams’'}. Each connects with its own access token.`,
      ...(s.mode === 'enterprise' ? { actions: create() } : {}),
    };
  }

  /** The API lets the owner, a maintainer of the owning team and administrators change a client. */
  #mine(c: McpClient, s: Session): boolean {
    if (s.admin) return true;
    if (c.owner == null || s.user === undefined) return false;
    if (c.owner.kind === 'user') return c.owner.name === s.user.username;
    return s.user.teams.some((t) => t.name === c.owner?.name && t.role === 'maintainer');
  }

  async #rotate(c: McpClient): Promise<void> {
    let token = '';
    const rotated = await confirm({
      title: `Replace the token of ${c.name}?`,
      body: 'The current token stops working at once. The application using it loses the gateway until it is given the new one.',
      action: 'Replace token',
      working: 'Replacing…',
      failed: 'Couldn’t replace the token',
      run: async () => {
        token = (await rotateClientToken(c.name)).access_token ?? '';
      },
    });
    if (!rotated) return;
    await showToken(c.name, token, CLIENT_TOKEN_USE);
    this.done(`${c.name} has a new token.`);
  }

  async #delete(c: McpClient): Promise<void> {
    const deleted = await confirm({
      title: `Delete ${c.name}?`,
      body: 'Its token stops working at once and cannot be restored. The application using it loses the gateway.',
      action: 'Delete client',
      working: 'Deleting…',
      failed: 'Couldn’t delete',
      run: () => deleteClient(c.name),
    });
    if (deleted) this.done(`MCP client ${c.name} deleted.`);
  }

  protected content(clients: McpClient[], s: Session): TemplateResult {
    if (clients.length === 0) {
      return html`<section class="panel">
        ${state(
          'clients',
          'No MCP clients yet',
          'Create one for each agent or application, so that a token can be replaced or removed without affecting the others.',
          create(),
        )}
      </section>`;
    }
    return html`<section class="panel" aria-labelledby="cl-h">
      <h2 id="cl-h" class="vh">MCP clients</h2>
      <div class="tblw">
        <table class="tbl stack">
          <thead>
            <tr>
              <th scope="col" class="w-name">Name</th>
              <th scope="col">Owner</th>
              <th scope="col">Can reach</th>
              <th scope="col"><span class="vh">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            ${clients.map(
              (c) =>
                html`<tr>
                  <td data-label="Name" class="lead">${nameCell(c.name, c.description)}</td>
                  <td data-label="Owner">${c.owner == null ? none('None') : subject(c.owner.kind, c.owner.name)}</td>
                  <td data-label="Can reach">${reach(c)}</td>
                  <td data-label="Actions" class="act">
                    ${this.#mine(c, s)
                      ? html`<div class="row end">
                          <button type="button" class="btn" @click=${() => void this.#rotate(c)}>
                            ${icon('rotate')}New token<span class="vh"> for ${c.name}</span>
                          </button>
                          <button type="button" class="btn danger" @click=${() => void this.#delete(c)}>
                            Delete<span class="vh"> ${c.name}</span>
                          </button>
                        </div>`
                      : nothing}
                  </td>
                </tr>`,
            )}
          </tbody>
        </table>
      </div>
    </section>`;
  }
}

/** What the allow list means, which depends on whether the client has an owner. */
function reach(c: McpClient): TemplateResult {
  const everything = c.allow_list.length === 0 || c.allow_list.includes('*');
  if (c.owner != null && everything) return html`<span>Everything its owner can use</span>`;
  if (c.owner == null && c.allow_list.includes('*')) return html`<strong>All servers</strong>`;
  if (c.allow_list.length === 0) return none('Nothing');
  return html`<div class="tags">${c.allow_list.map((name) => badge(name, '', undefined, true))}</div>`;
}

customElements.define('trunks-clients', TrunksClients);
