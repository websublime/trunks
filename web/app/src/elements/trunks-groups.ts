// <trunks-groups>: the tool groups this viewer may use.

import { html, type TemplateResult } from 'lit';

import { get } from '../api/client';
import type { GroupSummary } from '../api/types.gen';
import { count } from '../lib/text';
import { DataView, type Heading } from './data-view';
import { icon } from './icon';
import { badge, nameCell, state } from './parts';

const create = () => html`<a class="btn primary" href="/groups/new/">${icon('plus')}New group</a>`;

class TrunksGroups extends DataView<GroupSummary[]> {
  protected readonly subject = 'tool groups';

  protected load(): Promise<GroupSummary[]> {
    return get<GroupSummary[]>('/api/dashboard/tool-groups');
  }

  protected heading(groups: GroupSummary[] | undefined): Heading {
    return {
      title: 'Tool groups',
      sub:
        groups === undefined
          ? 'Named selections of tools, each with its own endpoint.'
          : `${count(groups.length, 'tool group')}. Each has its own MCP endpoint.`,
      ...(this.session?.admin ? { actions: create() } : {}),
    };
  }

  protected content(groups: GroupSummary[]): TemplateResult {
    if (groups.length === 0) {
      return html`<section class="panel">
        ${state(
          'groups',
          'No tool groups yet',
          this.session?.admin
            ? 'Create one to give a client a curated subset of tools.'
            : 'You have not been granted any tool group.',
          this.session?.admin ? create() : undefined,
        )}
      </section>`;
    }
    return html`<section class="panel" aria-labelledby="tg-h">
      <h2 id="tg-h" class="vh">Tool groups</h2>
      <div class="tblw">
        <table class="tbl stack">
          <thead>
            <tr>
              <th scope="col" class="w-name">Name</th>
              <th scope="col" class="num">Effective tools</th>
              <th scope="col" class="num">Servers</th>
              <th scope="col" class="num">Included tools</th>
              <th scope="col" class="num">Excluded tools</th>
            </tr>
          </thead>
          <tbody>
            ${groups.map(
              (g) =>
                html`<tr>
                  <td data-label="Name" class="lead">
                    ${nameCell(g.name, g.description, `/groups/detail/?name=${encodeURIComponent(g.name)}`)}
                  </td>
                  <td data-label="Effective tools" class="num">
                    ${g.degraded
                      ? badge('0 · no effective tools', 'warning', 'warning')
                      : g.effective_tools.length}
                  </td>
                  <td data-label="Servers" class="num">${g.included_servers.length}</td>
                  <td data-label="Included tools" class="num">${g.included_tools.length}</td>
                  <td data-label="Excluded tools" class="num">${g.excluded_tools.length}</td>
                </tr>`,
            )}
          </tbody>
        </table>
      </div>
    </section>`;
  }
}

customElements.define('trunks-groups', TrunksGroups);
