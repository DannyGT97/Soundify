import { defineConfig } from 'vitest/config';

// Unit tests of the front's pure logic. The screens are tested with Playwright
// (e2e/), and the back with `cargo test`.
export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
    environment: 'node',
  },
});
