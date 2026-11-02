// What "Copiar playlist" and "Importar" tell the user when they finish: pure
// text and tone, so it is tested without a window (the Java app built these
// inline in PlaylistManagerController).

import { t } from './i18n';
import type { ApiError, CopyOutcome, ImportCandidate, ImportOutcome } from './api/types';

export type Tone = 'success' | 'warning' | 'error';

export interface Message {
  tone: Tone;
  text: string;
}

/** How many playlists the import confirmation names before "…y N más". */
export const BULLET_LIMIT = 12;

/** The manifest every playlist folder carries; named in "nothing found". */
export const MANIFEST_NAME = 'soundify-playlist.json';

/** The text of a failed command: its i18n key, or the raw message of an unexpected one. */
export function errorText(error: unknown): string {
  if (isApiError(error)) return t(error.key, ...error.args);
  if (error instanceof Error) return error.message;
  return String(error);
}

function isApiError(value: unknown): value is ApiError {
  return typeof value === 'object' && value !== null
    && typeof (value as ApiError).key === 'string' && Array.isArray((value as ApiError).args);
}

/** The outcome of a copy, with the folder it left. */
export function copySummary(outcome: CopyOutcome): Message {
  const { result, register } = outcome;
  if (result.cancelled) return { tone: 'error', text: t('manager.copy.cancelled', result.undone) };

  // Copying again over a previous copy: nothing new, and saying "0 copied" read as a failure.
  const allAlreadyThere = result.copied === 0 && result.missing === 0 && result.skipped > 0;
  const base = allAlreadyThere
    ? t('manager.copy.alreadyThere', outcome.folder)
    : t('manager.copy.done', result.copied,
        result.skipped > 0 ? t('manager.copy.doneSkipped', result.skipped) : '',
        result.missing > 0 ? t('manager.copy.doneMissing', result.missing) : '',
        outcome.folder);
  const registerText = register === 'registered' ? t('manager.copy.registered')
    : register === 'alreadyRegistered' ? t('manager.copy.alreadyRegistered')
    : register === 'failed' ? t('manager.copy.registerFailed')
    : '';
  const caution = allAlreadyThere || register === 'alreadyRegistered' || register === 'failed';
  return { tone: caution ? 'warning' : 'success', text: registerText ? `${base} ${registerText}` : base };
}

/** The list of the import confirmation: one line per playlist, capped at `BULLET_LIMIT`. */
export function bulletList(candidates: ImportCandidate[]): string {
  const shown = candidates.slice(0, BULLET_LIMIT)
    .map(c => `\n  • ${c.title} (${c.format.toUpperCase()})`).join('');
  const rest = candidates.length - BULLET_LIMIT;
  return rest > 0 ? shown + t('manager.import.andMore', rest) : shown;
}

/** The scan found no playlist at all under the chosen folder. */
export function nothingFound(folderName: string): Message {
  return { tone: 'error', text: t('manager.import.nothingFound', folderName, MANIFEST_NAME) };
}

/** Every playlist the scan found was already registered. */
export function nothingNew(found: number): Message {
  return {
    tone: 'success',
    text: found === 1 ? t('manager.import.nothingNew.one') : t('manager.import.nothingNew.many', found),
  };
}

/** The outcome of importing several folders; `omitted` were filtered out as already registered. */
export function importSummary(outcomes: ImportOutcome[], omitted: number): Message {
  const imported = outcomes.flatMap(o => (o.result ? [o.result] : []));
  // One registered between the scan and the "OK" is added to the ones already
  // known instead of being counted as a failure.
  const already = omitted + outcomes.filter(o => o.alreadyRegistered).length;
  const failed = outcomes.filter(o => !o.result && !o.alreadyRegistered);

  // A single playlist is summarised in detail (how many files are already
  // there and how many are missing); several, as a count — nobody reads the
  // full list of figures for eight playlists.
  if (outcomes.length === 1 && imported.length === 1) {
    const r = imported[0]!;
    return {
      tone: 'success',
      text: t('manager.import.one', r.title, r.format, r.downloaded, r.total,
        r.pending > 0 ? ` ${t('manager.import.onePending', r.pending)}` : t('manager.import.oneComplete'),
        already > 0 ? ` ${t('manager.import.oneAlready', already)}` : ''),
    };
  }
  if (imported.length === 0) {
    const first = outcomes[0]?.error;
    return { tone: 'error', text: t('manager.import.noneImported', first ? errorText(first) : '') };
  }
  const pending = imported.reduce((n, r) => n + r.pending, 0);
  return {
    tone: failed.length > 0 ? 'error' : 'success',
    text: t('manager.import.summary', imported.length,
      already > 0 ? t('manager.import.summaryAlready', already) : '',
      failed.length > 0 ? t('manager.import.summaryFailed', failed.length) : '',
      pending > 0 ? t('manager.import.summaryPending', pending) : '.'),
  };
}

/** The last part of a path, for the messages: "D:\Música\Lo-fi" → "Lo-fi". */
export function folderName(path: string): string {
  const parts = path.split(/[\\/]+/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}
