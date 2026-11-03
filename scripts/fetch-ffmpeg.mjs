// Puts the ffmpeg.exe and ffprobe.exe that travel with the app into
// src-tauri/resources/ffmpeg/ (the Java app kept them in
// src/main/resources/tools/ffmpeg/bin/). They are not in git: ~200 MB.
//
//   node scripts/fetch-ffmpeg.mjs                 downloads gyan.dev's essentials build
//   node scripts/fetch-ffmpeg.mjs --from <folder> copies them from a folder that has them
//
// A build without them still works: the app downloads ffmpeg on its first start.

import { copyFileSync, createWriteStream, existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { execFileSync } from 'node:child_process';

const ZIP_URL = 'https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip';
const TARGET = join(import.meta.dirname, '..', 'src-tauri', 'resources', 'ffmpeg');
const BINARIES = ['ffmpeg.exe', 'ffprobe.exe'];

mkdirSync(TARGET, { recursive: true });
const from = process.argv.indexOf('--from');
if (from !== -1) {
  copyFrom(process.argv[from + 1]);
} else {
  await download();
}
console.log(`ffmpeg ready in ${TARGET}`);

function copyFrom(folder) {
  for (const name of BINARIES) {
    const source = join(folder, name);
    if (!existsSync(source)) throw new Error(`${source} does not exist`);
    copyFileSync(source, join(TARGET, name));
  }
}

async function download() {
  const work = mkdtempSync(join(tmpdir(), 'soundify-ffmpeg-'));
  try {
    const zip = join(work, 'ffmpeg.zip');
    console.log(`Downloading ${ZIP_URL} (~115 MB, the server is slow)...`);
    const response = await fetch(ZIP_URL);
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    await pipeline(Readable.fromWeb(response.body), createWriteStream(zip));
    // bsdtar, which ships with Windows 10 and later, reads zips.
    execFileSync('tar', ['-xf', zip, '-C', work]);
    copyFrom(findBin(work));
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

/** The zip's folder carries the version number: look for the `bin` that holds ffmpeg.exe. */
function findBin(folder) {
  for (const entry of readdirSync(folder)) {
    const path = join(folder, entry);
    if (!statSync(path).isDirectory()) continue;
    if (entry === 'bin' && existsSync(join(path, 'ffmpeg.exe'))) return path;
    const found = findBin(path);
    if (found) return found;
  }
  return null;
}
