// Packs a release build into the three files a GitHub release carries, which
// are exactly what the app's updater asks for (see core/src/updater/mod.rs):
//
//   Soundify-<version>-tauri.zip         the portable: Soundify/Soundify.exe + tools/ffmpeg/
//   Soundify-<version>-tauri.zip.sha256  its SHA-256, as `sha256sum` writes it
//   Soundify-<version>-tauri.zip.sig     Ed25519 signature of the zip, in base64
//
//   npm run tauri build -- --no-bundle
//   node scripts/package-portable.mjs --key <private key .pem> [--out <folder>]
//
// The key can also come from SOUNDIFY_SIGNING_KEY. It is only read here: never
// printed, never copied. Before signing, the signature is checked against the
// public key compiled into the app (core/keys/update-signing.pub): a release
// signed with the wrong key would be refused by every installed copy.

import { createHash, createPrivateKey, createPublicKey, sign, verify } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';

const ROOT = join(import.meta.dirname, '..');
const version = JSON.parse(readFileSync(join(ROOT, 'src-tauri', 'tauri.conf.json'), 'utf8')).version;
const keyFile = argument('--key') ?? process.env.SOUNDIFY_SIGNING_KEY;
const out = argument('--out') ?? join(ROOT, 'release');
if (!keyFile) throw new Error('No signing key: pass --key <file.pem> or set SOUNDIFY_SIGNING_KEY');

const exe = join(ROOT, 'target', 'release', 'soundify.exe');
const ffmpeg = join(ROOT, 'src-tauri', 'resources', 'ffmpeg');
for (const needed of [exe, join(ffmpeg, 'ffmpeg.exe'), join(ffmpeg, 'ffprobe.exe')]) {
  if (!existsSync(needed)) throw new Error(`${needed} does not exist (build first, and fetch ffmpeg)`);
}

// The same layout the bundle's resources give: tools/ffmpeg/ next to the exe.
const stage = join(tmpdir(), `soundify-portable-${process.pid}`);
const app = join(stage, 'Soundify');
const tools = join(app, 'tools', 'ffmpeg');
rmSync(stage, { recursive: true, force: true });
mkdirSync(tools, { recursive: true });
copyFileSync(exe, join(app, 'Soundify.exe'));
for (const name of ['ffmpeg.exe', 'ffprobe.exe', 'LICENSE']) {
  if (existsSync(join(ffmpeg, name))) copyFileSync(join(ffmpeg, name), join(tools, name));
}

const name = `Soundify-${version}-tauri.zip`;
const zip = join(out, name);
mkdirSync(out, { recursive: true });
rmSync(zip, { force: true });
// Windows' own bsdtar: `-a` picks the format from the extension.
execFileSync(join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe'), ['-a', '-c', '-f', zip, '-C', stage, 'Soundify']);
rmSync(stage, { recursive: true, force: true });

const bytes = readFileSync(zip);
writeFileSync(`${zip}.sha256`, `${createHash('sha256').update(bytes).digest('hex')}  ${name}\n`);

const privateKey = createPrivateKey(readFileSync(keyFile));
const signature = sign(null, bytes, privateKey);
const publicKey = createPublicKey({ key: publicKeyDer(), format: 'der', type: 'spki' });
if (!verify(null, bytes, publicKey, signature)) {
  rmSync(zip, { force: true });
  rmSync(`${zip}.sha256`, { force: true });
  throw new Error('That key is not the one the app trusts: nothing was published');
}
writeFileSync(`${zip}.sig`, signature.toString('base64'));
console.log(`Ready in ${out}: ${name} (+ .sha256, .sig)`);

function argument(flag) {
  const at = process.argv.indexOf(flag);
  return at === -1 ? undefined : process.argv[at + 1];
}

/** The app's public key: base64 DER, or PEM, whichever the file holds. */
function publicKeyDer() {
  const text = readFileSync(join(ROOT, 'core', 'keys', 'update-signing.pub'), 'utf8');
  return Buffer.from(text.replace(/-----[^-]+-----/g, '').replace(/\s/g, ''), 'base64');
}
