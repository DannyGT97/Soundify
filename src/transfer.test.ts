import { describe, expect, it } from 'vitest';
import type { CopyOutcome, ExportResult, ImportCandidate, ImportOutcome } from './api/types';
import {
  BULLET_LIMIT, bulletList, copySummary, errorText, folderName, importSummary, nothingFound, nothingNew,
} from './transfer';

function copy(result: Partial<ExportResult>, register: CopyOutcome['register'] = 'notRequested'): CopyOutcome {
  return {
    result: { copied: 0, skipped: 0, missing: 0, cancelled: false, undone: 0, ...result },
    folder: 'E:\\Copia\\Lo-fi (mp3)',
    register,
  };
}

function imported(title: string, pending = 0): ImportOutcome {
  return {
    folder: `D:\\${title}`,
    result: { title, format: 'mp3', total: 10, downloaded: 10 - pending, pending },
    error: null,
    alreadyRegistered: false,
  };
}

function failed(key = 'transfer.driveRoot', args: string[] = []): ImportOutcome {
  return { folder: 'D:\\x', result: null, error: { key, args }, alreadyRegistered: false };
}

function alreadyRegistered(): ImportOutcome {
  return { ...failed('transfer.alreadyRegistered', ['Lo-fi', 'mp3']), alreadyRegistered: true };
}

describe('copySummary', () => {
  it('a cancelled copy is an error and says how many files were undone', () => {
    const message = copySummary(copy({ cancelled: true, undone: 4 }, 'notRequested'));
    expect(message.tone).toBe('error');
    expect(message.text).toContain('(4 archivo(s))');
  });

  it('a normal copy names the destination and only the counts that are not zero', () => {
    const message = copySummary(copy({ copied: 7 }));
    expect(message.tone).toBe('success');
    expect(message.text).toContain('7 archivo(s) copiado(s). En E:\\Copia\\Lo-fi (mp3)');
    expect(message.text).not.toContain('ya estaban');
    expect(message.text).not.toContain('sin archivo');
  });

  it('skipped and missing files are added to the same sentence', () => {
    const text = copySummary(copy({ copied: 2, skipped: 3, missing: 1 })).text;
    expect(text).toContain('2 archivo(s) copiado(s), 3 ya estaban, 1 sin archivo.');
  });

  it('copying again over a full copy says so instead of "0 copied", as a caution', () => {
    const message = copySummary(copy({ skipped: 5 }));
    expect(message.tone).toBe('warning');
    expect(message.text).toContain('ya está copiada en E:\\Copia\\Lo-fi (mp3)');
  });

  it('the register outcome is appended, and only a clean one keeps the success tone', () => {
    expect(copySummary(copy({ copied: 1 }, 'registered'))).toMatchObject({ tone: 'success' });
    expect(copySummary(copy({ copied: 1 }, 'registered')).text).toMatch(/Registrada en el Gestor\.$/);
    expect(copySummary(copy({ copied: 1 }, 'alreadyRegistered')).tone).toBe('warning');
    expect(copySummary(copy({ copied: 1 }, 'failed')).tone).toBe('warning');
    expect(copySummary(copy({ copied: 1 }, 'failed')).text).toContain('no se ha podido registrar');
  });
});

describe('bulletList', () => {
  const candidate = (n: number): ImportCandidate => ({ folder: `D:\\${n}`, title: `Lista ${n}`, format: 'mp3' });

  it('one line per playlist, with its format in capitals', () => {
    expect(bulletList([candidate(1), candidate(2)])).toBe('\n  • Lista 1 (MP3)\n  • Lista 2 (MP3)');
  });

  it(`stops at ${BULLET_LIMIT} and counts the rest`, () => {
    const many = Array.from({ length: BULLET_LIMIT + 3 }, (_, i) => candidate(i));
    const text = bulletList(many);
    expect(text.split('•').length - 1).toBe(BULLET_LIMIT + 1);
    expect(text).toMatch(/…y 3 más$/);
  });

  it('exactly the limit has no "and more" line', () => {
    const exact = Array.from({ length: BULLET_LIMIT }, (_, i) => candidate(i));
    expect(bulletList(exact)).not.toContain('más');
  });
});

describe('importSummary', () => {
  it('a single playlist is described in detail', () => {
    const message = importSummary([imported('Lo-fi', 2)], 0);
    expect(message.tone).toBe('success');
    expect(message.text).toContain('"Lo-fi" (mp3) importada: 8 de 10 archivo(s)');
    expect(message.text).toContain('quedan 2 pendiente(s)');
  });

  it('a complete single playlist says nothing is missing, and counts the omitted ones', () => {
    const text = importSummary([imported('Lo-fi')], 3).text;
    expect(text).toContain('. No falta ninguno.');
    expect(text).toContain('(3 más ya estaban registradas.)');
  });

  it('several are summarised as a count, with the pending total', () => {
    const message = importSummary([imported('A', 1), imported('B', 2)], 0);
    expect(message.tone).toBe('success');
    expect(message.text).toBe('2 playlist(s) importadas. Quedan 3 archivo(s) pendiente(s) en total.');
  });

  it('one failing among several turns the message into an error but still counts the rest', () => {
    const message = importSummary([imported('A'), failed()], 0);
    expect(message.tone).toBe('error');
    expect(message.text).toBe('1 playlist(s) importadas, 1 con problemas.');
  });

  it('one registered between the scan and the OK counts as already known, not as a failure', () => {
    const message = importSummary([imported('A'), alreadyRegistered()], 1);
    expect(message.tone).toBe('success');
    expect(message.text).toContain(', 2 ya estaban registradas');
  });

  it('none imported shows the first error, translated', () => {
    const message = importSummary([failed('transfer.folderMissing', ['Lo-fi (mp3)'])], 0);
    expect(message.tone).toBe('error');
    expect(message.text).toContain('Falta la carpeta "Lo-fi (mp3)"');
  });
});

describe('scan messages', () => {
  it('nothing found names the folder and the manifest', () => {
    const message = nothingFound('Música');
    expect(message.tone).toBe('error');
    expect(message.text).toContain('En "Música"');
    expect(message.text).toContain('soundify-playlist.json');
  });

  it('nothing new tells one from many', () => {
    expect(nothingNew(1).text).toContain('esa playlist ya estaba registrada');
    expect(nothingNew(4).text).toContain('las 4 playlists');
  });
});

describe('errorText', () => {
  it('translates an error from the back', () => {
    expect(errorText({ key: 'transfer.notEnoughSpace', args: ['2 GB', '1 GB'] }))
      .toBe('No hay espacio suficiente en el destino: hacen falta 2 GB y quedan 1 GB.');
  });

  it('falls back to the message of anything else', () => {
    expect(errorText(new Error('boom'))).toBe('boom');
    expect(errorText('plain')).toBe('plain');
  });
});

describe('folderName', () => {
  it('takes the last part of either kind of path', () => {
    expect(folderName('D:\\Música\\Lo-fi\\')).toBe('Lo-fi');
    expect(folderName('/home/u/Música')).toBe('Música');
  });
});
