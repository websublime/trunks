// Lists refresh on a fixed interval while the tab is visible and stop while it is hidden.
// There is no live stream: the server offers none.

export const REFRESH_SECONDS = 15;

export class Poller {
  #timer: ReturnType<typeof setInterval> | undefined;
  readonly #run: () => void;
  readonly #onVisibility = () => {
    if (document.hidden) this.#clear();
    else {
      this.#run();
      this.#arm();
    }
  };

  constructor(run: () => void) {
    this.#run = run;
  }

  start(): void {
    document.addEventListener('visibilitychange', this.#onVisibility);
    this.#run();
    if (!document.hidden) this.#arm();
  }

  stop(): void {
    document.removeEventListener('visibilitychange', this.#onVisibility);
    this.#clear();
  }

  /** Runs once now and restarts the interval. */
  now(): void {
    this.#run();
    if (!document.hidden) this.#arm();
  }

  #arm(): void {
    this.#clear();
    this.#timer = setInterval(this.#run, REFRESH_SECONDS * 1000);
  }

  #clear(): void {
    if (this.#timer !== undefined) clearInterval(this.#timer);
    this.#timer = undefined;
  }
}
