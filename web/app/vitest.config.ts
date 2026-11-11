import { defineConfig } from 'vitest/config';

// Unit tests cover the plain TypeScript modules (API client, session rules, formatting).
// The pages and elements are covered by the Playwright smoke tests against the real binary.
export default defineConfig({
  test: { include: ['src/**/*.test.ts'], environment: 'node' },
});
