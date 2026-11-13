// The way into «Dispositivos» (Soundify Connect) from the bar and the side menu, like the
// speaker icon of Spotify: lit while another device is being controlled, with a dot while
// some device plays and this one is not controlling it. Hidden while Connect is off.

import { connect } from '../connect';
import { icon } from '../icons';
import { t } from '../i18n';
import { openDevicesOverlay } from '../overlays/devices';
import { subscribeWhile, ui } from '../state';
import { control, describe } from './transport';

export function devicesButton(extraClass = ''): HTMLButtonElement {
  const button = control('monitor', 'connect.title', openDevicesOverlay);
  button.classList.add('devices-button');
  if (extraClass) button.classList.add(extraClass);
  const refresh = () => {
    const running = connect.status?.running ?? false;
    button.hidden = !running;
    const playingElsewhere = !ui.remote && connect.devices.some(device => device.playing && device.ready);
    button.classList.toggle('on', !!ui.remote);
    button.classList.toggle('has-dot', playingElsewhere);
    describe(button, 'connect.title');
    button.replaceChildren(icon('monitor', 18));
    button.title = ui.remote ? t('connect.controlling', ui.remote.name) : t('connect.title');
  };
  subscribeWhile(button, 'connect', refresh);
  subscribeWhile(button, 'player', refresh);
  refresh();
  return button;
}
