import { describe, expect, it } from 'vitest';
import {
  crumbsOf, daysSince, driveFor, driveLetter, isInside, remember, samePath, toggled, filterEntries, hasName, joinPath, kindOf, parentPath, playlistFolderName, playlistFromName, sizeText, sortEntries,
  splitPath, validFolderName, type ExplorerEntry,
} from './explorerLogic';

const folder = (name: string, extra: Partial<ExplorerEntry> = {}): ExplorerEntry => ({ name, isFolder: true, sizeBytes: 0, ...extra });
const file = (name: string, sizeBytes = 0, extra: Partial<ExplorerEntry> = {}): ExplorerEntry => ({ name, isFolder: false, sizeBytes, ...extra });

describe('paths', () => {
  it('splits a Windows path and a Unix one', () => {
    expect(splitPath('D:\\Música\\Soundify')).toEqual({ root: 'D:\\', parts: ['Música', 'Soundify'] });
    expect(splitPath('C:\\')).toEqual({ root: 'C:\\', parts: [] });
    expect(splitPath('/home/ana/Music')).toEqual({ root: '/', parts: ['home', 'ana', 'Music'] });
  });

  it('goes up one folder and stops at the root', () => {
    expect(parentPath('D:\\Música\\Soundify')).toBe('D:\\Música');
    expect(parentPath('D:\\Música')).toBe('D:\\');
    expect(parentPath('D:\\')).toBeNull();
    expect(parentPath('/home/ana')).toBe('/home');
    expect(parentPath('/home')).toBe('/');
    expect(parentPath('/')).toBeNull();
  });

  it('joins with the separator the path already uses', () => {
    expect(joinPath('D:\\Música', 'Rock')).toBe('D:\\Música\\Rock');
    expect(joinPath('D:\\', 'Rock')).toBe('D:\\Rock');
    expect(joinPath('/home/ana', 'Rock')).toBe('/home/ana/Rock');
    expect(joinPath('/', 'tmp')).toBe('/tmp');
  });

  it('draws the way from the root as steps that lead back', () => {
    expect(crumbsOf('D:\\Música\\Soundify')).toEqual([
      { label: 'D:', path: 'D:\\' },
      { label: 'Música', path: 'D:\\Música' },
      { label: 'Soundify', path: 'D:\\Música\\Soundify' },
    ]);
    expect(crumbsOf('/home/ana').map(c => c.path)).toEqual(['/', '/home', '/home/ana']);
  });

  it('recognises a drive', () => {
    expect(driveLetter('c:\\')).toBe('C:');
    expect(driveLetter('D:')).toBe('D:');
    expect(driveLetter('D:\\Música')).toBeNull();
  });
});

describe('where a path is', () => {
  it('compares paths the way the system does', () => {
    expect(samePath('D:\\Música\\', 'd:/música')).toBe(true);
    expect(samePath('/Music', '/music')).toBe(false);
    expect(samePath('/', '/')).toBe(true);
  });

  it('knows what is inside a folder, and not a lookalike', () => {
    expect(isInside('D:\\Música\\Rock', 'D:\\')).toBe(true);
    expect(isInside('D:\\Música', 'D:\\Música')).toBe(true);
    expect(isInside('D:\\MúsicaX', 'D:\\Música')).toBe(false);
    expect(isInside('/storage/emulated/0/Music', '/storage/emulated/0')).toBe(true);
    expect(isInside('/storage/emulated/01/Music', '/storage/emulated/0')).toBe(false);
    expect(isInside('/anything', '/')).toBe(true);
  });

  it('finds the disk a path is on, the deepest one when they nest', () => {
    const drives = [{ path: '/', id: 'root' }, { path: '/storage/3A1F-0C22', id: 'sd' }, { path: '/storage/emulated/0', id: 'internal' }];
    expect(driveFor('/storage/3A1F-0C22/Music', drives)?.id).toBe('sd');
    expect(driveFor('/storage/emulated/0/Music', drives)?.id).toBe('internal');
    expect(driveFor('/etc', drives)?.id).toBe('root');
    expect(driveFor('C:\\x', [{ path: 'D:\\' }])).toBeUndefined();
  });

  it('keeps the recent folders newest first, once each, up to the limit', () => {
    expect(remember(['/a', '/b', '/c'], '/b', 3)).toEqual(['/b', '/a', '/c']);
    expect(remember(['/a', '/b', '/c'], '/d', 3)).toEqual(['/d', '/a', '/b']);
    expect(remember([], 'D:\\x', 3)).toEqual(['D:\\x']);
  });

  it('stars a folder and unstars it', () => {
    expect(toggled(['/a'], '/b')).toEqual(['/a', '/b']);
    expect(toggled(['/a', '/b'], '/a')).toEqual(['/b']);
  });
});

describe('the list', () => {
  it('puts folders first and orders numbers as numbers', () => {
    const sorted = sortEntries([file('10 - b.mp3'), file('9 - a.mp3'), folder('Zeta'), folder('alfa')], 'name');
    expect(sorted.map(e => e.name)).toEqual(['alfa', 'Zeta', '9 - a.mp3', '10 - b.mp3']);
  });

  it('orders by date, newest first, and by size, biggest first; folders keep their name order', () => {
    const items = [
      file('a', 5, { modified: '2026-01-01T10:00:00' }),
      file('b', 50, { modified: '2026-03-01T10:00:00' }),
      folder('Y', { modified: '2026-01-01T00:00:00' }),
      folder('X', { modified: '2026-05-01T00:00:00' }),
    ];
    expect(sortEntries(items, 'date').map(e => e.name)).toEqual(['X', 'Y', 'b', 'a']);
    expect(sortEntries(items, 'size').map(e => e.name)).toEqual(['X', 'Y', 'b', 'a']);
  });

  it('filters ignoring accents and capitals, and finds a playlist by its title', () => {
    const items = [folder('Música'), file('Cañón.mp3'), folder('Lo-fi (mp3)', { playlist: { title: 'Para estudiar', format: 'mp3' } })];
    expect(filterEntries(items, 'musica').map(e => e.name)).toEqual(['Música']);
    expect(filterEntries(items, 'CANON').map(e => e.name)).toEqual(['Cañón.mp3']);
    expect(filterEntries(items, 'estudiar').map(e => e.name)).toEqual(['Lo-fi (mp3)']);
    expect(filterEntries(items, '  ')).toHaveLength(3);
  });

  it('knows what a file is from its name', () => {
    expect(kindOf(folder('x.mp3'))).toBe('folder');
    expect(kindOf(file('A.JPG'))).toBe('image');
    expect(kindOf(file('a.flac'))).toBe('audio');
    expect(kindOf(file('a.mkv'))).toBe('video');
    expect(kindOf(file('cookies.txt'))).toBe('text');
    expect(kindOf(file('.gitignore'))).toBe('file');
    expect(kindOf(file('README'))).toBe('file');
  });
});

describe('texts', () => {
  it('writes sizes', () => {
    expect(sizeText(0)).toBe('');
    expect(sizeText(900)).toBe('900 B');
    expect(sizeText(2048)).toBe('2 KB');
    expect(sizeText(4_200_000)).toBe('4.0 MB');
    expect(sizeText(5 * 1024 ** 3)).toBe('5.0 GB');
  });

  it('counts days between dates, and says nothing about a bad one', () => {
    const now = new Date(2026, 10, 17, 12, 0, 0);
    expect(daysSince('2026-11-17T01:00:00', now)).toBe(0);
    expect(daysSince('2026-11-16T23:59:00', now)).toBe(1);
    expect(daysSince('2026-11-10T08:00:00', now)).toBe(7);
    expect(daysSince('2026-12-01T08:00:00', now)).toBe(0);
    expect(daysSince(null, now)).toBeNull();
    expect(daysSince('not a date', now)).toBeNull();
  });
});

describe('folders', () => {
  it('accepts names Windows accepts', () => {
    for (const good of ['Música', 'Lo-fi (mp3)', ' con espacios ', 'a.b']) expect(validFolderName(good)).toBe(true);
    for (const bad of ['', '  ', '.', '..', 'a/b', 'a\\b', 'a:b', 'a*b', 'a?b', 'a"b', 'a<b', 'a|b', 'punto.']) expect(validFolderName(bad)).toBe(false);
  });

  it('finds a name already there regardless of capitals', () => {
    expect(hasName([folder('Rock')], ' rock ')).toBe(true);
    expect(hasName([folder('Rock')], 'Pop')).toBe(false);
  });

  it('names a playlist folder the way Soundify does and reads it back', () => {
    expect(playlistFolderName('Lo-fi', 'MP3')).toBe('Lo-fi (mp3)');
    expect(playlistFromName('Lo-fi (mp3)')).toEqual({ title: 'Lo-fi', format: 'MP3' });
    expect(playlistFromName('Música')).toBeNull();
  });
});
