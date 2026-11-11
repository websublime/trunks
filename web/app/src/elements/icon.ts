import { html, type TemplateResult } from 'lit';

import { ICONS, type IconName } from '../lib/icons';

/** An icon for a Lit template. Decorative unless `label` is given. */
export function icon(name: IconName, extra = '', label?: string): TemplateResult {
  return label === undefined
    ? html`<svg class="ic ${extra}" viewBox="0 0 24 24" aria-hidden="true">
        <path d=${ICONS[name]}></path>
      </svg>`
    : html`<svg class="ic ${extra}" viewBox="0 0 24 24" role="img" aria-label=${label}>
        <path d=${ICONS[name]}></path>
      </svg>`;
}
