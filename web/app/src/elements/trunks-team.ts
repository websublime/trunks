// <trunks-team>: one team and its members. Administrators and the team's maintainers change
// the membership; an administrator deletes the team.

import '@websublime/line-components/button';
import { html, nothing, type TemplateResult } from 'lit';

import { ApiError, get } from '../api/client';
import type { TeamDetail, TeamMember, User } from '../api/types.gen';
import { identityNameProblem } from '../lib/access';
import { flash } from '../lib/flash';
import { deleteTeam, removeMember, setMember } from '../lib/manage';
import type { Session } from '../lib/session';
import { param } from '../lib/text';
import { DataView, type Heading } from './data-view';
import { confirm } from './dialog';
import { icon } from './icon';
import { badge, state, textField } from './parts';

interface Loaded {
  team: TeamDetail;
  /** Everyone who could be added; only an administrator can list them. */
  users: string[];
}

const ROLES = ['member', 'maintainer'];

class TrunksTeam extends DataView<Loaded> {
  static override properties = {
    ...DataView.properties,
    username: { state: true },
    role: { state: true },
    problem: { state: true },
  };

  declare username: string;
  declare role: string;
  declare problem: string | undefined;

  protected readonly subject = 'teams';
  protected override readonly enterprise = true;

  constructor() {
    super();
    this.username = '';
    this.role = 'member';
  }

  get #name(): string {
    return param('name') ?? '';
  }

  protected async load(s: Session): Promise<Loaded> {
    if (this.#name === '') throw new ApiError(404, 'no team named');
    const [team, users] = await Promise.all([
      get<TeamDetail>(`/api/v0/teams/${encodeURIComponent(this.#name)}`),
      s.admin ? get<User[]>('/api/v0/users') : undefined,
    ]);
    return { team, users: users?.map((u) => u.username) ?? [] };
  }

  #manages(team: TeamDetail, s: Session): boolean {
    return s.admin || (s.user?.teams.some((t) => t.name === team.name && t.role === 'maintainer') ?? false);
  }

  protected heading(data: Loaded | undefined, s: Session): Heading {
    return {
      title: data?.team.name ?? (this.#name === '' ? 'Team' : this.#name),
      mono: true,
      crumbs: [['Teams', '/teams/']],
      ...(data ? { sub: data.team.description } : {}),
      ...(data && s.admin
        ? {
            actions: html`<button type="button" class="btn danger" @click=${() => void this.#delete(data.team)}>
              ${icon('trash')}Delete
            </button>`,
          }
        : {}),
    };
  }

  async #delete(team: TeamDetail): Promise<void> {
    const deleted = await confirm({
      title: `Delete ${team.name}?`,
      body: 'Its memberships and grants are removed, and so are the MCP clients it owns: their tokens stop working at once. Its members keep what they were granted directly.',
      action: 'Delete team',
      working: 'Deleting…',
      failed: 'Couldn’t delete',
      run: () => deleteTeam(team.name),
    });
    if (deleted) {
      flash(`Team ${team.name} deleted.`);
      location.assign('/teams/');
    }
  }

  async #remove(team: TeamDetail, m: TeamMember): Promise<void> {
    const removed = await confirm({
      title: `Remove ${m.username} from ${team.name}?`,
      body: 'They lose what they could use only through this team, at once. What they were granted directly stays.',
      action: 'Remove',
      working: 'Removing…',
      failed: 'Couldn’t remove',
      run: () => removeMember(team.name, m.username),
    });
    if (removed) this.done(`${m.username} removed from ${team.name}.`);
  }

  async #add(event: SubmitEvent, team: TeamDetail): Promise<void> {
    event.preventDefault();
    const username = this.username.trim();
    this.problem =
      identityNameProblem(username) ??
      (team.members.some((m) => m.username === username) ? `${username} is already a member.` : undefined);
    if (this.problem !== undefined) {
      await this.updateComplete;
      this.querySelector<HTMLElement>('#m-user')?.focus();
      return;
    }
    const role = this.role;
    await this.change(
      'add',
      async () => {
        await setMember(team.name, username, role);
        this.username = '';
        this.notice = { tone: 'success', title: `${username} added to ${team.name} as ${role}.` };
      },
      `Couldn’t add ${username}`,
    );
  }

  protected content({ team, users }: Loaded, s: Session): TemplateResult {
    const manages = this.#manages(team, s);
    const candidates = users.filter((u) => !team.members.some((m) => m.username === u));
    return html`<div class="split">
      <section class="panel" aria-labelledby="tm-h">
        <div class="panel-h">
          <h2 id="tm-h">Members</h2>
          ${badge(team.members.length)}
        </div>
        ${team.members.length === 0
          ? state('teams', 'No members yet', manages ? 'Add the first one below.' : 'This team has nobody in it.')
          : html`<div class="tblw">
              <table class="tbl stack">
                <thead>
                  <tr>
                    <th scope="col">Member</th>
                    <th scope="col">Role</th>
                    ${manages ? html`<th scope="col"><span class="vh">Actions</span></th>` : nothing}
                  </tr>
                </thead>
                <tbody>
                  ${team.members.map(
                    (m) =>
                      html`<tr>
                        <td data-label="Member" class="lead">
                          <span class="subj"
                            >${icon('user')}<span class="name-t">${m.username}</span>${m.username ===
                            s.user?.username
                              ? badge('you')
                              : nothing}</span
                          >
                        </td>
                        <td data-label="Role">
                          ${manages
                            ? html`<div class="inp w-role">
                                <select
                                  aria-label="Role of ${m.username}"
                                  ?disabled=${this.busy.has(`role ${m.username}`)}
                                  @change=${(e: Event) => {
                                    const role = (e.target as HTMLSelectElement).value;
                                    void this.change(
                                      `role ${m.username}`,
                                      () => setMember(team.name, m.username, role),
                                      `Couldn’t change the role of ${m.username}`,
                                    );
                                  }}
                                >
                                  ${ROLES.map((r) => html`<option value=${r} ?selected=${r === m.role}>${r}</option>`)}
                                </select>
                              </div>`
                            : badge(m.role)}
                        </td>
                        ${manages
                          ? html`<td data-label="Actions" class="act">
                              <button type="button" class="btn danger" @click=${() => void this.#remove(team, m)}>
                                Remove<span class="vh"> ${m.username}</span>
                              </button>
                            </td>`
                          : nothing}
                      </tr>`,
                  )}
                </tbody>
              </table>
            </div>`}
        ${manages
          ? html`<form class="row bottom add" novalidate @submit=${(e: SubmitEvent) => void this.#add(e, team)}>
              <div class="grow w-field">
                ${textField({
                  id: 'm-user',
                  label: 'User',
                  value: this.username,
                  input: (v) => (this.username = v),
                  problem: this.problem,
                  mono: true,
                  placeholder: 'Username',
                  ...(candidates.length > 0 ? { list: 'm-users' } : {}),
                })}
                <datalist id="m-users">${candidates.map((u) => html`<option value=${u}></option>`)}</datalist>
              </div>
              <div class="fld">
                <label for="m-role">Role</label>
                <div class="inp w-role">
                  <select id="m-role" @change=${(e: Event) => (this.role = (e.target as HTMLSelectElement).value)}>
                    ${ROLES.map((r) => html`<option value=${r} ?selected=${r === this.role}>${r}</option>`)}
                  </select>
                </div>
              </div>
              <line-button type="submit" ?loading=${this.busy.has('add')}><span slot="prefix" class="pfx">${icon('plus')}</span>Add member</line-button>
            </form>`
          : nothing}
      </section>
      <section class="panel">
        <div class="panel-h"><h2>Team</h2></div>
        <div class="panel-b col roomy">
          <dl class="kv">
            <dt>Name</dt>
            <dd class="mono">${team.name}</dd>
            <dt>Description</dt>
            <dd>${team.description === '' ? html`<span class="lo">None</span>` : team.description}</dd>
          </dl>
          <p class="hint">
            A member uses what the team was granted. A maintainer also adds and removes members and manages
            the team’s MCP clients.
          </p>
        </div>
      </section>
    </div>`;
  }
}

customElements.define('trunks-team', TrunksTeam);
