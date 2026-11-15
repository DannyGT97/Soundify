// Copia el instalador de Windows que dejó `node scripts/release-build.mjs` (NSIS, por usuario) a `release/` con el nombre de la release y su SHA-256:
//
//   Soundify-<version>-setup.exe         el instalador (se instala en %LOCALAPPDATA%\Programs, que se puede escribir: la app guarda `data/` junto al .exe)
//   Soundify-<version>-setup.exe.sha256  su SHA-256, como lo escribe `sha256sum`
//
// No lo usa el actualizador de la app (ese solo toma el ZIP/APK firmados): es para instalar a mano. Sin la palabra del stack en ningún nombre.
//
//   node scripts/package-installer.mjs

import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const ROOT = resolve(import.meta.dirname, '..');
const version = JSON.parse(readFileSync(join(ROOT, 'src-tauri', 'tauri.conf.json'), 'utf8')).version;
const folder = join(ROOT, 'target', 'release', 'bundle', 'nsis');
const built = existsSync(folder) ? readdirSync(folder).find(name => name.endsWith('-setup.exe') && name.includes(`_${version}_`)) : undefined;
if (!built) {
  console.error(`No hay instalador de la ${version} en ${folder}: ejecuta antes \`node scripts/release-build.mjs\`.`);
  process.exit(1);
}

const out = join(ROOT, 'release');
mkdirSync(out, { recursive: true });
const name = `Soundify-${version}-setup.exe`;
copyFileSync(join(folder, built), join(out, name));
const hash = createHash('sha256').update(readFileSync(join(out, name))).digest('hex');
writeFileSync(join(out, `${name}.sha256`), `${hash}  ${name}\n`);
console.log(`Ready in ${out}: ${name} (+ .sha256)`);
