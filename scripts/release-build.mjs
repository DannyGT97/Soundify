// Compila la release (`tauri build --no-bundle`) sin que el .exe lleve rutas de este equipo.
//
// rustc deja en el binario las rutas de los fuentes que compila (el registro de cargo, la
// biblioteca estándar y el propio repositorio), y esas rutas llevan el nombre de usuario de
// Windows. `--remap-path-prefix` las sustituye por otras genéricas. Cambiar RUSTFLAGS fuerza una
// compilación completa, así que conviene usar siempre este script y no mezclarlo con `tauri build` a mano.
//
//   node scripts/release-build.mjs
//
// Luego: `node scripts/package-portable.mjs --key "<ruta de la clave>"`.

import { spawnSync } from 'node:child_process';
import { homedir } from 'node:os';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const cargoHome = process.env.CARGO_HOME || join(homedir(), '.cargo');
const rustupHome = process.env.RUSTUP_HOME || join(homedir(), '.rustup');

const remaps = [
  // El más general primero: si dos casan, rustc usa el último.
  [homedir(), '/home'],
  [cargoHome, '/cargo'],
  [rustupHome, '/rustup'],
  [root, '/src'],
].map(([from, to]) => `--remap-path-prefix=${from}=${to}`);

const rustflags = [process.env.RUSTFLAGS, ...remaps].filter(Boolean).join(' ');
const run = spawnSync('npm', ['run', 'tauri', 'build', '--', '--no-bundle'], {
  cwd: root,
  stdio: 'inherit',
  shell: true,
  env: { ...process.env, RUSTFLAGS: rustflags },
});
process.exit(run.status ?? 1);
