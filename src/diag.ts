// What the page says to the log (the same file as the back, line `app_lib::front`). The page used to leave no trace at all: a loop
// in it (a state published again and again) was only visible from the OTHER device, as a storm of reconnections. Three things:
// events worth a line (`logEvent`, rate limited so a loop cannot flood the file), counters that are summed up in ONE line every
// 30 s and written as a warning when a number looks like a storm (`count`), and the page's own errors (`startDiagnostics`).

import { api } from './api';

export type Level = 'info' | 'warn' | 'error';
export type Sink = (level: Level, area: string, message: string) => void;

/** At most this many lines per second, whatever the page does. */
const LINES_PER_SECOND = 20;
/** The same line again within this time is counted, not written. */
const REPEAT_MS = 2000;
export const STATS_EVERY_MS = 30_000;

/** Per window: more of these than this is written as a warning. */
const SUSPICIOUS: Record<string, number> = { publish: 120, loadConnect: 200, commandIn: 200, commandOut: 400 };

/** A logger on its own clock and sink, so the limits can be tested. */
export function createDiag(sink: Sink, now: () => number = Date.now) {
  let tokens = LINES_PER_SECOND;
  let refilled = now();
  let skipped = 0;
  const lastSeen = new Map<string, { at: number; hidden: number }>();
  const counters = new Map<string, number>();

  function allow(): boolean {
    const t = now();
    tokens = Math.min(LINES_PER_SECOND, tokens + ((t - refilled) / 1000) * LINES_PER_SECOND);
    refilled = t;
    if (tokens < 1) {
      skipped++;
      return false;
    }
    tokens -= 1;
    return true;
  }

  function logEvent(level: Level, area: string, message: string): void {
    const key = `${level}|${area}|${message}`;
    const t = now();
    const seen = lastSeen.get(key);
    if (seen && t - seen.at < REPEAT_MS) {
      seen.hidden++;
      return;
    }
    if (lastSeen.size > 200) lastSeen.clear();
    const hidden = seen?.hidden ?? 0;
    lastSeen.set(key, { at: t, hidden: 0 });
    if (!allow()) return;
    const extra = hidden ? ` (x${hidden + 1} just before)` : '';
    const lost = skipped ? ` [${skipped} lines held back]` : '';
    skipped = 0;
    sink(level, area, `${message}${extra}${lost}`);
  }

  function count(key: string, n = 1): void {
    counters.set(key, (counters.get(key) ?? 0) + n);
  }

  /** The summary of the window (null if nothing happened); the counters start over. */
  function summary(seconds: number): { level: Level; text: string } | null {
    if (counters.size === 0) return null;
    const entries = [...counters.entries()].sort(([a], [b]) => a.localeCompare(b));
    counters.clear();
    const suspicious = entries.filter(([key, n]) => n > (SUSPICIOUS[key] ?? Infinity));
    const text = `stats (${seconds}s): ${entries.map(([key, n]) => `${key}=${n}`).join(' ')}`;
    return suspicious.length
      ? { level: 'warn', text: `${text} <- more than usual: ${suspicious.map(([key]) => key).join(', ')}` }
      : { level: 'info', text };
  }

  return { logEvent, count, summary };
}

const diag = createDiag((level, area, message) => api.log(level, area, message));
export const logEvent = diag.logEvent;
export const count = diag.count;

const text = (value: unknown): string => {
  if (value instanceof Error) return `${value.name}: ${value.message}`;
  if (typeof value === 'string') return value;
  try {
    return JSON.stringify(value) ?? String(value);
  } catch {
    return String(value);
  }
};

/** The summary of the counters every 30 s, and what goes wrong in the page (errors, rejected promises, console warnings and errors). */
export function startDiagnostics(): void {
  window.setInterval(() => {
    const summary = diag.summary(STATS_EVERY_MS / 1000);
    if (summary) diag.logEvent(summary.level, 'stats', summary.text);
  }, STATS_EVERY_MS);
  window.addEventListener('error', event => diag.logEvent('error', 'page', `${event.message} (${event.filename?.split('/').pop() ?? '?'}:${event.lineno})`));
  window.addEventListener('unhandledrejection', event => diag.logEvent('error', 'page', `unhandled rejection: ${text(event.reason)}`));
  let inside = false;
  for (const level of ['warn', 'error'] as const) {
    const original = console[level].bind(console);
    console[level] = (...args: unknown[]) => {
      original(...args);
      if (inside) return;
      inside = true;
      try {
        diag.logEvent(level, 'console', args.map(text).join(' '));
      } finally {
        inside = false;
      }
    };
  }
}
