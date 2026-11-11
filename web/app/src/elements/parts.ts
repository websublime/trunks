// Pieces the views share: badges, the name cell of a table, states of a data region and
// the copy button. Native elements with line://ui tokens until line://ui has them.

import { html, nothing, type TemplateResult } from 'lit';

import type { OAuthStatus, Transport } from '../api/types.gen';
import type { IconName } from '../lib/icons';
import { icon } from './icon';

type Tone = '' | 'success' | 'warning' | 'danger' | 'info' | 'accent';

export function badge(text: string | number, tone: Tone = '', glyph?: IconName, mono = false) {
  return html`<span class="bdg ${tone} ${mono ? 'mono' : ''}"
    >${glyph === undefined ? nothing : icon(glyph)}${text}</span
  >`;
}

export const transportBadge = (transport: Transport) =>
  badge(transport, '', transport === 'stdio' ? 'terminal' : 'globe', true);

export const statusBadge = (enabled: boolean) =>
  enabled ? badge('Enabled', 'success', 'check') : badge('Disabled', '', 'minusCircle');

export function oauthBadge(status: OAuthStatus): TemplateResult {
  switch (status) {
    case 'authorized':
      return badge('Authorized', 'success', 'checkCircle');
    case 'pending':
      return badge('Pending', 'warning', 'clock');
    case 'none':
      return badge('None', '', 'minusCircle');
  }
}

/** A monospace name with a copy button, and a description below it. */
export function nameCell(name: string, description: string, href?: string, shown = name) {
  return html`<div class="nm">
    <div class="nmr">
      ${href === undefined
        ? html`<span class="name-t" title=${name}>${shown}</span>`
        : html`<a class="name" href=${href} title=${name}>${shown}</a>`}
      <trunks-copy value=${name} label=${`Copy ${name}`}></trunks-copy>
    </div>
    ${description === '' ? nothing : html`<span class="desc" title=${description}>${description}</span>`}
  </div>`;
}

/** An error text, two lines at most; the full text is in the title. */
export function errorText(message: string) {
  return html`<div class="err">
    ${icon('alert')}<span class="clamp2" title=${message}>${message}</span>
  </div>`;
}

export const none = (text = '—') => html`<span class="lo">${text}</span>`;

/** A user or a team, by kind and name. */
export function subject(kind: string, name: string, tag?: string) {
  return html`<span class="subj"
    >${icon(kind === 'team' ? 'teams' : 'user')}<span class="lo small">${kind}</span
    ><span class="mono">${name}</span>${tag === undefined ? nothing : html`<span class="bdg">${tag}</span>`}</span
  >`;
}

export interface TextField {
  id: string;
  label: string;
  value: string;
  input: (value: string) => void;
  problem?: string | undefined;
  hint?: TemplateResult | string;
  required?: boolean;
  mono?: boolean;
  /** `id` of a `<datalist>` with suggestions. */
  list?: string;
  placeholder?: string;
}

/** A labelled text input with its hint and, when there is one, its problem. */
export function textField(f: TextField) {
  const described = [f.problem === undefined ? '' : `${f.id}-e`, f.hint === undefined ? '' : `${f.id}-h`]
    .filter((x) => x !== '')
    .join(' ');
  return html`<div class="fld">
    <label for=${f.id}>${f.label} ${f.required ? html`<span class="req">(required)</span>` : nothing}</label>
    <div class="inp ${f.mono ? 'mono' : ''} ${f.problem === undefined ? '' : 'invalid'}">
      <input
        id=${f.id}
        type="text"
        spellcheck="false"
        autocomplete="off"
        autocapitalize="off"
        list=${f.list ?? nothing}
        placeholder=${f.placeholder ?? nothing}
        aria-required=${f.required ? 'true' : 'false'}
        aria-invalid=${f.problem === undefined ? 'false' : 'true'}
        aria-describedby=${described === '' ? nothing : described}
        .value=${f.value}
        @input=${(e: InputEvent) => f.input((e.target as HTMLInputElement).value)}
      />
    </div>
    ${f.problem === undefined ? nothing : html`<div class="ferr" id="${f.id}-e">${icon('alert')}${f.problem}</div>`}
    ${f.hint === undefined ? nothing : html`<div class="hint" id="${f.id}-h">${f.hint}</div>`}
  </div>`;
}

/**
 * An on/off switch that acts at once. `label` names what it switches ("Enabled: github");
 * while `busy` it cannot be pressed again.
 */
export function toggle(on: boolean, label: string, change: (next: boolean) => void, busy = false) {
  return html`<button
    type="button"
    class="sw"
    role="switch"
    aria-checked=${on ? 'true' : 'false'}
    aria-label=${label}
    ?disabled=${busy}
    @click=${() => change(!on)}
  >
    <span class="tr"><span class="th"></span></span><span class="lbl">${on ? 'On' : 'Off'}</span>
  </button>`;
}

/** A centred message inside a panel: nothing here, nothing found, no permission. */
export function state(
  glyph: IconName,
  title: string,
  text: string | TemplateResult,
  action?: TemplateResult,
) {
  return html`<div class="state">
    ${icon(glyph, 'ic-xl')}
    <span class="ttl">${title}</span>
    <p>${text}</p>
    ${action ?? nothing}
  </div>`;
}

class TrunksCopy extends HTMLElement {
  #timer: ReturnType<typeof setTimeout> | undefined;

  connectedCallback(): void {
    if (this.firstElementChild !== null) return;
    const label = this.getAttribute('label') ?? 'Copy';
    const button = document.createElement('button');
    button.type = 'button';
    button.className = `btn ibtn ghost copy ${this.hasAttribute('always') ? 'show' : ''}`;
    button.setAttribute('aria-label', label);
    button.title = label;
    button.innerHTML = glyph('copy');
    button.addEventListener('click', () => void this.#copy(button, label));
    const status = document.createElement('span');
    status.className = 'vh';
    status.setAttribute('role', 'status');
    this.append(button, status);
  }

  async #copy(button: HTMLButtonElement, label: string): Promise<void> {
    const value = this.getAttribute('value') ?? '';
    const status = this.querySelector('[role=status]');
    try {
      await navigator.clipboard.writeText(value);
      button.innerHTML = glyph('check');
      button.title = 'Copied';
      if (status) status.textContent = 'Copied';
    } catch {
      button.title = 'Couldn’t copy';
      if (status) status.textContent = 'Couldn’t copy. Select the text and copy it yourself.';
    }
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => {
      button.innerHTML = glyph('copy');
      button.title = label;
      if (status) status.textContent = '';
    }, 2000);
  }
}

import { ICONS } from '../lib/icons';

function glyph(name: IconName): string {
  return `<svg class="ic" viewBox="0 0 24 24" aria-hidden="true"><path d="${ICONS[name]}"></path></svg>`;
}

if (!customElements.get('trunks-copy')) customElements.define('trunks-copy', TrunksCopy);
