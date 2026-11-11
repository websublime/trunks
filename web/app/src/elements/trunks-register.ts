// <trunks-register>: the form that adds an upstream server, and the authorization that
// follows when the server asks for one. Nothing typed here is kept anywhere but in this
// page's memory, and it is gone when the page is left.

import '@websublime/line-components/button';
import { LitElement, html, nothing, type TemplateResult } from 'lit';

import { ApiError, get } from '../api/client';
import type { AuthorizationRequired, OAuthSessionStatus, ServerDetail } from '../api/types.gen';
import { flash } from '../lib/flash';
import { clock } from '../lib/format';
import { registerServer } from '../lib/manage';
import {
  definition,
  emptyDraft,
  nameProblem,
  problems,
  redirectUri,
  safeProviderUrl,
  type Draft,
  type Field,
  type Pair,
} from '../lib/register';
import type { Session } from '../lib/session';
import { count } from '../lib/text';
import { icon } from './icon';
import './parts';
import { state } from './parts';
import { session } from './trunks-shell';

type Phase = 'form' | 'authorize' | 'waiting' | 'authorized' | 'expired';

/** How often the page asks whether the authorization has finished. */
const CHECK_MS = 3000;

/** Which field of the page a problem belongs to, in the order they appear. */
const FIELD_INPUT: Record<Field, string> = {
  name: 'r-name',
  command: 'r-cmd',
  env: 'env-k0',
  url: 'r-url',
  headers: 'hd-k0',
};

class TrunksRegister extends LitElement {
  static override properties = {
    session: { state: true },
    draft: { state: true },
    found: { state: true },
    failure: { state: true },
    busy: { state: true },
    shown: { state: true },
    phase: { state: true },
    pending: { state: true },
    registered: { state: true },
  };

  declare session: Session | undefined;
  declare draft: Draft;
  /** Problems found by the last attempt to submit. */
  declare found: Partial<Record<Field, string>>;
  /** Why the server refused the registration. */
  declare failure: string | undefined;
  declare busy: boolean;
  /** Secret fields currently shown as text. */
  declare shown: ReadonlySet<string>;
  declare phase: Phase;
  declare pending: AuthorizationRequired | undefined;
  /** The server as registered once its authorization finished. */
  declare registered: ServerDetail | undefined;

  #timer: ReturnType<typeof setInterval> | undefined;

  constructor() {
    super();
    this.draft = emptyDraft();
    this.found = {};
    this.busy = false;
    this.shown = new Set();
    this.phase = 'form';
  }

  protected override createRenderRoot(): HTMLElement {
    return this;
  }

  override connectedCallback(): void {
    super.connectedCallback();
    void session().then((s) => (this.session = s));
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback();
    this.#stopChecking();
  }

  #set(changes: Partial<Draft>): void {
    this.draft = { ...this.draft, ...changes };
  }

  async #submit(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (this.busy) return;
    this.failure = undefined;
    this.found = problems(this.draft);
    const first = (Object.keys(FIELD_INPUT) as Field[]).find((f) => this.found[f] !== undefined);
    if (first !== undefined) {
      await this.updateComplete;
      this.querySelector<HTMLElement>(`#${FIELD_INPUT[first]}`)?.focus();
      return;
    }
    this.busy = true;
    const name = this.draft.name.trim();
    try {
      const result = await registerServer(definition(this.draft, location.origin));
      if (result.authorization_required != null) {
        this.pending = result.authorization_required;
        this.phase = 'authorize';
        this.#startChecking();
        await this.updateComplete;
        this.querySelector<HTMLElement>('[data-focus]')?.focus();
        return;
      }
      flash(`Server ${name} registered.`);
      location.assign(`/servers/detail/?name=${encodeURIComponent(name)}`);
    } catch (e) {
      if (e instanceof ApiError && e.signedOut) {
        location.reload();
        return;
      }
      if (e instanceof ApiError && e.status === 400 && e.message.includes('already exists')) {
        // The API offers to replace it; the console asks for a deliberate delete instead.
        this.found = { name: 'A server of this name is already registered. Choose another name, or delete that server first.' };
        await this.updateComplete;
        this.querySelector<HTMLElement>('#r-name')?.focus();
        return;
      }
      this.failure = refusal(e);
      await this.updateComplete;
      this.querySelector<HTMLElement>('#r-failure')?.focus();
    } finally {
      this.busy = false;
    }
  }

  #startChecking(): void {
    this.#stopChecking();
    this.#timer = setInterval(() => void this.#check(), CHECK_MS);
  }

  #stopChecking(): void {
    clearInterval(this.#timer);
    this.#timer = undefined;
  }

  async #check(): Promise<void> {
    const pending = this.pending;
    if (pending === undefined) return;
    let status: OAuthSessionStatus['status'] = 'pending';
    try {
      const answer = await get<OAuthSessionStatus>(
        `/api/dashboard/oauth/session/${encodeURIComponent(pending.session_id)}`,
      );
      status = answer.status;
    } catch (e) {
      if (e instanceof ApiError && e.signedOut) location.reload();
      // anything else: keep waiting, the next check may get through
    }
    if (this.pending !== pending) return;
    if (status === 'pending' && Date.now() > Date.parse(pending.expires_at)) status = 'expired';
    if (status === 'pending') return;
    this.#stopChecking();
    if (status === 'completed') {
      const name = this.draft.name.trim();
      this.registered = await get<ServerDetail>(`/api/dashboard/servers/${encodeURIComponent(name)}`).catch(
        () => undefined,
      );
      this.phase = 'authorized';
    } else {
      this.phase = 'expired';
    }
    await this.updateComplete;
    this.querySelector<HTMLElement>('[data-focus]')?.focus();
  }

  /** Back to the form, with everything that was typed. */
  #again(): void {
    this.#stopChecking();
    this.pending = undefined;
    this.phase = 'form';
  }

  protected override render(): TemplateResult | typeof nothing {
    const s = this.session;
    if (s === undefined) return nothing;
    const name = this.draft.name.trim();
    return html`
      <div class="ph">
        <div class="t">
          <nav class="crumb" aria-label="Breadcrumb">
            <a href="/servers/">Servers</a>${icon('chevronRight')}<span aria-current="page">Register</span>
          </nav>
          <h1>Register a server</h1>
          <p class="sub">
            ${this.phase === 'form'
              ? 'Add an upstream MCP server to this gateway.'
              : html`<span class="mono">${name}</span> · authorization`}
          </p>
        </div>
      </div>
      ${!s.admin
        ? html`<section class="panel">
            ${state(
              'lock',
              'You can’t register servers',
              'Only administrators add servers to the gateway.',
              html`<a class="btn" href="/servers/">Back to servers</a>`,
            )}
          </section>`
        : this.phase === 'form'
          ? this.#form()
          : this.#authorization(name)}
    `;
  }

  #authorization(name: string): TemplateResult {
    const pending = this.pending;
    const provider = pending === undefined ? undefined : safeProviderUrl(pending.authorization_url);
    const until = pending === undefined ? '' : clock(new Date(pending.expires_at));
    const open = (label: string, primary: boolean) =>
      provider === undefined
        ? nothing
        : html`<a
            class="btn ${primary ? 'primary' : ''}"
            href=${provider}
            target="_blank"
            rel="noopener noreferrer"
            @click=${() => (this.phase = 'waiting')}
            >${label}${icon('external')}</a
          >`;
    const cancel = html`<button type="button" class="btn borderless" @click=${() => this.#again()}>Cancel</button>`;
    switch (this.phase) {
      case 'authorize':
        return html`<section class="panel narrow">
          <div class="panel-b col roomy">
            <h2 class="row" tabindex="-1" data-focus>${icon('lock')}Authorization required</h2>
            ${provider === undefined
              ? html`<div class="alert danger" role="alert">
                  ${icon('alert')}
                  <div class="bd">
                    <span class="ttl">The provider’s address is not a web address</span>
                    <span>Trunks will not open it. Check the server’s URL and start again.</span>
                  </div>
                </div>`
              : html`<p>
                  <span class="mono">${name}</span> asks for authorization. Sign in with its provider
                  and grant Trunks access. The server is registered when that finishes; until then
                  nothing is stored.
                </p>`}
            <div class="row">${open('Open provider', true)}${cancel}</div>
            <p class="hint">Opens in a new window. Keep this page open. The request is valid until ${until}.</p>
          </div>
        </section>`;
      case 'waiting':
        return html`<section class="panel narrow">
          <div class="panel-b col roomy">
            <h2 class="row" role="status">${icon('spinner', 'spin')}Waiting for authorization…</h2>
            <p>Finish signing in in the provider window. This page checks every few seconds.</p>
            <div class="row">${open('Open provider again', false)}${cancel}</div>
            <p class="hint">
              Window closed by mistake? Open it again. The same request stays valid until ${until}.
            </p>
          </div>
        </section>`;
      case 'authorized': {
        const found = this.registered?.counts;
        return html`<section class="panel narrow">
          <div class="panel-b col roomy">
            <div class="alert success" role="status" tabindex="-1" data-focus>
              ${icon('checkCircle')}
              <div class="bd">
                <span class="ttl"><span class="mono">${name}</span> is authorized and registered</span>
                <span
                  >${found === undefined
                    ? 'Its tools are available through the gateway.'
                    : `Trunks found ${count(found.tools, 'tool')}, ${count(found.prompts, 'prompt')} and ${count(found.resources, 'resource')}. They are enabled.`}</span
                >
                <div class="act">
                  <a class="btn primary" href="/servers/detail/?name=${encodeURIComponent(name)}">View server</a>
                </div>
              </div>
            </div>
            <p class="hint">The provider window can be closed.</p>
          </div>
        </section>`;
      }
      default:
        return html`<section class="panel narrow">
          <div class="panel-b col roomy">
            <div class="alert warning" role="status" tabindex="-1" data-focus>
              ${icon('warning')}
              <div class="bd">
                <span class="ttl">The authorization request expired</span>
                <span>Nothing was registered. Start again to get a new provider link.</span>
                <div class="act">
                  <button type="button" class="btn primary" @click=${() => this.#again()}>
                    ${icon('rotate')}Start again
                  </button>
                  <a class="btn" href="/servers/">Back to servers</a>
                </div>
              </div>
            </div>
          </div>
        </section>`;
    }
  }

  #form(): TemplateResult {
    const d = this.draft;
    const local = d.kind === 'local';
    const wrong = Object.keys(this.found).length;
    return html`<form class="form narrow" novalidate @submit=${(e: SubmitEvent) => void this.#submit(e)}>
      ${this.failure === undefined
        ? nothing
        : html`<div class="alert danger" role="alert" id="r-failure" tabindex="-1">
            ${icon('alert')}
            <div class="bd">
              <span class="ttl">Couldn’t register ${d.name.trim()}</span>
              <span>${this.failure} Nothing was stored.</span>
            </div>
          </div>`}
      ${this.#text('name', 'r-name', 'Name', d.name, (v) => this.#set({ name: v }), {
        required: true,
        mono: true,
        hint: html`Letters, digits, hyphens and underscores. Becomes the tool prefix, as in
          <span class="mono">${nameProblem(d.name.trim()) === undefined ? d.name.trim() : 'jira'}__search_issues</span>.`,
      })}
      <div class="fld">
        <label for="r-desc">Description</label>
        <div class="inp ta">
          <textarea
            id="r-desc"
            rows="3"
            aria-describedby="r-desc-h"
            .value=${d.description}
            @input=${(e: InputEvent) => this.#set({ description: (e.target as HTMLTextAreaElement).value })}
          ></textarea>
        </div>
        <div class="hint" id="r-desc-h">Shown to people in the console.</div>
      </div>
      <div class="fld">
        <span class="lb" id="r-kind">How Trunks reaches it</span>
        <div class="bgrp" role="group" aria-labelledby="r-kind">
          <button type="button" class="btn" aria-pressed=${local ? 'true' : 'false'} @click=${() => this.#kind('local')}>
            ${icon('terminal')}Local process
          </button>
          <button type="button" class="btn" aria-pressed=${local ? 'false' : 'true'} @click=${() => this.#kind('remote')}>
            ${icon('globe')}Remote server
          </button>
        </div>
      </div>
      ${local ? this.#local(d) : this.#remote(d)}
      <div class="fld">
        <label for="r-mode">Session mode</label>
        <div class="inp">
          <select
            id="r-mode"
            aria-describedby="r-mode-h"
            @change=${(e: Event) =>
              this.#set({ mode: (e.target as HTMLSelectElement).value === 'stateful' ? 'stateful' : 'stateless' })}
          >
            <option value="stateless" ?selected=${d.mode === 'stateless'}>stateless</option>
            <option value="stateful" ?selected=${d.mode === 'stateful'}>stateful</option>
          </select>
        </div>
        <div class="hint" id="r-mode-h">
          ${local
            ? 'Stateless starts the process for each call. Stateful keeps one running and restarts it after a failure.'
            : 'Stateless opens a session for each call. Stateful keeps one upstream session open.'}
        </div>
      </div>
      ${wrong === 0
        ? nothing
        : html`<div class="ferr big" role="alert">
            ${icon('alert')}${wrong === 1 ? '1 field needs attention.' : `${wrong} fields need attention.`}
          </div>`}
      <div class="row">
        <line-button variant="primary" type="submit" ?loading=${this.busy}
          >${this.busy ? 'Connecting…' : 'Register server'}</line-button
        >
        <a class="btn" href="/servers/">Cancel</a>
      </div>
      ${this.busy
        ? html`<p class="hint" role="status">
            Trunks is connecting to the server to read its tools. This can take up to half a minute.
          </p>`
        : nothing}
    </form>`;
  }

  #kind(kind: Draft['kind']): void {
    this.#set({ kind });
    // Problems of the fields that are no longer on screen would be counted but not shown.
    this.found = {};
  }

  #local(d: Draft): TemplateResult {
    return html`<fieldset class="fs">
      <legend>Local process</legend>
      ${this.#text('command', 'r-cmd', 'Command', d.command, (v) => this.#set({ command: v }), {
        required: true,
        mono: true,
        hint: 'An executable on the gateway host’s PATH, or an absolute path. It runs as the Trunks user.',
      })}
      <div class="fld">
        <span class="lb" id="r-args">Arguments</span>
        <div class="col" role="group" aria-labelledby="r-args">
          ${d.args.map(
            (arg, i) =>
              html`<div class="kvrow one">
                <div class="inp mono">
                  <input
                    id="arg-${i}"
                    type="text"
                    spellcheck="false"
                    autocomplete="off"
                    aria-label="Argument ${i + 1}"
                    .value=${arg}
                    @input=${(e: InputEvent) =>
                      this.#set({ args: d.args.map((a, j) => (j === i ? (e.target as HTMLInputElement).value : a)) })}
                  />
                </div>
                ${removeButton(`Remove argument ${i + 1}`, () => this.#set({ args: d.args.filter((_, j) => j !== i) }))}
              </div>`,
          )}
        </div>
        <div>
          <button type="button" class="btn borderless" @click=${() => this.#add('arg', { args: [...d.args, ''] }, d.args.length)}>
            ${icon('plus')}Add argument
          </button>
        </div>
        <div class="hint">One argument per row, passed in this order. No shell expansion.</div>
      </div>
      ${this.#pairs('env', 'Environment variables', 'variable', d.env, (env) => this.#set({ env }))}
    </fieldset>`;
  }

  #remote(d: Draft): TemplateResult {
    const redirect = redirectUri(location.origin);
    return html`<fieldset class="fs">
      <legend>Remote server</legend>
      ${this.#text('url', 'r-url', 'URL', d.url, (v) => this.#set({ url: v }), {
        required: true,
        mono: true,
        hint: 'The server’s streamable HTTP endpoint.',
      })}
      <div class="fld">
        <label for="r-tok">Bearer token</label>
        ${this.#secret('r-tok', d.token, (v) => this.#set({ token: v }), 'token', 'r-tok-h', 'Optional')}
        <div class="hint" id="r-tok-h">
          Leave empty if the server asks for authorization through its provider: Trunks then takes you
          there.
        </div>
      </div>
      ${this.#pairs('hd', 'Headers', 'header', d.headers, (headers) => this.#set({ headers }))}
      <details class="adv">
        <summary>Authorization with your own OAuth client</summary>
        <div class="col roomy">
          <p class="hint">
            Only for a provider that does not register clients by itself. Create a client there with
            this redirect URI, then fill in what it gave you. Ignored when a bearer token is set.
          </p>
          <div class="mono-box">
            <span class="v" title=${redirect}>${redirect}</span>
            <trunks-copy always value=${redirect} label="Copy redirect URI"></trunks-copy>
          </div>
          ${this.#text(undefined, 'r-cid', 'Client ID', d.clientId, (v) => this.#set({ clientId: v }), { mono: true })}
          <div class="fld">
            <label for="r-csec">Client secret</label>
            ${this.#secret('r-csec', d.clientSecret, (v) => this.#set({ clientSecret: v }), 'secret')}
          </div>
          ${this.#text(undefined, 'r-scp', 'Scopes', d.scopes, (v) => this.#set({ scopes: v }), {
            mono: true,
            hint: 'Separated by spaces. Leave empty for the provider’s defaults.',
          })}
        </div>
      </details>
    </fieldset>`;
  }

  /** Adds a row and puts the cursor in it. */
  #add(prefix: string, changes: Partial<Draft>, index: number): void {
    this.#set(changes);
    void this.updateComplete.then(() =>
      this.querySelector<HTMLElement>(`#${prefix}-${prefix === 'arg' ? '' : 'k'}${index}`)?.focus(),
    );
  }

  #text(
    field: Field | undefined,
    id: string,
    label: string,
    value: string,
    change: (value: string) => void,
    options: { required?: boolean; mono?: boolean; hint?: TemplateResult | string } = {},
  ): TemplateResult {
    const problem = field === undefined ? undefined : this.found[field];
    const described = [problem === undefined ? '' : `${id}-e`, options.hint === undefined ? '' : `${id}-h`]
      .filter((x) => x !== '')
      .join(' ');
    return html`<div class="fld">
      <label for=${id}>${label} ${options.required ? html`<span class="req">(required)</span>` : nothing}</label>
      <div class="inp ${options.mono ? 'mono' : ''} ${problem === undefined ? '' : 'invalid'}">
        <input
          id=${id}
          type="text"
          spellcheck="false"
          autocomplete="off"
          autocapitalize="off"
          aria-required=${options.required ? 'true' : 'false'}
          aria-invalid=${problem === undefined ? 'false' : 'true'}
          aria-describedby=${described === '' ? nothing : described}
          .value=${value}
          @input=${(e: InputEvent) => change((e.target as HTMLInputElement).value)}
        />
      </div>
      ${problem === undefined ? nothing : html`<div class="ferr" id="${id}-e">${icon('alert')}${problem}</div>`}
      ${options.hint === undefined ? nothing : html`<div class="hint" id="${id}-h">${options.hint}</div>`}
    </div>`;
  }

  /** A field whose value is hidden until asked for. */
  #secret(
    id: string,
    value: string,
    change: (value: string) => void,
    what: string,
    describedBy?: string,
    placeholder?: string,
    label?: string,
  ): TemplateResult {
    const shown = this.shown.has(id);
    const flip = () => {
      const next = new Set(this.shown);
      if (shown) next.delete(id);
      else next.add(id);
      this.shown = next;
    };
    return html`<div class="inp mono">
      <input
        id=${id}
        type=${shown ? 'text' : 'password'}
        spellcheck="false"
        autocomplete="off"
        placeholder=${placeholder ?? nothing}
        aria-label=${label ?? nothing}
        aria-describedby=${describedBy ?? nothing}
        .value=${value}
        @input=${(e: InputEvent) => change((e.target as HTMLInputElement).value)}
      />
      <span class="sfx">
        <button
          type="button"
          class="btn ibtn ghost"
          aria-label="Show ${what}"
          aria-pressed=${shown ? 'true' : 'false'}
          title="Show ${what}"
          @click=${flip}
        >
          ${icon('eye')}
        </button>
      </span>
    </div>`;
  }

  /** Rows of name and secret value: environment variables, headers. */
  #pairs(prefix: 'env' | 'hd', label: string, one: string, pairs: Pair[], change: (pairs: Pair[]) => void) {
    const field: Field = prefix === 'env' ? 'env' : 'headers';
    const problem = this.found[field];
    const edit = (i: number, part: Partial<Pair>) => change(pairs.map((p, j) => (j === i ? { ...p, ...part } : p)));
    return html`<div class="fld">
      <span class="lb" id="${prefix}-l">${label}</span>
      <div class="col" role="group" aria-labelledby="${prefix}-l" aria-describedby="${prefix}-h">
        ${pairs.map(
          (pair, i) =>
            html`<div class="kvrow">
              <div class="inp mono ${problem !== undefined && pair.key.trim() === '' ? 'invalid' : ''}">
                <input
                  id="${prefix}-k${i}"
                  type="text"
                  spellcheck="false"
                  autocomplete="off"
                  autocapitalize="off"
                  placeholder="NAME"
                  aria-label="Name of ${one} ${i + 1}"
                  .value=${pair.key}
                  @input=${(e: InputEvent) => edit(i, { key: (e.target as HTMLInputElement).value })}
                />
              </div>
              ${this.#secret(
                `${prefix}-v${i}`,
                pair.value,
                (v) => edit(i, { value: v }),
                'value',
                undefined,
                'value',
                `Value of ${one} ${i + 1}`,
              )}
              ${removeButton(`Remove ${pair.key.trim() === '' ? `${one} ${i + 1}` : pair.key.trim()}`, () =>
                change(pairs.filter((_, j) => j !== i)),
              )}
            </div>`,
        )}
      </div>
      <div>
        <button
          type="button"
          class="btn borderless"
          @click=${() =>
            this.#add(prefix, prefix === 'env' ? { env: [...pairs, { key: '', value: '' }] } : { headers: [...pairs, { key: '', value: '' }] }, pairs.length)}
        >
          ${icon('plus')}Add ${one}
        </button>
      </div>
      ${problem === undefined ? nothing : html`<div class="ferr">${icon('alert')}${problem}</div>`}
      <div class="hint" id="${prefix}-h">Values are stored and never shown again.</div>
    </div>`;
  }
}

function removeButton(label: string, remove: () => void): TemplateResult {
  return html`<button type="button" class="btn ibtn ghost" aria-label=${label} title=${label} @click=${remove}>
    ${icon('trash')}
  </button>`;
}

/** The server's reason for refusing a registration, in words that say what to do. */
function refusal(e: unknown): string {
  if (e instanceof ApiError && e.code === 'upstream_oauth_required') {
    return 'The server did not accept the bearer token and asks for authorization through its provider. Remove the token to authorize that way.';
  }
  const message = e instanceof Error ? e.message : 'Unknown error';
  return /[.!?]$/.test(message) ? message : `${message}.`;
}

customElements.define('trunks-register', TrunksRegister);
