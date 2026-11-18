import { expect, test, type Page } from '@playwright/test';

// Soundify Connect with the simulated network of `api/mock/connect.ts`. Like the real thing,
// devices are open by default: «Salón» is already connected and nothing is typed anywhere.
// «Tablet de la cocina» is strict and needs the PIN 123456.

async function openDevices(page: Page, route = '/?mockConnect#/settings'): Promise<void> {
  await page.goto(route);
  await page.getByRole('button', { name: 'Dispositivos…' }).click();
  await expect(page.locator('.overlay-window')).toBeVisible();
}

const row = (page: Page, id: string) => page.locator(`[data-device="${id}"]`);
const barTitle = (page: Page) => page.locator('.player-bar .player-now .row-title');

/** Something sounds on this device: the device button lives in the side menu's mini player, which only shows then. */
async function playSomething(page: Page, back: string): Promise<void> {
  await page.evaluate(() => { location.hash = '#/player?b=pl1:MP3'; });
  await page.locator('.tracks .track-row:not(.off)').first().click();
  await page.evaluate(hash => { location.hash = hash; }, back);
  await expect(page.locator('.sidebar .mini-controls')).toBeVisible();
}

test('an open device just shows up connected: nothing to pair, nothing to type', async ({ page }) => {
  await openDevices(page);
  const salon = row(page, 'tv');
  await expect(salon).toContainText('Conectado');
  await expect(salon.getByRole('button', { name: 'Controlar' })).toBeVisible();
  await expect(salon.locator('.cx-pin-input')).toHaveCount(0);
  // This device shows no PIN either while it is open.
  await expect(page.locator('.cx-pin-code')).toHaveCount(0);
});

test('a strict device asks for its PIN: a wrong one says so, the right one connects', async ({ page }) => {
  await openDevices(page);
  const tablet = row(page, 'tablet');
  await expect(tablet).toContainText('Pide un PIN');

  await tablet.locator('.cx-pin-input').fill('000000');
  await tablet.getByRole('button', { name: 'Emparejar' }).click();
  await expect(page.locator('.toast', { hasText: 'PIN incorrecto' })).toBeVisible();

  await tablet.locator('.cx-pin-input').fill('123 456');
  await tablet.getByRole('button', { name: 'Emparejar' }).click();
  await expect(tablet).toContainText('Conectado');
  await expect(tablet.getByRole('button', { name: 'Controlar' })).toBeVisible();
});

test('a PIN that is not six digits is not even sent', async ({ page }) => {
  await openDevices(page);
  await row(page, 'tablet').locator('.cx-pin-input').fill('12');
  await row(page, 'tablet').getByRole('button', { name: 'Emparejar' }).click();
  await expect(page.locator('.toast', { hasText: '6 cifras' })).toBeVisible();
  await expect(row(page, 'tablet')).toContainText('Pide un PIN');
});

test('asking for a PIN is a switch: this device then shows its own PIN', async ({ page }) => {
  await openDevices(page);
  await expect(page.locator('.cx-pin-code')).toHaveCount(0);
  await page.locator('.cx-switch', { hasText: 'Pedir un PIN' }).locator('.switch').check();
  await expect(page.locator('.cx-pin-code')).toHaveText('482 915');
  await page.locator('.cx-switch', { hasText: 'Pedir un PIN' }).locator('.switch').uncheck();
  await expect(page.locator('.cx-pin-code')).toHaveCount(0);
});

test('turning Soundify Connect off hides the devices', async ({ page }) => {
  await openDevices(page);
  await page.locator('.cx-switch', { hasText: 'Soundify Connect' }).locator('.switch').uncheck();
  await expect(page.locator('[data-device]')).toHaveCount(0);
});

test('one tap controls a connected device: the bar shows its music and the buttons drive it', async ({ page }) => {
  await openDevices(page);
  await row(page, 'tv').getByRole('button', { name: 'Controlar' }).click();
  await expect(barTitle(page)).toHaveText('Canción del salón');
  await page.locator('.player-bar .player-control[title="Siguiente"]').click();
  await expect(barTitle(page)).toHaveText('Siguiente del salón');

  // Reproduciendo says where it sounds.
  await page.goto('/?mockConnect#/now-playing');
  await expect(page.locator('.np-device')).toContainText('Salón');
});

test('a device that starts playing does not fill the screen with a notice (it shows only in the devices window)', async ({ page }) => {
  await page.goto('/?mockConnect#/home');
  await page.evaluate(() => (window as unknown as { __connect: { salonPlays: (on: boolean) => void } }).__connect.salonPlays(true));
  // Give the page the time a notice would take to show.
  await page.waitForTimeout(600);
  await expect(page.locator('.toast', { hasText: 'Salón' })).toHaveCount(0);
});

test('the list says what each device is playing', async ({ page }) => {
  await page.goto('/?mockConnect#/home');
  await playSomething(page, '#/home');
  await page.evaluate(() => (window as unknown as { __connect: { salonPlays: (on: boolean) => void } }).__connect.salonPlays(true));
  await page.locator('.sidebar .devices-button:visible').click();
  await expect(row(page, 'tv')).toContainText('Sonando: Canción del salón');
});

test('connection lost: this device is the one controlled again and the user is told', async ({ page }) => {
  await page.goto('/?mockConnect#/player?b=pl1:MP3');
  await page.locator('.track-row').first().click();
  const own = await barTitle(page).textContent();
  await openDevices(page);
  await row(page, 'tv').getByRole('button', { name: 'Controlar' }).click();
  await expect(barTitle(page)).toHaveText('Canción del salón');

  await page.evaluate(() => (window as unknown as { __connect: { drop: () => void } }).__connect.drop());
  await expect(page.locator('.toast', { hasText: 'Se perdió la conexión' })).toBeVisible();
  await expect(barTitle(page)).toHaveText(own ?? '');
});

test('being controlled: a command from another device moves this player and its state is published', async ({ page }) => {
  await page.goto('/?mockConnect#/player?b=pl1:MP3');
  await page.locator('.track-row').first().click();
  const first = await barTitle(page).textContent();

  const incoming = (command: object) => page.evaluate(c => (window as unknown as { __connect: { incoming: (c: object) => void } }).__connect.incoming(c), command);
  await incoming({ cmd: 'next' });
  await expect(barTitle(page)).not.toHaveText(first ?? '');

  await incoming({ cmd: 'pause' });
  await expect(page.locator('.player-bar .player-play')).toBeVisible();
  const published = await page.evaluate(() => (window as unknown as { __connect: { published: Array<{ playing: boolean; nowPlaying: { title: string } | null }> } }).__connect.published);
  expect(published.length).toBeGreaterThan(0);
  expect(published.at(-1)?.playing).toBe(false);
  expect(published.at(-1)?.nowPlaying?.title).toBeTruthy();
});

test('being controlled: the playlist that sounds is published and a controller can pick one of its songs', async ({ page }) => {
  await page.goto('/?mockConnect#/player?b=pl1:MP3');
  await page.locator('.track-row').first().click();
  const queue = await page.evaluate(() => (window as unknown as { __connect: { publishedQueues: Array<{ playlist: string; items: Array<{ videoId: string; title: string }> } | null> } }).__connect.publishedQueues.at(-1));
  expect(queue?.playlist).toBeTruthy();
  expect(queue?.items.length).toBeGreaterThan(1);
  const wanted = queue!.items[1]!;
  await page.evaluate(id => (window as unknown as { __connect: { incoming: (c: object) => void } }).__connect.incoming({ cmd: 'playTrack', videoId: id }), wanted.videoId);
  await expect(barTitle(page)).toHaveText(wanted.title);
});

test('controlling a device lists the playlist it plays, says where it is, and picking a song plays it there', async ({ page }) => {
  await openDevices(page);
  await row(page, 'tv').getByRole('button', { name: 'Controlar' }).click();
  await expect(barTitle(page)).toHaveText('Canción del salón');
  // Reproduciendo: its arrow goes to the remote playlist, not to a playlist of this device.
  await page.locator('.player-bar .player-now').click();
  await page.locator('.np-back').click();
  await expect(page.locator('.eyebrow')).toHaveText('Playlist remota');
  await expect(page.locator('.pl-sub')).toContainText('En «Salón»');
  await expect(page.locator('.track-row')).toHaveCount(3);
  await expect(page.locator('.track-row.playing')).toContainText('Canción del salón');
  // Picking another one plays it on the device, and the list follows.
  await page.locator('.track-row', { hasText: 'Tercera del salón' }).click();
  await expect(page.locator('.track-row.playing')).toContainText('Tercera del salón');
  await expect(barTitle(page)).toHaveText('Tercera del salón');
});

test('the remote playlist follows the device when it starts another one', async ({ page }) => {
  await openDevices(page);
  await row(page, 'tv').getByRole('button', { name: 'Controlar' }).click();
  await page.evaluate(() => { location.hash = '#/remote-playlist'; });
  await page.evaluate(() => (window as unknown as { __connect: { salonQueue: (p: string, t: string[]) => void } }).__connect.salonQueue('Jazz', ['Uno', 'Dos']));
  await expect(page.locator('.detail h1')).toHaveText('Jazz');
  await expect(page.locator('.track-row')).toHaveCount(2);
});

test('the remote controls (volume, shuffle, repeat) reach the device and come back in its state, on a phone too', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 800 });
  await openDevices(page, '/?mockConnect&mockAndroid#/settings');
  await row(page, 'tv').getByRole('button', { name: 'Controlar' }).click();
  await page.evaluate(() => { location.hash = '#/now-playing'; });
  const controls = page.locator('.np-controls');
  // The phone's buttons are not the volume of another device: its slider shows.
  await expect(controls.locator('.player-volume')).toBeVisible();
  await controls.locator('.player-volume input').fill('0.2');
  await expect(controls.locator('.player-volume input')).toHaveValue('0.2');
  await controls.getByTitle('Aleatorio').click();
  await expect(controls.locator('.player-control.on').first()).toBeVisible();
  await controls.getByTitle(/Repetir/).click();
  await expect(controls.locator('.player-control.on')).toHaveCount(2);
});

test('what is typed in the address and PIN boxes survives the list redrawing itself', async ({ page }) => {
  await openDevices(page);
  const address = page.locator('.cx-add input');
  await address.click();
  await address.pressSequentially('192.168');
  const pin = row(page, 'tablet').locator('.cx-pin-input');
  await pin.fill('12');
  await address.click();
  // The window reloads its list every few seconds.
  await page.waitForTimeout(3600);
  await expect(address).toHaveValue('192.168');
  await expect(address).toBeFocused();
  await expect(pin).toHaveValue('12');
  await address.pressSequentially('.1.31');
  await expect(address).toHaveValue('192.168.1.31');
});

test('Windows: a Public network is warned about and the firewall rule is one button', async ({ page }) => {
  await openDevices(page, '/?mockConnect&mockFirewall#/settings');
  await expect(page.locator('.cx-warning', { hasText: 'Pública' })).toBeVisible();
  // One click makes the network private; the rule is another one.
  await page.getByRole('button', { name: 'Cambiar a red privada' }).click();
  await expect(page.locator('.toast', { hasText: 'tu red ahora es privada' })).toBeVisible();
  await expect(page.locator('.cx-warning', { hasText: 'Pública' })).toHaveCount(0);
  const card = page.locator('.cx-firewall');
  await expect(card).toContainText('Permitir Soundify en el cortafuegos');
  await card.getByRole('button', { name: 'Permitir en el cortafuegos' }).click();
  await expect(page.locator('.toast', { hasText: 'Soundify puede recibir conexiones' })).toBeVisible();
  await expect(page.locator('.cx-firewall')).toHaveCount(0);
});

test('without Windows there is nothing about the firewall', async ({ page }) => {
  await openDevices(page);
  await expect(page.locator('.cx-firewall')).toHaveCount(0);
});

test('Android 17: without the local network permission the window says so and one button asks for it', async ({ page }) => {
  await openDevices(page, '/?mockConnect&mockLocalNetwork#/settings');
  const card = page.locator('.cx-firewall', { hasText: 'permiso de red local' });
  await expect(card).toBeVisible();
  await card.getByRole('button', { name: 'Permitir red local' }).click();
  await expect(page.locator('.cx-firewall', { hasText: 'permiso de red local' })).toHaveCount(0, { timeout: 8000 });
});

test('the device icon only lists the devices to switch between, and points to Ajustes for the rest', async ({ page }) => {
  await page.goto('/?mockConnect#/player');
  await playSomething(page, '#/player');
  await page.locator('.devices-button:visible').first().click();
  const list = page.locator('.overlay-window');
  // Two sections: where what plays here sounds, and which other device to control.
  await expect(list.locator('.cx-section')).toHaveText(['Escuchar en', 'Controlar otro dispositivo']);
  await expect(list.locator('[data-pass="this"]')).toContainText('Suena aquí');
  await expect(list.locator('[data-pass="tv"]')).toContainText('Pasar aquí');
  await expect(list.locator('[data-device="tv"]')).toContainText('Salón');
  // No pairing, PIN or address here: that is in Ajustes.
  await expect(list.locator('.cx-pin-input, .cx-add')).toHaveCount(0);
  await list.locator('[data-device="tv"]').click();
  await expect(barTitle(page)).toHaveText('Canción del salón');
  await page.locator('.devices-button:visible').first().click();
  await page.getByRole('button', { name: 'Gestionar dispositivos' }).click();
  await expect(page).toHaveURL(/#\/settings/);
  await expect(page.locator('.overlay-window .cx-switch').first()).toBeVisible();
});

test('the back gesture closes the devices window instead of leaving the screen', async ({ page }) => {
  await page.goto('/?mockConnect#/settings');
  await page.getByRole('button', { name: 'Dispositivos…' }).click();
  await expect(page.locator('.overlay-window')).toBeVisible();
  await page.goBack();
  await expect(page.locator('.overlay-window')).toHaveCount(0);
  await expect(page).toHaveURL(/#\/settings/);
});

test('the Reproductor lists the playlist of the device being controlled, with its name, next to the local ones', async ({ page }) => {
  await openDevices(page);
  await row(page, 'tv').getByRole('button', { name: 'Controlar' }).click();
  await page.evaluate(() => { location.hash = '#/player'; });
  await expect(page.locator('.section-head', { hasText: 'En «Salón»' })).toBeVisible();
  const card = page.locator('.pl-card', { hasText: 'Playlist remota' });
  await expect(card).toContainText('Mix');
  await expect(card.locator('.badge')).toContainText('Salón');
  await card.click();
  await expect(page.locator('.eyebrow')).toHaveText('Playlist remota');
});

test('the controlled device only says who took it over, not what that device then does', async ({ page }) => {
  await page.goto('/?mockConnect#/home');
  const incoming = (command: object) => page.evaluate(c => (window as unknown as { __connect: { incoming: (c: object) => void } }).__connect.incoming(c), command);
  await incoming({ cmd: 'take' });
  await expect(page.locator('.toast', { hasText: 'controla ahora este dispositivo' })).toBeVisible();
  await incoming({ cmd: 'pause' });
  await incoming({ cmd: 'volume', volume: 0.3 });
  await page.waitForTimeout(300);
  await expect(page.locator('.toast')).toHaveCount(1);
});

test('Reproduciendo is titled with the playlist, and says «Playlist remota» when it is on another device', async ({ page }) => {
  await openDevices(page);
  await row(page, 'tv').getByRole('button', { name: 'Controlar' }).click();
  await page.evaluate(() => { location.hash = '#/now-playing'; });
  await expect(page.locator('.np-head-eyebrow')).toHaveText('Playlist remota');
  await expect(page.locator('.np-head-name')).toHaveText('Mix');
});

test('repeat goes round off, all, one and back to off on the controlled device', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 800 });
  await openDevices(page, '/?mockConnect&mockAndroid#/settings');
  await row(page, 'tv').getByRole('button', { name: 'Controlar' }).click();
  await page.evaluate(() => { location.hash = '#/now-playing'; });
  const repeat = page.locator('.np-controls .player-controls .player-control').last();
  for (const wanted of ['Repetir: toda la playlist', 'Repetir: este archivo', 'Repetir: no']) {
    await repeat.click();
    await expect(repeat).toHaveAttribute('title', wanted);
  }
});

test('playing a file of this device while controlling another lets go of the control, not of the connection', async ({ page }) => {
  await openDevices(page);
  await row(page, 'tv').getByRole('button', { name: 'Controlar' }).click();
  await expect(barTitle(page)).toHaveText('Canción del salón');
  await page.goto('/?mockConnect#/player?b=pl1:MP3');
  const local = page.locator('.tracks .track-row:not(.off)').first();
  const title = (await local.locator('.row-title').textContent()) ?? '';
  await local.click();
  await expect(barTitle(page)).toHaveText(title);
  const notice = page.locator('.toast', { hasText: 'sigue conectado' });
  await expect(notice).toBeVisible();
  await expect(page.locator('.toast', { hasText: 'Se perdió la conexión' })).toHaveCount(0);
  // The other device is still there: one tap takes it back.
  await notice.getByRole('button', { name: 'Volver a controlarlo' }).click();
  await expect(barTitle(page)).toHaveText('Canción del salón');
});

test('the remote playlist sorts by # and title like a local one', async ({ page }) => {
  await openDevices(page);
  await row(page, 'tv').getByRole('button', { name: 'Controlar' }).click();
  await page.evaluate(() => { location.hash = '#/remote-playlist'; });
  await page.evaluate(() => (window as unknown as { __connect: { salonQueue: (p: string, t: string[]) => void } }).__connect.salonQueue('Jazz', ['Beta', 'Alfa', 'Gamma']));
  const titles = page.locator('.track-row .row-title');
  await expect(titles).toHaveText(['Beta', 'Alfa', 'Gamma']);
  await page.locator('.cols .col-sort', { hasText: 'Título' }).click();
  await expect(titles).toHaveText(['Alfa', 'Beta', 'Gamma']);
  await page.locator('.cols .col-sort', { hasText: 'Título' }).click();
  await expect(titles).toHaveText(['Gamma', 'Beta', 'Alfa']);
  await page.locator('.cols .col-sort', { hasText: '#' }).click();
  await expect(titles).toHaveText(['Beta', 'Alfa', 'Gamma']);
});

test('refreshing the device list does not publish the state again (with three devices that was a storm)', async ({ page }) => {
  await openDevices(page);
  const count = () => page.evaluate(() => (window as unknown as { __connect: { published: unknown[] } }).__connect.published.length);
  await page.waitForTimeout(300);
  const before = await count();
  // Each of these makes the list reload, like a state arriving from another device does.
  for (let i = 0; i < 5; i++) {
    await page.evaluate(on => (window as unknown as { __connect: { salonPlays: (on: boolean) => void } }).__connect.salonPlays(on), i % 2 === 0);
    await page.waitForTimeout(50);
  }
  await page.waitForTimeout(300);
  expect(await count()).toBe(before);
});

test('what the page does with Connect leaves a line in the log (the page used to leave no trace)', async ({ page }) => {
  await openDevices(page);
  await row(page, 'tv').getByRole('button', { name: 'Controlar' }).click();
  await expect(barTitle(page)).toHaveText('Canción del salón');
  const logged = () => page.evaluate(() => (window as unknown as { __logged: string[] }).__logged);
  expect((await logged()).some(line => line.includes('[connect] control Salón'))).toBe(true);
  expect((await logged()).some(line => line.includes('[app] page started'))).toBe(true);
});

test('Ajustes → Acerca de copies the diagnostics', async ({ page, context }) => {
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.goto('/?mockConnect#/settings');
  await page.getByRole('button', { name: 'Acerca de' }).click();
  await page.getByRole('button', { name: 'Copiar diagnóstico' }).click();
  await expect(page.locator('.toast', { hasText: 'Diagnóstico copiado' })).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toContain('Diagnóstico de Soundify');
});

test('the devices button is in the side menu with nothing playing, and moves into the mini player when something does', async ({ page }) => {
  await page.goto('/?mockConnect#/home');
  await expect(page.locator('.sidebar .side-devices-idle:visible')).toHaveCount(1);
  await playSomething(page, '#/home');
  await expect(page.locator('.sidebar .side-devices-idle:visible')).toHaveCount(0);
  await expect(page.locator('.sidebar .mini-controls .devices-button:visible')).toHaveCount(1);
});

// «Escuchar en», the device that RECEIVES: another device passes what it plays and it sounds here, read from it.
const pass = {
  cmd: 'cast', playlist: 'Rock del móvil', index: 1, position: 30, playing: true, shuffle: false, repeat: 'all',
  items: [
    { videoId: 'cast-1', title: 'Primera del móvil', duration: 200, format: 'mp3', video: false, cover: false },
    { videoId: 'cast-2', title: 'Segunda del móvil', duration: 180, format: 'mp3', video: false, cover: false },
    { videoId: 'cast-3', title: 'Tercera del móvil', duration: 210, format: 'mp3', video: false, cover: false },
  ],
};
const incoming = (page: Page, command: object) => page.evaluate(c => (window as unknown as { __connect: { incoming: (c: object) => void } }).__connect.incoming(c), command);
type Published = Array<{ playing: boolean; nowPlaying: { videoId: string; playlist: string } | null }>;

test('a pass from another device plays here from its file and second, says who, and is published as what sounds', async ({ page }) => {
  await page.goto('/?mockConnect#/home');
  await incoming(page, pass);
  await expect(barTitle(page)).toHaveText('Segunda del móvil');
  await expect(page.locator('.player-bar .player-now .subtitle')).toHaveText('Rock del móvil · Desde «Móvil de prueba»');
  await expect(page.locator('.toast', { hasText: 'Móvil de prueba está reproduciendo aquí' })).toBeVisible();
  const last = await page.evaluate(() => (window as unknown as { __connect: { published: Published } }).__connect.published.at(-1));
  expect(last?.nowPlaying).toMatchObject({ videoId: 'cast-2', playlist: 'Rock del móvil' });
  expect(last?.playing).toBe(true);

  // The other device drives it like any controlled player: «next» is ITS next file.
  await incoming(page, { cmd: 'next' });
  await expect(barTitle(page)).toHaveText('Tercera del móvil');
  await incoming(page, { cmd: 'playTrack', videoId: 'cast-1' });
  await expect(barTitle(page)).toHaveText('Primera del móvil');
});

test('when the device that passed it takes it back, nothing of it sounds here', async ({ page }) => {
  await page.goto('/?mockConnect#/home');
  await incoming(page, pass);
  await expect(barTitle(page)).toHaveText('Segunda del móvil');
  await incoming(page, { cmd: 'castEnd' });
  await expect(page.locator('.toast', { hasText: 'ya no reproduce aquí' })).toBeVisible();
  await expect(page.locator('.player-bar')).toBeHidden();
});

test('playing a file of this device ends the pass, and this device keeps its own last file', async ({ page }) => {
  await page.goto('/?mockConnect#/player?b=pl1:MP3');
  await page.locator('.track-row').first().click();
  const own = await barTitle(page).textContent();
  await incoming(page, pass);
  await expect(barTitle(page)).toHaveText('Segunda del móvil');
  await page.locator('.track-row').first().click();
  await expect(barTitle(page)).toHaveText(own ?? '');
  const last = await page.evaluate(() => (window as unknown as { __connect: { published: Published } }).__connect.published.at(-1));
  expect(last?.nowPlaying?.videoId).not.toMatch(/^cast-/);
});

// «Escuchar en», the device that PASSES: what plays here goes on sounding on another device, read from this one.
type Casts = Array<{ deviceId: string; playlist: string; index: number; position: number; items: Array<{ videoId: string; title: string }> }>;
const casts = (page: Page) => page.evaluate(() => (window as unknown as { __connect: { casts: Casts } }).__connect.casts);
const castEnds = (page: Page) => page.evaluate(() => (window as unknown as { __connect: { castEnds: string[] } }).__connect.castEnds);

async function passToSalon(page: Page): Promise<string> {
  await page.goto('/?mockConnect#/player');
  await playSomething(page, '#/player');
  const title = (await barTitle(page).textContent()) ?? '';
  await page.locator('.devices-button:visible').first().click();
  await page.locator('[data-pass="tv"]').click();
  await expect(page.locator('.overlay-window')).toHaveCount(0);
  return title;
}

test('passing what plays here sends its queue to the other device, which plays it from the same file', async ({ page }) => {
  const title = await passToSalon(page);
  const [pass] = await casts(page);
  expect(pass?.deviceId).toBe('tv');
  expect(pass?.items[pass.index]?.title).toBe(title);
  expect(pass?.items.length).toBeGreaterThan(1);
  // This device is now the remote: the bar shows what sounds THERE, which is the same file.
  await expect(barTitle(page)).toHaveText(title);
  await page.locator('.devices-button:visible').first().click();
  await expect(page.locator('.overlay-window .cx-banner-cast')).toContainText('Sonando en «Salón»');
});

test('«Traer aquí» ends the pass and this device plays the file again', async ({ page }) => {
  const title = await passToSalon(page);
  await page.locator('.devices-button:visible').first().click();
  await page.getByRole('button', { name: 'Traer aquí' }).click();
  await expect.poll(() => castEnds(page)).toContain('bring');
  await expect(barTitle(page)).toHaveText(title);
  await page.locator('.devices-button:visible').first().click();
  await expect(page.locator('.overlay-window .cx-banner-cast')).toHaveCount(0);
});

test('«Detener» ends the pass without bringing anything back', async ({ page }) => {
  await passToSalon(page);
  await page.locator('.devices-button:visible').first().click();
  await page.getByRole('button', { name: 'Detener' }).click();
  await expect.poll(() => castEnds(page)).toContain('stop');
});

test('nothing to pass while nothing of this device plays', async ({ page }) => {
  await page.goto('/?mockConnect#/home');
  await page.locator('.devices-button:visible').first().click();
  await expect(page.locator('.overlay-window [data-device="tv"]')).toBeVisible();
  await expect(page.locator('[data-pass]')).toHaveCount(0);
});

test('controlling another device is said in its own colour, with the way out, and passing waits until it ends', async ({ page }) => {
  await page.goto('/?mockConnect#/player');
  await playSomething(page, '#/player');
  await page.locator('.devices-button:visible').first().click();
  await page.locator('[data-device="tv"]').click();
  await expect(barTitle(page)).toHaveText('Canción del salón');
  await expect(page.locator('.devices-button:visible').first()).toHaveClass(/(^| )is-control( |$)/);
  await page.locator('.devices-button:visible').first().click();
  const list = page.locator('.overlay-window');
  await expect(list.locator('.cx-banner-control')).toContainText('Controlando «Salón»');
  await expect(list.locator('[data-pass]')).toHaveCount(0);
  await expect(list).toContainText('Deja de controlar «Salón» para pasar tu música.');
  await list.getByRole('button', { name: 'Dejar de controlar' }).click();
  await expect(page.locator('.devices-button:visible').first()).not.toHaveClass(/(^| )is-control( |$)/);
});

test('while the music of this device plays elsewhere the devices button says so in green', async ({ page }) => {
  await passToSalon(page);
  const button = page.locator('.devices-button:visible').first();
  await expect(button).toHaveClass(/(^| )is-cast( |$)/);
  await expect(button).toHaveAttribute('title', 'Sonando en «Salón»');
  await button.click();
  // The device with this one's music is not offered to control (it is controlled already, as the pass).
  await expect(page.locator('.overlay-window [data-device="tv"]')).toHaveCount(0);
  await expect(page.locator('.overlay-window [data-pass="tv"]')).toContainText('Sonando tu música');
});

test('a device with an older Soundify is listed as such and cannot be used', async ({ page }) => {
  await page.goto('/?mockConnect&mockOldDevice#/player');
  await playSomething(page, '#/player');
  await page.locator('.devices-button:visible').first().click();
  const old = page.locator('.overlay-window [data-pass="old"]');
  await expect(old).toContainText('Versión antigua de Soundify: actualízalo');
  await expect(old).toHaveClass(/offline/);
  await expect(page.locator('.overlay-window [data-device="old"]')).toHaveCount(0);
});

test('Reproduciendo shows what another device passed here, replacing what was shown of this device', async ({ page }) => {
  await page.goto('/?mockConnect#/player?b=pl1:MP3');
  await page.locator('.tracks .track-row:not(.off)').first().click();
  await page.evaluate(() => { location.hash = '#/now-playing'; });
  const own = await page.locator('.np-info .np-title, .np-title').first().textContent();
  await incoming(page, pass);
  await expect(page.locator('.np-title').first()).toHaveText('Segunda del móvil');
  expect(own).not.toBe('Segunda del móvil');
  // Its playlist and where it comes from, not a playlist of this device.
  await expect(page.locator('.np-head-name')).toHaveText('Rock del móvil');
  await incoming(page, { cmd: 'next' });
  await expect(page.locator('.np-title').first()).toHaveText('Tercera del móvil');
});

// Controlling a device that plays nothing: its library is listed, to pick a playlist, see its songs and start one.
test('a device that plays nothing lists its playlists, and a song of one can be started from here', async ({ page }) => {
  await page.goto('/?mockConnect&mockSalonIdle#/settings');
  await openDevices(page, '/?mockConnect&mockSalonIdle#/settings');
  await row(page, 'tv').getByRole('button', { name: 'Controlar' }).click();
  await page.evaluate(() => { location.hash = '#/player'; });
  await page.locator('.pl-card', { hasText: 'Playlists' }).click();
  // Nothing plays there: the playlists of its library, not an empty list.
  await expect(page.locator('.np-head-name, h1').first()).toContainText('Playlists');
  await expect(page.locator('[data-library]')).toHaveCount(2);
  await expect(page.locator('.eyebrow')).toHaveText('Biblioteca de «Salón»');
  await expect(page.locator('[data-library="salon:cocina"]')).toContainText('2 canciones');
  // One of them: its songs (nothing starts by looking).
  await page.locator('[data-library="salon:cocina"]').click();
  await expect(page.locator('.track-row')).toHaveCount(2);
  await expect(page.locator('.track-row').first()).toContainText('Para cocinar');
  await expect(page.locator('.player-bar')).toBeHidden();
  // A song: it plays THERE, and what is shown is now what plays.
  await page.locator('.track-row').nth(1).click();
  await expect(barTitle(page)).toHaveText('Para fregar');
  await expect(page.locator('[data-library]')).toHaveCount(0);
  await expect(page.locator('.track-row')).toHaveCount(2);
});

test('while a playlist plays there, «Todas las playlists» goes to the library and back to what plays', async ({ page }) => {
  await openDevices(page);
  await row(page, 'tv').getByRole('button', { name: 'Controlar' }).click();
  await page.evaluate(() => { location.hash = '#/remote-playlist'; });
  await expect(page.locator('.track-row')).toHaveCount(3);
  await page.getByRole('button', { name: 'Todas las playlists' }).click();
  await expect(page.locator('[data-library]')).toHaveCount(2);
  await page.getByRole('button', { name: 'Lo que suena' }).click();
  await expect(page.locator('.track-row')).toHaveCount(3);
});
