// Small text helpers shared by the elements.

/** `3 d 4 h 12 min`, `4 h 12 min`, `12 min`, `45 s`: the two or three largest units. */
export function uptime(seconds: number): string {
  const s = Math.max(0, Math.floor(seconds));
  const days = Math.floor(s / 86_400);
  const hours = Math.floor((s % 86_400) / 3_600);
  const minutes = Math.floor((s % 3_600) / 60);
  if (days > 0) return `${days} d ${hours} h ${minutes} min`;
  if (hours > 0) return `${hours} h ${minutes} min`;
  if (minutes > 0) return `${minutes} min`;
  return `${s} s`;
}

/** `12:04:31`, in the reader's own time zone. */
export function clock(date: Date): string {
  const two = (n: number) => String(n).padStart(2, '0');
  return `${two(date.getHours())}:${two(date.getMinutes())}:${two(date.getSeconds())}`;
}

/** `v1.2.3` → `1.2.3`: the version as a value rather than as a label. */
export function bareVersion(version: string): string {
  return version.replace(/^v/, '');
}

export function modeLabel(mode: 'development' | 'enterprise'): string {
  return mode === 'enterprise' ? 'Enterprise' : 'Development';
}
