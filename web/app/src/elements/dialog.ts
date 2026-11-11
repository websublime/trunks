// A confirmation before something that cannot be undone. A native <dialog>: it traps focus,
// makes the rest of the page inert and closes on Esc by itself.

import { html, nothing, render, type TemplateResult } from 'lit';

import { icon } from './icon';

export interface Confirmation {
  title: string;
  /** What will happen, in a sentence or two. */
  body: TemplateResult | string;
  /** The verb on the button: "Delete". */
  action: string;
  /** The same verb while it runs: "Deleting…". */
  working: string;
  /** Title of the error shown when `run` fails: "Couldn’t delete". */
  failed: string;
  run: () => Promise<unknown>;
}

let serial = 0;

/** Asks; resolves `true` once `run` succeeded, `false` when the person backed out. */
export function confirm(c: Confirmation): Promise<boolean> {
  return new Promise((resolve) => {
    const id = `dlg-${++serial}`;
    const dialog = document.createElement('dialog');
    dialog.className = 'dlg';
    dialog.setAttribute('role', 'alertdialog');
    dialog.setAttribute('aria-labelledby', `${id}-h`);
    dialog.setAttribute('aria-describedby', `${id}-d`);
    let busy = false;
    let error: string | undefined;
    let done = false;

    const close = () => {
      if (!busy) dialog.close();
    };
    const go = async () => {
      if (busy) return;
      busy = true;
      error = undefined;
      draw();
      try {
        await c.run();
        done = true;
        busy = false;
        dialog.close();
      } catch (e) {
        busy = false;
        error = e instanceof Error ? e.message : 'Unknown error';
        draw();
      }
    };
    const draw = () =>
      render(
        html`<div class="dlg-h">
            <h2 id="${id}-h">${c.title}</h2>
            <button type="button" class="btn ibtn ghost" aria-label="Close" title="Close" @click=${close}>
              ${icon('close')}
            </button>
          </div>
          <div class="dlg-b">
            <p id="${id}-d">${c.body}</p>
            ${error === undefined
              ? nothing
              : html`<div class="alert danger" role="alert">
                  ${icon('alert')}
                  <div class="bd">
                    <span class="ttl">${c.failed}</span><span>${error} Nothing changed.</span>
                  </div>
                </div>`}
          </div>
          <div class="dlg-f">
            <button type="button" class="btn" data-cancel @click=${close}>Cancel</button>
            <button
              type="button"
              class="btn danger"
              aria-busy=${busy ? 'true' : 'false'}
              aria-disabled=${busy ? 'true' : 'false'}
              @click=${() => void go()}
            >
              ${busy ? html`${icon('spinner', 'spin')}${c.working}` : c.action}
            </button>
          </div>`,
        dialog,
      );

    // Esc while the action runs would leave it running with nobody watching.
    dialog.addEventListener('cancel', (event) => {
      if (busy) event.preventDefault();
    });
    dialog.addEventListener('close', () => {
      dialog.remove();
      resolve(done);
    });
    draw();
    document.body.append(dialog);
    dialog.showModal();
    // The safest button gets the focus.
    dialog.querySelector<HTMLButtonElement>('[data-cancel]')?.focus();
  });
}

/**
 * Shows an access token, once. There is no way to close it by accident: no close button,
 * Esc does nothing, only "Done". The token is in the page while the dialog is open and
 * nowhere afterwards.
 */
export function showToken(owner: string, token: string, use: TemplateResult | string): Promise<void> {
  return new Promise((resolve) => {
    const id = `tok-${++serial}`;
    const dialog = document.createElement('dialog');
    dialog.className = 'dlg';
    dialog.setAttribute('aria-labelledby', `${id}-h`);
    dialog.setAttribute('aria-describedby', `${id}-w`);
    let copied: boolean | undefined;

    const copy = async () => {
      try {
        await navigator.clipboard.writeText(token);
        copied = true;
      } catch {
        copied = false;
      }
      draw();
    };
    const draw = () =>
      render(
        html`<div class="dlg-h">
            <h2 id="${id}-h">Access token for <span class="mono">${owner}</span></h2>
          </div>
          <div class="dlg-b">
            <div class="alert warning" id="${id}-w">
              ${icon('warning')}
              <div class="bd">
                <span class="ttl">Shown once</span>
                <span
                  >Copy the token now and store it somewhere safe. Trunks will not show it again. If
                  it is lost, replace it with a new one.</span
                >
              </div>
            </div>
            <div class="fld">
              <span class="lb" id="${id}-l">Token</span>
              <div class="mono-box wrap" role="group" aria-labelledby="${id}-l">
                <span class="v" data-token>${token}</span>
              </div>
            </div>
            <p class="hint">${use}</p>
            <div class="row">
              <button type="button" class="btn primary" data-copy @click=${() => void copy()}>
                ${icon(copied === true ? 'check' : 'copy')}Copy token
              </button>
              <span class="small lo" role="status"
                >${copied === undefined
                  ? ''
                  : copied
                    ? 'Copied.'
                    : 'Couldn’t copy. Select the token and copy it yourself.'}</span
              >
            </div>
          </div>
          <div class="dlg-f">
            <button type="button" class="btn" @click=${() => dialog.close()}>Done</button>
          </div>`,
        dialog,
      );

    dialog.addEventListener('cancel', (event) => event.preventDefault());
    dialog.addEventListener('close', () => {
      dialog.remove();
      resolve();
    });
    draw();
    document.body.append(dialog);
    dialog.showModal();
    dialog.querySelector<HTMLButtonElement>('[data-copy]')?.focus();
  });
}
