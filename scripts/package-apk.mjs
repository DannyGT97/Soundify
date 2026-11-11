// Packs an Android build into the three files a GitHub release carries for the
// phone, which are exactly what the app's updater asks for on Android (see
// core/src/updater/mod.rs, `Package::Apk`):
//
//   Soundify-<version>-tauri.apk         the APK
//   Soundify-<version>-tauri.apk.sha256  its SHA-256, as `sha256sum` writes it
//   Soundify-<version>-tauri.apk.sig     Ed25519 signature of the APK, in base64
//
//   npx tauri android build --debug --apk --target aarch64 --target x86_64
//   node scripts/package-apk.mjs --key <private key .pem> [--apk <file.apk>] [--out <folder>]
//
// They go in the same release as the Windows zip. Android only installs an
// update over the installed app when BOTH carry the same signing key (today the
// debug key) and the new versionCode is higher; the APK's own signature is not
// this one, which is the app's update key (core/keys/update-signing.pub).
//
// The key can also come from SOUNDIFY_SIGNING_KEY. It is only read here: never
// printed, never copied. Before signing, the signature is checked against the
// public key compiled into the app: a release signed with the wrong key would
// be refused by every installed copy.

import { createHash, createPrivateKey, createPublicKey, sign, verify } from 'node:crypto';
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const ROOT = join(import.meta.dirname, '..');
const version = JSON.parse(readFileSync(join(ROOT, 'src-tauri', 'tauri.conf.json'), 'utf8')).version;
const keyFile = argument('--key') ?? process.env.SOUNDIFY_SIGNING_KEY;
const out = argument('--out') ?? join(ROOT, 'release');
if (!keyFile) throw new Error('No signing key: pass --key <file.pem> or set SOUNDIFY_SIGNING_KEY');

const source = argument('--apk') ?? findApk();
if (!existsSync(source)) throw new Error(`${source} does not exist (build the APK first)`);

const name = `Soundify-${version}-tauri.apk`;
const apk = join(out, name);
mkdirSync(out, { recursive: true });
rmSync(apk, { force: true });
copyFileSync(source, apk);

const bytes = readFileSync(apk);
writeFileSync(`${apk}.sha256`, `${createHash('sha256').update(bytes).digest('hex')}  ${name}\n`);

const privateKey = createPrivateKey(readFileSync(keyFile));
const signature = sign(null, bytes, privateKey);
const publicKey = createPublicKey({ key: publicKeyDer(), format: 'der', type: 'spki' });
if (!verify(null, bytes, publicKey, signature)) {
  rmSync(apk, { force: true });
  rmSync(`${apk}.sha256`, { force: true });
  throw new Error('That key is not the one the app trusts: nothing was published');
}
writeFileSync(`${apk}.sig`, signature.toString('base64'));
console.log(`Ready in ${out}: ${name} (+ .sha256, .sig)`);

function argument(flag) {
  const at = process.argv.indexOf(flag);
  return at === -1 ? undefined : process.argv[at + 1];
}

/** The newest .apk under Gradle's output folder. */
function findApk() {
  const base = join(ROOT, 'src-tauri', 'gen', 'android', 'app', 'build', 'outputs', 'apk');
  const found = [];
  const walk = folder => {
    if (!existsSync(folder)) return;
    for (const entry of readdirSync(folder, { withFileTypes: true })) {
      const path = join(folder, entry.name);
      if (entry.isDirectory()) walk(path);
      else if (entry.name.endsWith('.apk')) found.push(path);
    }
  };
  walk(base);
  if (!found.length) throw new Error(`No .apk under ${base}: build it or pass --apk <file>`);
  return found.sort((a, b) => statSync(b).mtimeMs - statSync(a).mtimeMs)[0];
}

/** The app's public key: base64 DER, or PEM, whichever the file holds. */
function publicKeyDer() {
  const text = readFileSync(join(ROOT, 'core', 'keys', 'update-signing.pub'), 'utf8');
  return Buffer.from(text.replace(/-----[^-]+-----/g, '').replace(/\s/g, ''), 'base64');
}
