// Starts a real Trunks server for a spec and drives it with the real CLI.

import { execFile, spawn, type ChildProcess } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { promisify } from 'node:util';

const exe = process.platform === 'win32' ? '.exe' : '';
const target = resolve(import.meta.dirname, '../../../target/debug');
const TRUNKS = process.env['TRUNKS_BIN'] ?? join(target, `trunks${exe}`);
/** A stdio MCP server with three tools, one prompt and one resource. */
export const FAKE_UPSTREAM = process.env['TRUNKS_FAKE_UPSTREAM'] ?? join(target, `fake-upstream${exe}`);

async function freePort(): Promise<number> {
  return new Promise((done, fail) => {
    const probe = createServer();
    probe.once('error', fail);
    probe.listen(0, '127.0.0.1', () => {
      const address = probe.address();
      const port = typeof address === 'object' && address ? address.port : 0;
      probe.close(() => done(port));
    });
  });
}

export class Server {
  private constructor(
    readonly url: string,
    private readonly dir: string,
    private readonly child: ChildProcess,
  ) {}

  static async start(mode: 'development' | 'enterprise'): Promise<Server> {
    const dir = await mkdtemp(join(tmpdir(), 'trunks-web-'));
    const port = await freePort();
    const child = spawn(TRUNKS, ['start'], {
      env: {
        ...process.env,
        TRUNKS_MODE: mode,
        TRUNKS_BIND_HOST: '127.0.0.1',
        TRUNKS_PORT: String(port),
        TRUNKS_DATABASE_URL: `sqlite://${join(dir, 'trunks.db')}`,
        TRUNKS_LOG: 'warn',
      },
      stdio: 'ignore',
    });
    const url = `http://127.0.0.1:${port}`;
    const server = new Server(url, dir, child);
    for (let attempt = 0; attempt < 150; attempt++) {
      try {
        if ((await fetch(`${url}/health`)).ok) return server;
      } catch {
        // not listening yet
      }
      await new Promise((r) => setTimeout(r, 100));
    }
    await server.stop();
    throw new Error(`the ${mode} server did not start: ${TRUNKS}`);
  }

  /** Runs a management command against this server; returns what it printed. */
  async cli(...args: string[]): Promise<string> {
    const { stdout } = await promisify(execFile)(TRUNKS, ['--registry', this.url, ...args], {
      env: { ...process.env, TRUNKS_CONFIG: join(this.dir, 'cli.toml') },
    });
    return stdout;
  }

  async stop(): Promise<void> {
    this.child.kill();
    await new Promise((r) => this.child.once('exit', r));
    await rm(this.dir, { recursive: true, force: true });
  }
}
