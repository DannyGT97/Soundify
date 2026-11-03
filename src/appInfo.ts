// The app's real version (the Java `AppInfo`), read once before the first
// screen. It was written by hand in the dictionary ("v1.4"), which is exactly
// what the Java app's regression forbids: a version that says something other
// than the build, or "null", is worse than saying it is not known.

import { api } from './api';
import { t } from './i18n';

let version: string | null = null;

export async function loadAppInfo(): Promise<void> {
  version = await api.appVersion().catch(() => null);
}

/** "v1.4.0", or "desconocida" when the build did not say. */
export function appVersion(): string {
  return versionLabel(version);
}

/** Never "vnull", "v${project.version}" or a blank chip. */
export function versionLabel(raw: string | null | undefined): string {
  const clean = raw?.trim().replace(/^v/i, '') ?? '';
  return /^\d+(\.\d+)*([-+][\w.-]+)?$/.test(clean) ? `v${clean}` : t('app.unknownVersion');
}
