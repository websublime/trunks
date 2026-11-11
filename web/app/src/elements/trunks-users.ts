// <trunks-users>: the people who can sign in, for administrators.

import '@websublime/line-components/button';
import { html, nothing, type TemplateResult } from 'lit';

import { ApiError, get } from '../api/client';
import type { User } from '../api/types.gen';
import { identityNameProblem } from '../lib/access';
import { createUser, deleteUser, rotateUserToken } from '../lib/manage';
import type { Session } from '../lib/session';
import { count } from '../lib/text';
import { DataView, type Heading } from './data-view';
import { confirm, showToken } from './dialog';
import { icon } from './icon';
import { badge, textField } from './parts';

const USER_TOKEN_USE = html`Give it to the person. They sign in to this console with it, or run
  <span class="mono">trunks login &lt;token&gt;</span>.`;

class TrunksUsers extends DataView<User[]> {
  static override properties = {
    ...DataView.properties,
    username: { state: true },
    problem: { state: true },
    adding: { state: true },
  };

  declare username: string;
  declare problem: string | undefined;
  declare adding: boolean;

  protected readonly subject = 'users';
  protected override readonly enterprise = true;

  constructor() {
    super();
    this.username = '';
    this.adding = false;
  }

  protected load(): Promise<User[]> {
    return get<User[]>('/api/v0/users');
  }

  protected heading(users: User[] | undefined): Heading {
    return {
      title: 'Users',
      sub: users === undefined ? 'The people who can sign in.' : `${count(users.length, 'user')}.`,
    };
  }

  async #add(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (this.adding) return;
    const username = this.username.trim();
    this.problem = identityNameProblem(username);
    if (this.problem !== undefined) {
      await this.updateComplete;
      this.querySelector<HTMLElement>('#u-name')?.focus();
      return;
    }
    this.adding = true;
    try {
      const created = await createUser(username);
      this.username = '';
      await showToken(created.username, created.access_token, USER_TOKEN_USE);
      this.done(`User ${created.username} added. They can use nothing until they are granted access.`);
    } catch (e) {
      if (e instanceof ApiError && e.signedOut) {
        location.reload();
        return;
      }
      const message = e instanceof Error ? e.message : 'Unknown error';
      this.problem = message.includes('already exists') ? 'A user of this name already exists.' : message;
      await this.updateComplete;
      this.querySelector<HTMLElement>('#u-name')?.focus();
    } finally {
      this.adding = false;
    }
  }

  async #rotate(u: User): Promise<void> {
    let token = '';
    const rotated = await confirm({
      title: `Replace the token of ${u.username}?`,
      body: 'The current token stops working at once, in the CLI and for signing in here. Sessions already open in a browser stay open.',
      action: 'Replace token',
      working: 'Replacing…',
      failed: 'Couldn’t replace the token',
      run: async () => {
        token = (await rotateUserToken(u.username)).access_token;
      },
    });
    if (!rotated) return;
    await showToken(u.username, token, USER_TOKEN_USE);
    this.done(`${u.username} has a new token.`);
  }

  async #delete(u: User): Promise<void> {
    const deleted = await confirm({
      title: `Delete ${u.username}?`,
      body: 'Their sessions, team memberships and grants are removed, and so are the MCP clients they own: those clients’ tokens stop working at once.',
      action: 'Delete user',
      working: 'Deleting…',
      failed: 'Couldn’t delete',
      run: () => deleteUser(u.username),
    });
    if (deleted) this.done(`User ${u.username} deleted.`);
  }

  protected content(users: User[], s: Session): TemplateResult {
    const me = s.user?.username;
    return html`
      <section class="panel" aria-labelledby="ua-h">
        <div class="panel-h"><h2 id="ua-h">Add a user</h2></div>
        <div class="panel-b">
          <form class="row bottom" novalidate @submit=${(e: SubmitEvent) => void this.#add(e)}>
            <div class="grow w-field">
              ${textField({
                id: 'u-name',
                label: 'Username',
                value: this.username,
                input: (v) => (this.username = v),
                problem: this.problem,
                mono: true,
                hint: 'Created with the role user and a new access token, shown once.',
              })}
            </div>
            <line-button class="with-hint" variant="primary" type="submit" ?loading=${this.adding}
              >Add user</line-button
            >
          </form>
        </div>
      </section>
      <section class="panel" aria-labelledby="ul-h">
        <h2 id="ul-h" class="vh">Users</h2>
        <div class="tblw">
          <table class="tbl stack">
            <thead>
              <tr>
                <th scope="col">Username</th>
                <th scope="col">Role</th>
                <th scope="col"><span class="vh">Actions</span></th>
              </tr>
            </thead>
            <tbody>
              ${users.map(
                (u) =>
                  html`<tr>
                    <td data-label="Username" class="lead">
                      <span class="subj"
                        >${icon('user')}<span class="name-t">${u.username}</span>${u.username === me
                          ? badge('you')
                          : nothing}</span
                      >
                    </td>
                    <td data-label="Role">
                      ${u.role === 'admin' ? badge('admin', 'accent', 'shield') : badge(u.role)}
                    </td>
                    <td data-label="Actions" class="act">
                      <div class="row end">
                        <button type="button" class="btn" @click=${() => void this.#rotate(u)}>
                          ${icon('rotate')}New token<span class="vh"> for ${u.username}</span>
                        </button>
                        ${u.username === me
                          ? nothing
                          : html`<button type="button" class="btn danger" @click=${() => void this.#delete(u)}>
                              Delete<span class="vh"> ${u.username}</span>
                            </button>`}
                      </div>
                    </td>
                  </tr>`,
              )}
            </tbody>
          </table>
        </div>
      </section>
    `;
  }
}

customElements.define('trunks-users', TrunksUsers);
