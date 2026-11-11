// <trunks-access>: who may use which servers and tool groups, for administrators. A grant
// is the only thing that gives a user or a team anything.

import '@websublime/line-components/button';
import { html, nothing, type TemplateResult } from 'lit';

import { get } from '../api/client';
import type { GrantSpec, GroupSummary, ServerSummary, Team, User } from '../api/types.gen';
import { byHolder, targetLabel } from '../lib/access';
import { addGrant, revokeGrant } from '../lib/manage';
import { DataView, type Heading } from './data-view';
import { confirm } from './dialog';
import { icon } from './icon';
import { badge, state, subject } from './parts';

interface Loaded {
  grants: GrantSpec[];
  users: string[];
  teams: string[];
  servers: string[];
  groups: string[];
}

type Kind = 'server' | 'tool_group' | 'all';

class TrunksAccess extends DataView<Loaded> {
  static override properties = {
    ...DataView.properties,
    who: { state: true },
    kind: { state: true },
    target: { state: true },
    problem: { state: true },
  };

  /** `user:<name>` or `team:<name>`. */
  declare who: string;
  declare kind: Kind;
  declare target: string;
  declare problem: string | undefined;

  protected readonly subject = 'grants';
  protected override readonly enterprise = true;

  constructor() {
    super();
    this.who = '';
    this.kind = 'server';
    this.target = '';
  }

  protected async load(): Promise<Loaded> {
    const [grants, users, teams, servers, groups] = await Promise.all([
      get<GrantSpec[]>('/api/v0/grants'),
      get<User[]>('/api/v0/users'),
      get<Team[]>('/api/v0/teams'),
      get<ServerSummary[]>('/api/dashboard/servers'),
      get<GroupSummary[]>('/api/dashboard/tool-groups'),
    ]);
    return {
      grants: byHolder(grants),
      // an administrator already uses everything: a grant to one changes nothing
      users: users.filter((u) => u.role !== 'admin').map((u) => u.username),
      teams: teams.map((t) => t.name),
      servers: servers.map((s) => s.name),
      groups: groups.map((g) => g.name),
    };
  }

  protected heading(): Heading {
    return { title: 'Access', sub: 'Who can use which servers and tool groups.' };
  }

  async #add(event: SubmitEvent, data: Loaded): Promise<void> {
    event.preventDefault();
    const [subjectKind, ...rest] = this.who.split(':');
    const subjectName = rest.join(':');
    const options = this.kind === 'server' ? data.servers : this.kind === 'tool_group' ? data.groups : ['*'];
    const target = this.kind === 'all' ? '*' : options.includes(this.target) ? this.target : (options[0] ?? '');
    this.problem =
      subjectName === '' || subjectKind === undefined
        ? 'Choose a user or a team.'
        : target === ''
          ? this.kind === 'server'
            ? 'There is no server to grant yet.'
            : 'There is no tool group to grant yet.'
          : undefined;
    if (this.problem !== undefined) {
      await this.updateComplete;
      this.querySelector<HTMLElement>(subjectName === '' ? '#g-who' : '#g-target')?.focus();
      return;
    }
    const grant: GrantSpec = {
      subject_kind: subjectKind ?? '',
      subject: subjectName,
      target_kind: this.kind === 'tool_group' ? 'tool_group' : 'server',
      target,
    };
    const exists = data.grants.some(
      (g) =>
        g.subject_kind === grant.subject_kind &&
        g.subject === grant.subject &&
        g.target_kind === grant.target_kind &&
        g.target === grant.target,
    );
    if (exists) {
      this.problem = `${grant.subject_kind === 'team' ? 'Team' : 'User'} ${grant.subject} already has that grant.`;
      return;
    }
    await this.change(
      'add',
      async () => {
        await addGrant(grant);
        this.notice = {
          tone: 'success',
          title: `${grant.subject_kind === 'team' ? 'Team' : 'User'} ${grant.subject} can now use ${targetLabel(grant)}.`,
        };
      },
      'Couldn’t add the grant',
    );
  }

  async #revoke(g: GrantSpec): Promise<void> {
    const holder = `${g.subject_kind === 'team' ? 'Team' : 'User'} ${g.subject}`;
    const revoked = await confirm({
      title: 'Revoke access?',
      body: html`${holder} will lose ${targetLabel(g)}, unless another grant gives it.
      ${g.subject_kind === 'team'
        ? 'MCP clients owned by the team stop seeing its tools on their next request.'
        : 'MCP clients they own stop seeing its tools on their next request.'}`,
      action: 'Revoke',
      working: 'Revoking…',
      failed: 'Couldn’t revoke',
      run: () => revokeGrant(g),
    });
    if (revoked) this.done(`${holder} no longer has ${targetLabel(g)} by that grant.`);
  }

  protected content(data: Loaded): TemplateResult {
    const options = this.kind === 'server' ? data.servers : this.kind === 'tool_group' ? data.groups : [];
    const kinds: [Kind, string][] = [
      ['server', 'Server'],
      ['tool_group', 'Tool group'],
      ['all', 'All servers'],
    ];
    return html`
      <section class="panel" aria-labelledby="ag-h">
        <div class="panel-h"><h2 id="ag-h">Add a grant</h2></div>
        <div class="panel-b col">
          <form class="row bottom wide-gap" novalidate @submit=${(e: SubmitEvent) => void this.#add(e, data)}>
            <div class="fld grow w-field">
              <label for="g-who">User or team</label>
              <div class="inp mono">
                <select id="g-who" @change=${(e: Event) => (this.who = (e.target as HTMLSelectElement).value)}>
                  <option value="" ?selected=${this.who === ''}>Choose…</option>
                  <optgroup label="Teams">
                    ${data.teams.map(
                      (t) => html`<option value="team:${t}" ?selected=${this.who === `team:${t}`}>${t}</option>`,
                    )}
                  </optgroup>
                  <optgroup label="Users">
                    ${data.users.map(
                      (u) => html`<option value="user:${u}" ?selected=${this.who === `user:${u}`}>${u}</option>`,
                    )}
                  </optgroup>
                </select>
              </div>
            </div>
            <div class="fld">
              <span class="lb" id="g-k">Can use</span>
              <div class="bgrp" role="group" aria-labelledby="g-k">
                ${kinds.map(
                  ([kind, label]) =>
                    html`<button
                      type="button"
                      class="btn"
                      aria-pressed=${this.kind === kind ? 'true' : 'false'}
                      @click=${() => {
                        this.kind = kind;
                        this.target = '';
                        this.problem = undefined;
                      }}
                    >
                      ${label}
                    </button>`,
                )}
              </div>
            </div>
            ${this.kind === 'all'
              ? nothing
              : html`<div class="fld grow w-field">
                  <label for="g-target">${this.kind === 'server' ? 'Server' : 'Tool group'}</label>
                  <div class="inp mono">
                    <select id="g-target" @change=${(e: Event) => (this.target = (e.target as HTMLSelectElement).value)}>
                      ${options.map((o) => html`<option value=${o} ?selected=${o === this.target}>${o}</option>`)}
                    </select>
                  </div>
                </div>`}
            <line-button variant="primary" type="submit" ?loading=${this.busy.has('add')}
              ><span slot="prefix" class="pfx">${icon('plus')}</span>Add grant</line-button
            >
          </form>
          ${this.problem === undefined
            ? nothing
            : html`<div class="ferr" role="alert">${icon('alert')}${this.problem}</div>`}
          <p class="hint">
            ${this.kind === 'all'
              ? 'Every server, including the ones registered later.'
              : this.kind === 'server'
                ? 'Every tool, prompt and resource of the server.'
                : 'Only the group’s effective tools, without access to the servers behind them.'}
            Administrators already use everything and are not listed.
          </p>
        </div>
      </section>
      <section class="panel" aria-labelledby="gl-h">
        <div class="panel-h">
          <h2 id="gl-h">Grants</h2>
          ${badge(data.grants.length)}
        </div>
        ${data.grants.length === 0
          ? state('lock', 'No grants yet', 'Until one is added, only administrators can use the gateway.')
          : html`<div class="tblw">
              <table class="tbl stack">
                <thead>
                  <tr>
                    <th scope="col">User or team</th>
                    <th scope="col">Can use</th>
                    <th scope="col"><span class="vh">Actions</span></th>
                  </tr>
                </thead>
                <tbody>
                  ${data.grants.map(
                    (g) =>
                      html`<tr>
                        <td data-label="User or team" class="lead">${subject(g.subject_kind, g.subject)}</td>
                        <td data-label="Can use">
                          <span class="subj"
                            >${icon('arrowRight', 'arrow')}${icon(g.target_kind === 'server' ? 'servers' : 'groups')}${g.target ===
                            '*'
                              ? html`<strong>All servers</strong>`
                              : html`<span class="lo small">${g.target_kind === 'server' ? 'server' : 'tool group'}</span
                                  ><span class="mono">${g.target}</span>`}</span
                          >
                        </td>
                        <td data-label="Actions" class="act">
                          <button type="button" class="btn danger" @click=${() => void this.#revoke(g)}>
                            Revoke<span class="vh"> ${targetLabel(g)} from ${g.subject_kind} ${g.subject}</span>
                          </button>
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

customElements.define('trunks-access', TrunksAccess);
