// The way into «Dispositivos» (Soundify Connect) from the bar and the side menu, like the
// speaker icon of Spotify: lit while another device is being controlled, with a dot while
// some device plays and this one is not controlling it. Hidden while Connect is off.

import { connect } from '../connect';
import { icon } from '../icons';
import { t } from '../i18n';
import { openDevicesSwitcher } from '../overlays/devicesSwitcher';
import { subscribeWhile, ui } from '../state';
import { control } from './transport';

export function devicesButton(extraClass = ''): HTMLButtonElement {
  const button = control('monitor', 'connect.title', openDevicesSwitcher);
  button.classList.add('devices-button');
  if (extraClass) button.classList.add(extraClass);
  // Drawn once: the refresh runs with every tick of the song (several a second), and a button whose content is replaced
  // between pressing and letting go of the mouse never gets its click.
  button.replaceChildren(icon('monitor', 18));
  button.setAttribute('aria-label', t('connect.title'));
  const refresh = () => {
    const running = connect.status?.running ?? false;
    if (button.hidden === running) button.hidden = !running;
    const playingElsewhere = !ui.remote && connect.devices.some(device => device.playing && device.ready);
    button.classList.toggle('on', !!ui.remote);
    button.classList.toggle('has-dot', playingElsewhere);
    const title = ui.remote ? t('connect.controlling', ui.remote.name) : t('connect.title');
    if (button.title !== title) button.title = title;
  };
  subscribeWhile(button, 'connect', refresh);
  subscribeWhile(button, 'player', refresh);
  refresh();
  return button;
}
