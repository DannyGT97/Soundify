// Builds the Android RELEASE apk signed with the project's own keystore (the one the app's
// updates depend on: see CLAUDE.md, «Actualizar la app en Android»). The keystore lives outside
// the repository; only the path of its properties file is passed here:
//
//   node scripts/android-release-build.mjs --keystore-props "<soundify-android.properties>" [--target aarch64 --target x86_64]
//
// (or SOUNDIFY_ANDROID_KEYSTORE_PROPS). It needs ANDROID_HOME and NDK_HOME. Gradle reads the
// properties file itself: this script never opens it. Afterwards the apk is checked to be
// signed and NOT with the debug key, and its certificate fingerprint is printed.
// Then: node scripts/package-apk.mjs --key "<update signing key>"

import { execFileSync, spawnSync } from 'node:child_process';
import { existsSync, readdirSync, rmSync } from 'node:fs';
import { join } from 'node:path';

const ROOT = join(import.meta.dirname, '..');
const props = argument('--keystore-props') ?? process.env.SOUNDIFY_ANDROID_KEYSTORE_PROPS;
if (!props || !existsSync(props)) throw new Error('No keystore: pass --keystore-props <file> or set SOUNDIFY_ANDROID_KEYSTORE_PROPS');
const sdk = process.env.ANDROID_HOME;
if (!sdk || !process.env.NDK_HOME) throw new Error('Set ANDROID_HOME and NDK_HOME first');

const targets = argumentsOf('--target');
const apkFolder = join(ROOT, 'src-tauri', 'gen', 'android', 'app', 'build', 'outputs', 'apk');
// A previous build (debug included) must not be mistaken for this one.
rmSync(apkFolder, { recursive: true, force: true });

const built = spawnSync('npx', ['tauri', 'android', 'build', '--apk', ...(targets.length ? targets : ['aarch64', 'x86_64']).flatMap(t => ['--target', t])], {
  cwd: ROOT, stdio: 'inherit', shell: true, env: { ...process.env, SOUNDIFY_ANDROID_KEYSTORE_PROPS: props },
});
if (built.status !== 0) throw new Error('The Android build failed');

const apk = join(apkFolder, 'universal', 'release', 'app-universal-release.apk');
if (!existsSync(apk)) throw new Error(`${apk} was not produced (an unsigned one is named ...-unsigned.apk: the keystore was not applied)`);

// The newest build-tools has apksigner.
const tools = join(sdk, 'build-tools');
const newest = readdirSync(tools).sort((a, b) => a.localeCompare(b, undefined, { numeric: true })).pop();
const apksigner = join(tools, newest, process.platform === 'win32' ? 'apksigner.bat' : 'apksigner');
const report = execFileSync(apksigner, ['verify', '--print-certs', apk], { encoding: 'utf8', shell: true });
const fingerprint = /certificate SHA-256 digest: (\w+)/.exec(report)?.[1];
if (!fingerprint) throw new Error('apksigner did not verify the apk');
if (/Android Debug/i.test(report)) throw new Error('The apk is signed with the DEBUG key: the keystore was not applied');
console.log(`OK ${apk}\nSigned; certificate SHA-256 ${fingerprint}\nNow: node scripts/package-apk.mjs --key "<ruta>"`);

function argument(flag) {
  const at = process.argv.indexOf(flag);
  return at === -1 ? undefined : process.argv[at + 1];
}

function argumentsOf(flag) {
  return process.argv.flatMap((value, at) => (value === flag ? [process.argv[at + 1]] : []));
}
