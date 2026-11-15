// The same diagnostic as Ajustes → Acerca de → «Copiar diagnóstico», but from the terminal: for whoever (Claude included) needs to
// look at what the app did without the user pressing anything. Reads the logs of the copy in `target/debug/data` (dev), of a data folder
// (`--data <carpeta>`, or SOUNDIFY_DATA_DIR) or of a phone connected by adb (`--adb [paquete]`, only builds that can be entered with
// `run-as`: the debug one, `com.dannygt97.soundify.debug`). Prints: version, Connect, the last statistics and warnings of the log,
// events per minute and the tail of the log. The user's folders are written as `~` and `<data>`.
//
//   node scripts/diagnostics.mjs [--data <carpeta>] [--lines 200] [--adb [paquete]]

import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');
const args = process.argv.slice(2);
const option = name => {
  const at = args.indexOf(name);
  return at < 0 ? null : (args[at + 1] && !args[at + 1].startsWith('--') ? args[at + 1] : true);
};
const lines = Number(option('--lines')) || 200;
const adb = option('--adb');

function adbPath() {
  const sdk = process.env.ANDROID_HOME || process.env.ANDROID_SDK_ROOT;
  const exe = process.platform === 'win32' ? 'adb.exe' : 'adb';
  return sdk && existsSync(join(sdk, 'platform-tools', exe)) ? join(sdk, 'platform-tools', exe) : 'adb';
}

function readRemote(pkg, file) {
  try {
    return execFileSync(adbPath(), ['shell', 'run-as', pkg, 'cat', `files/data/${file}`], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
  } catch (error) {
    throw new Error(`No se pudo leer ${file} del aparato (¿conectado por adb y con la build de depuración?): ${error.message.split('\n')[0]}`);
  }
}

let data = null;
let log;
let connectJson = null;
if (adb) {
  const pkg = typeof adb === 'string' ? adb : 'com.dannygt97.soundify.debug';
  log = readRemote(pkg, 'logs/soundify-0.log');
  try { connectJson = readRemote(pkg, 'connect/connect.json'); } catch { /* optional */ }
} else {
  data = resolve(option('--data') && option('--data') !== true ? option('--data') : process.env.SOUNDIFY_DATA_DIR || join(root, 'target', 'debug', 'data'));
  const file = join(data, 'logs', 'soundify-0.log');
  if (!existsSync(file)) {
    console.error(`No hay registro en ${file}. Usa --data <carpeta> o --adb.`);
    process.exit(1);
  }
  log = readFileSync(file, 'utf8');
  const connectFile = join(data, 'connect', 'connect.json');
  if (existsSync(connectFile)) connectJson = readFileSync(connectFile, 'utf8');
}

// The log writes the time on one line and the entry on the next.
const entries = [];
const raw = log.split(/\r?\n/);
for (let i = 0; i < raw.length; i++) {
  const stamp = raw[i].match(/^(\d\d)\/(\d\d)\/(\d{4}) (\d\d:\d\d:\d\d)/);
  if (stamp && raw[i + 1] !== undefined) {
    entries.push({ at: `${stamp[3]}-${stamp[2]}-${stamp[1]} ${stamp[4]}`, text: raw[i + 1] });
    i++;
  }
}

const version = (() => {
  try { return JSON.parse(readFileSync(join(root, 'src-tauri', 'tauri.conf.json'), 'utf8')).version; } catch { return '?'; }
})();
const out = [];
out.push('# Diagnóstico de Soundify (terminal)', `Versión del código: ${version}`, `Origen: ${adb ? 'aparato por adb' : 'carpeta de datos'}`, '');

out.push('## Connect');
if (connectJson) {
  try {
    const c = JSON.parse(connectJson);
    out.push(`enabled=${c.enabled} open=${c.open} name=${c.name} id=${String(c.deviceId).slice(0, 6)}`);
  } catch { out.push('connect.json ilegible'); }
}
const connectLines = entries.filter(e => e.text.includes('Soundify Connect'));
out.push(`Líneas de Connect en el registro: ${connectLines.length}`);
const perMinute = new Map();
for (const e of connectLines) {
  if (/ (IN|OUT) /.test(e.text) || /controls this device|connected to|ended|left/.test(e.text)) {
    const minute = e.at.slice(0, 16);
    perMinute.set(minute, (perMinute.get(minute) ?? 0) + 1);
  }
}
const busiest = [...perMinute.entries()].sort((a, b) => b[1] - a[1]).slice(0, 5);
out.push(`Minutos con más actividad de conexiones: ${busiest.map(([m, n]) => `${m} (${n})`).join(', ') || 'ninguno'}`);
out.push('', '### Últimos resúmenes y avisos de Connect y de la página');
const notable = entries.filter(e => /stats \(|more than usual|dropping|WARNING|SEVERE|\[page\]|\[console\]/.test(e.text) || /\bWARNING\b/.test(e.text));
for (const e of notable.slice(-20)) out.push(`${e.at} ${e.text}`);
if (!notable.length) out.push('(ninguno)');

out.push('', `## Final del registro (últimas ${lines} entradas)`);
for (const e of entries.slice(-lines)) out.push(`${e.at} ${e.text}`);

let text = out.join('\n');
if (data) text = text.split(data).join('<data>').split(data.replace(/\\/g, '/')).join('<data>');
const home = homedir();
if (home.length > 3) text = text.split(home).join('~').split(home.replace(/\\/g, '/')).join('~');
console.log(text);
