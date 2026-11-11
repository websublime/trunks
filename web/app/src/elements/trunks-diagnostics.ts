// <trunks-diagnostics>: how this gateway is configured and what is failing. For
// administrators; the API refuses everyone else.

import { html, type TemplateResult } from 'lit';

import { get } from '../api/client';
import type { Diagnostics } from '../api/types.gen';
import { DataView, type Heading } from './data-view';
import { badge, errorText, none, state } from './parts';

class TrunksDiagnostics extends DataView<Diagnostics> {
  protected readonly subject = 'diagnostics';

  protected load(): Promise<Diagnostics> {
    return get<Diagnostics>('/api/dashboard/diagnostics');
  }

  protected heading(): Heading {
    return { title: 'Diagnostics', sub: 'How this gateway is configured and what is failing.' };
  }

  protected content(d: Diagnostics): TemplateResult {
    const seconds = (n: number, zero: string) => (n === 0 ? zero : `${n} s`);
    return html`
      <div class="grid g2 top">
        <section class="panel">
          <div class="panel-h"><h2>Configuration</h2></div>
          <div class="panel-b">
            <dl class="kv">
              <dt>Database backend</dt>
              <dd>${badge(d.database.backend, '', undefined, true)}</dd>
              <dt>Database</dt>
              <dd><span class="mono wrap-any">${d.database.url_redacted}</span></dd>
              <dt>Listen address</dt>
              <dd><span class="mono">${d.bind.host}:${d.bind.port}</span></dd>
              <dt>Allowed hosts</dt>
              <dd>
                ${d.allowed_hosts.length === 0
                  ? none('Loopback names only')
                  : html`<div class="tags">
                      ${d.allowed_hosts.map((h) => badge(h, '', undefined, true))}
                    </div>`}
              </dd>
              <dt>Metrics</dt>
              <dd>${d.metrics_enabled ? 'Served at /metrics' : 'Off'}</dd>
            </dl>
          </div>
        </section>
        <section class="panel" aria-labelledby="tl-h">
          <div class="panel-h"><h2 id="tl-h">Time limits</h2></div>
          <div class="tblw">
            <table class="tbl">
              <thead>
                <tr>
                  <th scope="col">Limit</th>
                  <th scope="col" class="num">Value</th>
                </tr>
              </thead>
              <tbody>
                <tr>
                  <td>Handshake with an upstream</td>
                  <td class="num">${seconds(d.upstream_init_timeout_sec, 'None')}</td>
                </tr>
                <tr>
                  <td>Idle upstream session</td>
                  <td class="num">${seconds(d.session_idle_timeout_sec, 'Never closed')}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </section>
      </div>
      <section class="panel" aria-labelledby="un-h">
        <div class="panel-h">
          <h2 id="un-h">Unreachable servers</h2>
          ${badge(d.servers_unreachable.length, d.servers_unreachable.length === 0 ? '' : 'danger')}
        </div>
        ${d.servers_unreachable.length === 0
          ? state('checkCircle', 'Every server answered', 'No upstream failed its last connection.')
          : html`<ul class="list">
              ${d.servers_unreachable.map(
                (s) =>
                  html`<li>
                    <div class="col grow tight">
                      <a class="name" href="/servers/detail/?name=${encodeURIComponent(s.name)}"
                        >${s.name}</a
                      >
                      ${errorText(s.error)}
                    </div>
                  </li>`,
              )}
            </ul>`}
      </section>
      <section class="panel" aria-labelledby="pa-h">
        <div class="panel-h">
          <h2 id="pa-h">Pending authorizations</h2>
          ${badge(d.pending_oauth.length, d.pending_oauth.length === 0 ? '' : 'warning')}
        </div>
        ${d.pending_oauth.length === 0
          ? state('checkCircle', 'Nothing pending', 'No server is waiting for an authorization.')
          : html`<ul class="list">
              ${d.pending_oauth.map(
                (p) =>
                  html`<li>
                    <div class="row grow">
                      <span class="name-t">${p.server_name}</span>
                      ${badge('Pending', 'warning', 'clock')}
                      <span class="small lo"
                        >expires ${new Date(p.expires_at).toLocaleTimeString()}</span
                      >
                    </div>
                  </li>`,
              )}
            </ul>`}
      </section>
    `;
  }
}

customElements.define('trunks-diagnostics', TrunksDiagnostics);
