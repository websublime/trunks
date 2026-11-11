// <trunks-account>: who the person is signed in as, what they can use and through which
// grant.

import { html, nothing, type TemplateResult } from 'lit';

import { get, post } from '../api/client';
import type { GroupSummary, ServerSummary, WhoAmI } from '../api/types.gen';
import { groupsReaching, viaForGroup, viaForServer } from '../lib/access';
import type { Session } from '../lib/session';
import { DataView, type Heading } from './data-view';
import { icon } from './icon';
import { badge, none, state } from './parts';

interface Loaded {
  servers: ServerSummary[];
  groups: GroupSummary[];
}

class TrunksAccount extends DataView<Loaded> {
  protected readonly subject = 'your account';
  protected override readonly enterprise = true;

  protected async load(): Promise<Loaded> {
    const [servers, groups] = await Promise.all([
      get<ServerSummary[]>('/api/dashboard/servers'),
      get<GroupSummary[]>('/api/dashboard/tool-groups'),
    ]);
    return { servers, groups };
  }

  protected heading(): Heading {
    return { title: 'Account', sub: 'Who you are signed in as, and what you can use.' };
  }

  async #signOut(): Promise<void> {
    await post('/auth/logout').catch(() => undefined);
    location.assign('/login/');
  }

  protected content({ servers, groups }: Loaded, s: Session): TemplateResult {
    const me = s.user;
    if (me === undefined) return html``;
    const admin = me.role === 'admin';
    return html`<div class="split">
      <div class="col roomy">
        <section class="panel" aria-labelledby="as-h">
          <div class="panel-h">
            <h2 id="as-h">Servers you can use</h2>
            ${admin || me.access.all ? badge('All servers', 'accent') : badge(servers.length)}
          </div>
          ${servers.length === 0
            ? state(
                'servers',
                admin ? 'No servers yet' : 'No servers',
                admin ? 'None is registered.' : 'You have not been granted any server. Ask an administrator.',
              )
            : html`<ul class="list">
                ${servers.map(
                  (server) =>
                    html`<li>
                      ${icon('servers')}
                      <a class="name" href="/servers/detail/?name=${encodeURIComponent(server.name)}">${server.name}</a>
                      <span class="grow"></span>
                      <span class="small lo">${serverVia(server.name, me, groups)}</span>
                    </li>`,
                )}
              </ul>`}
        </section>
        <section class="panel" aria-labelledby="ag-h">
          <div class="panel-h">
            <h2 id="ag-h">Tool groups you can use</h2>
            ${badge(groups.length)}
          </div>
          ${groups.length === 0
            ? state('groups', 'No tool groups', admin ? 'None exists.' : 'You have not been granted any tool group.')
            : html`<ul class="list">
                ${groups.map((group) => {
                  const via = viaForGroup(group.name, me.grants, me.username);
                  return html`<li>
                    ${icon('groups')}
                    <a class="name" href="/groups/detail/?name=${encodeURIComponent(group.name)}">${group.name}</a>
                    <span class="grow"></span>
                    <span class="small lo"
                      >${admin ? 'as administrator' : via.length > 0 ? `via ${via.join(', ')}` : nothing}</span
                    >
                  </li>`;
                })}
              </ul>`}
        </section>
      </div>
      <section class="panel">
        <div class="panel-h"><h2>You</h2></div>
        <div class="panel-b col roomy">
          <dl class="kv">
            <dt>Username</dt>
            <dd class="mono">${me.username}</dd>
            <dt>Role</dt>
            <dd>${admin ? badge('admin', 'accent', 'shield') : badge(me.role)}</dd>
            <dt>Teams</dt>
            <dd>
              ${me.teams.length === 0
                ? none('None')
                : html`<div class="tags">
                    ${me.teams.map(
                      (t) =>
                        html`<a class="bdg link" href="/teams/detail/?name=${encodeURIComponent(t.name)}"
                          ><span class="mono hi">${t.name}</span>· ${t.role}</a
                        >`,
                    )}
                  </div>`}
            </dd>
          </dl>
          <div>
            <button type="button" class="btn" @click=${() => void this.#signOut()}>${icon('signOut')}Sign out</button>
          </div>
        </div>
      </section>
    </div>`;
  }
}

/** Why this person can use the server: the grants, or the tool groups that reach into it. */
function serverVia(server: string, me: WhoAmI, groups: GroupSummary[]): string {
  if (me.role === 'admin') return 'as administrator';
  const via = viaForServer(server, me.grants, me.username);
  if (via.length > 0) return `via ${via.join(', ')}`;
  const through = groupsReaching(server, groups);
  return through.length > 0 ? `some tools, via tool group ${through.join(', ')}` : '';
}

customElements.define('trunks-account', TrunksAccount);
