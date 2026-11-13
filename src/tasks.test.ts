import { describe, expect, it } from 'vitest';
import { activityText, fileNumber, liveText, noticeTone, overallPercent, percent, pillResult, pointsToSettings, progressLine, shortStamp, summaryText, toTasksState } from './tasks';
import type { FinishedTask, RunningTask } from './state';

const NOW = new Date(2026, 10, 3, 20, 0);

function running(fraction: number | null, total = 4): RunningTask {
  return { id: 1, title: 'Lo-fi', origin: 'manager', step: fraction === null ? 'prepare' : 'download', fraction, total, blockKey: null };
}

describe('pillResult', () => {
  const done = (status: FinishedTask['status']): FinishedTask => ({ title: 'X', origin: 'url', status, summary: null, summaryArgs: [], finishedAt: '18:42' });

  it('shows the unseen result first, then the latest finished task once it has been seen', () => {
    expect(pillResult({ unseenResult: 'failed', history: [done('ok')] })).toBe('failed');
    expect(pillResult({ unseenResult: null, history: [done('warning'), done('ok')] })).toBe('warning');
  });

  it('only disappears when no finished task is left', () => {
    expect(pillResult({ unseenResult: null, history: [] })).toBeNull();
  });
});

describe('toTasksState', () => {
  it('maps the running task, its origin key and the history', () => {
    const state = toTasksState({
      running: [{ id: 3, title: 'Lo-fi', origin: 'tasks.origin.manager', step: 'download', fraction: 0.5, total: 4 }],
      queued: [], unseen: null,
      history: [{ status: 'WARNING', title: 'Antes', origin: 'tasks.origin.url', summary: 'activity.retry.summaryOne',
        summaryArgs: ['X', 'failReason.geoBlocked'], finishedAt: '2026-11-03 18:42' }],
    }, NOW);
    expect(state.task).toEqual({ id: 3, title: 'Lo-fi', origin: 'manager', step: 'download', fraction: 0.5, total: 4, blockKey: null });
    expect(state.history[0]).toEqual({ title: 'Antes', origin: 'url', status: 'warning', summary: 'activity.retry.summaryOne',
      summaryArgs: ['X', 'failReason.geoBlocked'], finishedAt: '18:42' });
  });

  it('shows the first of the running tasks, and no result is pending while any runs', () => {
    const wire = { title: 'A', origin: 'tasks.origin.manager', step: 'download' as const, fraction: 0.2, total: 2 };
    const state = toTasksState({ running: [{ ...wire, id: 7 }, { ...wire, id: 8, title: 'B' }], queued: [], unseen: 'FAILED', history: [] }, NOW);
    expect(state.task).toMatchObject({ id: 7, title: 'A' });
    expect(state.unseenResult).toBeNull();
  });

  it('keeps every running task, the first one as the lead', () => {
    const wire = { origin: 'tasks.origin.manager', step: 'download' as const, fraction: 0.2, total: 2 };
    const state = toTasksState({ running: [{ ...wire, id: 7, title: 'A', blockKey: 'a' }, { ...wire, id: 8, title: 'B', blockKey: 'b' }], queued: [], unseen: null, history: [] }, NOW);
    expect(state.running.map(task => [task.id, task.blockKey])).toEqual([[7, 'a'], [8, 'b']]);
    expect(state.task).toBe(state.running[0]);
    expect(toTasksState({ running: [], queued: [], unseen: null, history: [] }, NOW)).toMatchObject({ task: null, running: [] });
  });

  it('carries the block being downloaded, once the back knows it', () => {
    const wire = { id: 1, title: 'Lo-fi', origin: 'tasks.origin.manager', step: 'download' as const, fraction: 0, total: 3 };
    expect(toTasksState({ running: [{ ...wire, blockKey: 'D:/x/data.json' }], queued: [], unseen: null, history: [] }, NOW).task?.blockKey)
        .toBe('D:/x/data.json');
    expect(toTasksState({ running: [wire], queued: [], unseen: null, history: [] }, NOW).task?.blockKey).toBeNull();
  });

  it('keeps the queue in order, with the origin as the screens know it', () => {
    const state = toTasksState({
      running: [],
      queued: [
        { id: 4, title: 'Chill', origin: 'tasks.origin.manager', blockKey: 'D:/x/data.json' },
        { id: 5, title: 'https://youtu.be/x', origin: 'tasks.origin.url' },
      ],
      unseen: null,
      history: [],
    }, NOW);
    expect(state.queue).toEqual([
      { id: 4, title: 'Chill', origin: 'manager', blockKey: 'D:/x/data.json' },
      { id: 5, title: 'https://youtu.be/x', origin: 'url', blockKey: null },
    ]);
  });

  it('shows the unseen result only while nothing runs', () => {
    expect(toTasksState({ running: [], queued: [], unseen: 'FAILED', history: [] }, NOW).unseenResult).toBe('failed');
    expect(toTasksState({ running: [{ id: 1, title: 'Lo-fi', origin: 'tasks.origin.url', step: 'download', fraction: 0.1, total: 1 }], queued: [], unseen: 'OK', history: [] }, NOW).unseenResult).toBeNull();
  });

  it('reads a history the Java app wrote, with fields missing or unknown', () => {
    const [item] = toTasksState({ running: [], queued: [], unseen: null, history: [{ status: 'WHATEVER' }] }, NOW).history;
    expect(item).toEqual({ title: '', origin: 'url', status: 'ok', summary: null, summaryArgs: [], finishedAt: '' });
  });
});

describe('progress', () => {
  it('has no percent while preparing', () => {
    expect(percent(running(null))).toBeNull();
    expect(percent(null)).toBeNull();
  });

  it('rounds the fraction and keeps it within 0–100', () => {
    expect(percent(running(0.426))).toBe(43);
    expect(percent(running(1.2))).toBe(100);
  });

  it('counts the file in progress from 1 and never past the total', () => {
    expect(fileNumber(running(0))).toBe(1);
    expect(fileNumber(running(0.5))).toBe(3);
    expect(fileNumber(running(1))).toBe(4);
  });
});

describe('shortStamp', () => {
  it('gives only the time for today and day/month before that', () => {
    expect(shortStamp('2026-11-03 09:05', NOW)).toBe('09:05');
    expect(shortStamp('2026-10-30 09:05', NOW)).toBe('30/10 09:05');
  });

  it('leaves an unknown shape as it came', () => {
    expect(shortStamp('ayer', NOW)).toBe('ayer');
    expect(shortStamp(null, NOW)).toBe('');
  });
});

describe('texts', () => {
  it('translates an app message and the arguments that are keys themselves', () => {
    const text = activityText({ kind: 'message', key: 'activity.retry.failedOne', args: ['Canción', 'failReason.geoBlocked'] });
    expect(text).toBe('❌ No se pudo descargar "Canción": está bloqueado en tu país');
  });

  it("shows yt-dlp's own lines as they came", () => {
    expect(activityText({ kind: 'raw', text: '[download] 50%' })).toBe('[download] 50%');
  });

  it('shows a summary the Java app already translated as it is', () => {
    const task: FinishedTask = { title: '', origin: 'url', status: 'ok', summary: 'Ya está todo', summaryArgs: [], finishedAt: '' };
    expect(summaryText(task)).toBe('Ya está todo');
    expect(summaryText({ ...task, summary: 'activity.manager.upToDate', summaryArgs: ['Lo-fi'] }))
      .toBe('✅ "Lo-fi" ya está al día — no había nada pendiente que descargar.');
  });

  it('picks the notice tone from the status', () => {
    expect(noticeTone({ status: 'OK', key: '', args: [] })).toBe('success');
    expect(noticeTone({ status: 'FAILED', key: '', args: [] })).toBe('error');
    expect(noticeTone({ status: 'WARNING', key: '', args: [] })).toBe('warning');
  });
});

describe('the Gestor row of a running download', () => {
  it('says what it is doing with that format and how far it goes', () => {
    expect(liveText(running(null), 'MP3')).toBe('Preparando MP3…');
    expect(liveText(running(0.5, 12), 'MP3')).toBe('Descargando MP3 · 7/12');
    expect(liveText({ ...running(1, 2), step: 'retry' }, 'OPUS')).toBe('Segunda pasada OPUS');
  });

  it('counts files over the bar, or says it is preparing before the counter exists', () => {
    expect(progressLine(running(null))).toBe('Preparando…');
    expect(progressLine(running(0, 3))).toBe('Archivo 1 de 3');
    expect(progressLine(running(1, 3))).toBe('Archivo 3 de 3');
  });
});

describe('pointsToSettings', () => {
  const notice = (key: string) => ({ status: 'FAILED', key, args: ['3'] });
  it('is true for the YouTube bot-check notices, with or without cookies', () => {
    for (const key of ['streakNotice', 'streakNoticeNoCookies', 'retryNotice', 'retryNoticeNoCookies']) {
      expect(pointsToSettings(notice(`activity.botcheck.${key}`))).toBe(true);
    }
  });
  it('is false for any other notice', () => {
    expect(pointsToSettings(notice('activity.botcheck.notice'))).toBe(false);
    expect(pointsToSettings(notice('download.error.isPlaylist'))).toBe(false);
  });
});

describe('overallPercent', () => {
  it('is the mean of the running tasks, and null while none knows how far it is', () => {
    expect(overallPercent([running(0.5), running(1)])).toBe(75);
    expect(overallPercent([running(null), running(null)])).toBeNull();
    // One still preparing counts as nothing done yet.
    expect(overallPercent([running(null), running(1)])).toBe(50);
    expect(overallPercent([])).toBeNull();
  });
});
