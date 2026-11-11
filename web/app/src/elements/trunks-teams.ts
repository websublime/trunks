// <trunks-teams>: the teams and who is in each. Everyone signed in can read them; an
// administrator creates them.

import '@websublime/line-components/button';
import { html, nothing, type TemplateResult } from 'lit';

import { ApiError, get } from '../api/client';
import type { Team, TeamDetail } from '../api/types.gen';
import { identityNameProblem } from '../lib/access';
import { createTeam } from '../lib/manage';
import type { Session } from '../lib/session';
import { count } from '../lib/text';
import { DataView, type Heading } from './data-view';
import { nameCell, none, state, textField } from './parts';

class TrunksTeams extends DataView<TeamDetail[]> {
  static override properties = {
    ...DataView.properties,
    name: { state: true },
    description: { state: true },
    problem: { state: true },
    adding: { state: true },
  };

  declare name: string;
  declare description: string;
  declare problem: string | undefined;
  declare adding: boolean;

  protected readonly subject = 'teams';
  protected override readonly enterprise = true;

  constructor() {
    super();
    this.name = '';
    this.description = '';
    this.adding = false;
  }

  protected async load(): Promise<TeamDetail[]> {
    const teams = await get<Team[]>('/api/v0/teams');
    return Promise.all(teams.map((t) => get<TeamDetail>(`/api/v0/teams/${encodeURIComponent(t.name)}`)));
  }

  protected heading(teams: TeamDetail[] | undefined): Heading {
    return {
      title: 'Teams',
      sub: teams === undefined ? 'Groups of users that are granted access together.' : `${count(teams.length, 'team')}.`,
    };
  }

  async #add(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (this.adding) return;
    const name = this.name.trim();
    this.problem = identityNameProblem(name);
    if (this.problem !== undefined) {
      await this.updateComplete;
      this.querySelector<HTMLElement>('#t-name')?.focus();
      return;
    }
    this.adding = true;
    try {
      await createTeam({ name, description: this.description.trim() });
      this.name = '';
      this.description = '';
      this.done(`Team ${name} created.`);
    } catch (e) {
      if (e instanceof ApiError && e.signedOut) {
        location.reload();
        return;
      }
      const message = e instanceof Error ? e.message : 'Unknown error';
      this.problem = message.includes('already exists') ? 'A team of this name already exists.' : message;
      await this.updateComplete;
      this.querySelector<HTMLElement>('#t-name')?.focus();
    } finally {
      this.adding = false;
    }
  }

  protected content(teams: TeamDetail[], s: Session): TemplateResult {
    return html`
      ${s.admin
        ? html`<section class="panel" aria-labelledby="ta-h">
            <div class="panel-h"><h2 id="ta-h">New team</h2></div>
            <div class="panel-b">
              <form class="row bottom" novalidate @submit=${(e: SubmitEvent) => void this.#add(e)}>
                <div class="w-field">
                  ${textField({
                    id: 't-name',
                    label: 'Name',
                    value: this.name,
                    input: (v) => (this.name = v),
                    problem: this.problem,
                    mono: true,
                  })}
                </div>
                <div class="grow w-field">
                  ${textField({
                    id: 't-desc',
                    label: 'Description',
                    value: this.description,
                    input: (v) => (this.description = v),
                  })}
                </div>
                <line-button variant="primary" type="submit" ?loading=${this.adding}>Create team</line-button>
              </form>
            </div>
          </section>`
        : nothing}
      <section class="panel" aria-labelledby="tl-h">
        <h2 id="tl-h" class="vh">Teams</h2>
        ${teams.length === 0
          ? state(
              'teams',
              'No teams yet',
              s.admin
                ? 'A team lets you grant access once for several people.'
                : 'An administrator has not created any team.',
            )
          : html`<div class="tblw">
              <table class="tbl stack">
                <thead>
                  <tr>
                    <th scope="col" class="w-name">Name</th>
                    <th scope="col">Members</th>
                  </tr>
                </thead>
                <tbody>
                  ${teams.map(
                    (t) =>
                      html`<tr>
                        <td data-label="Name" class="lead">
                          ${nameCell(t.name, t.description, `/teams/detail/?name=${encodeURIComponent(t.name)}`)}
                        </td>
                        <td data-label="Members">
                          ${t.members.length === 0
                            ? none('None')
                            : html`<div class="tags">
                                ${t.members.map(
                                  (m) =>
                                    html`<span class="bdg"><span class="mono hi">${m.username}</span>· ${m.role}</span>`,
                                )}
                              </div>`}
                        </td>
                      </tr>`,
                  )}
                </tbody>
              </table>
            </div>`}
      </section>
    `;
  }
}

customElements.define('trunks-teams', TrunksTeams);
