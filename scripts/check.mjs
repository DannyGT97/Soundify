// `npm run check`: everything that must be green before calling a change done.
// The output is short on purpose: one line per tool when it passes, and only
// what failed when it does not. The full output of these tools runs to
// thousands of lines, and it is read after every change.

import { spawnSync } from 'node:child_process';

const env = { ...process.env, NO_COLOR: '1', CARGO_TERM_COLOR: 'never' };
delete env.FORCE_COLOR;

const steps = [
  {
    name: 'tsc',
    command: 'npx tsc --noEmit --pretty false',
    summary: () => 'ok',
  },
  {
    name: 'vitest',
    command: 'npx vitest run --reporter=dot',
    summary: out => `${out.match(/Tests\s+(\d+) passed/)?.[1] ?? '?'} ok`,
  },
  {
    name: 'cargo test',
    command: 'cargo test -q --workspace',
    summary: out => `${[...out.matchAll(/test result: ok\. (\d+) passed/g)].reduce((n, m) => n + Number(m[1]), 0)} ok`,
  },
  {
    name: 'clippy',
    command: 'cargo clippy -q --workspace --all-targets -- -D warnings',
    summary: () => 'ok',
  },
];

let failed = false;
for (const step of steps) {
  const run = spawnSync(step.command, { shell: true, env, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  const output = `${run.stdout ?? ''}${run.stderr ?? ''}`;
  if (run.status === 0) {
    console.log(`${step.name}: ${step.summary(output)}`);
  } else {
    failed = true;
    console.log(`${step.name}: FAILED\n${failures(output)}\n`);
  }
}
process.exit(failed ? 1 : 0);

/** The lines worth reading in a failed run, without the progress noise. */
function failures(output) {
  const noise = /^\s*(Compiling|Checking|Finished|Running|Blocking|Downloaded|Downloading|Updating|Locking|Adding)\b|^\s*$|^[·.]+$/;
  const lines = output.split(/\r?\n/).filter(line => !noise.test(line));
  const limit = 80;
  return lines.length > limit
    ? [...lines.slice(0, limit), `… ${lines.length - limit} more lines`].join('\n')
    : lines.join('\n');
}
