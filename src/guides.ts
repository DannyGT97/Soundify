// The first-use guides of the app (what each says is in `locales`, `guide.*`): where each one is shown is in its own place
// (the first start in `main.ts`, the first Dispositivos in its windows, the first pass in the devices list...). The window
// is `overlays/guide.ts`; every id is shown once, and Ajustes → «Restablecer avisos y guías» brings them back.

import type { GuideOptions } from './overlays/guide';
import { t } from './i18n';

/** The ids of every guide (the mock starts with all of them seen, so the tests do not meet one by surprise: `?mockGuides`). */
export const GUIDE_IDS = ['welcome', 'android', 'connect', 'pass', 'listen', 'remote'] as const;

/** The first start: what the app is for, in three steps. */
export function welcomeGuide(after?: () => void): GuideOptions {
  return {
    id: 'welcome', title: t('guide.welcome.title'), done: t('guide.welcome.done'), after,
    steps: [
      { icon: 'download', title: t('guide.welcome.1.title'), body: t('guide.welcome.1.body') },
      { icon: 'player', title: t('guide.welcome.2.title'), body: t('guide.welcome.2.body') },
      { icon: 'device', title: t('guide.welcome.3.title'), body: t('guide.welcome.3.body') },
    ],
  };
}

/** The first start on Android: why the system asks for what it asks. */
export function androidGuide(after?: () => void): GuideOptions {
  return {
    id: 'android', title: t('guide.android.title'), done: t('guide.android.done'), after,
    steps: [
      { icon: 'folder', title: t('guide.android.1.title'), body: t('guide.android.1.body') },
      { icon: 'device', title: t('guide.android.2.title'), body: t('guide.android.2.body') },
      { icon: 'updates', title: t('guide.android.3.title'), body: t('guide.android.3.body') },
    ],
  };
}

/** The first time the devices window opens: pairing, «Controlar» and «Escuchar en». */
export function connectGuide(run: () => void): GuideOptions {
  return {
    id: 'connect', title: t('guide.connect.title'), done: t('guide.connect.done'), run,
    steps: [
      { icon: 'device', title: t('guide.connect.1.title'), body: t('guide.connect.1.body') },
      { icon: 'control', title: t('guide.connect.2.title'), body: t('guide.connect.2.body') },
      { icon: 'cast', title: t('guide.connect.3.title'), body: t('guide.connect.3.body') },
    ],
  };
}

/** The first time what plays here is passed to another device. */
export function passGuide(name: string, run: () => void): GuideOptions {
  return {
    id: 'pass', title: t('guide.pass.title'), done: t('guide.pass.done'), run,
    steps: [{ icon: 'cast', title: t('guide.pass.1.title', name), body: t('guide.pass.1.body') }],
  };
}

/** The first time «Escuchar aquí» is pressed. */
export function listenGuide(name: string, run: () => void): GuideOptions {
  return {
    id: 'listen', title: t('guide.listen.title'), done: t('guide.listen.done'), run,
    steps: [{ icon: 'cast', title: t('guide.listen.1.title', name), body: t('guide.listen.1.body', name) }],
  };
}

/** The first time the library or the playlist of another device is shown. */
export function remoteGuide(name: string): GuideOptions {
  return {
    id: 'remote', title: t('guide.remote.title'), done: t('guide.remote.done'),
    steps: [{ icon: 'control', title: t('guide.remote.1.title', name), body: t('guide.remote.1.body', name) }],
  };
}
