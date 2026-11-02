// Hash routing between the screens (#/manager, #/player?b=pl1:MP3). Each
// screen is rebuilt on every visit; what has to survive lives in `ui`.

export type Route = 'home' | 'download' | 'new-playlist' | 'manager' | 'player' | 'now-playing' | 'settings';

export type Params = Record<string, string>;
export type View = (params: Params) => HTMLElement;

const ROUTES: Route[] = ['home', 'download', 'new-playlist', 'manager', 'player', 'now-playing', 'settings'];

let views: Partial<Record<Route, View>> = {};
let onRoute: (route: Route) => void = () => {};
let teardown: (() => void)[] = [];

export function navigate(route: Route, params: Params = {}): void {
  const query = new URLSearchParams(params).toString();
  const hash = `#/${route}${query ? `?${query}` : ''}`;
  if (location.hash === hash) render();
  else location.hash = hash;
}

export function currentRoute(): { route: Route; params: Params } {
  const [path = '', query = ''] = location.hash.replace(/^#\//, '').split('?');
  const route = (ROUTES as string[]).includes(path) ? (path as Route) : 'home';
  return { route, params: Object.fromEntries(new URLSearchParams(query)) };
}

/** Registers something to undo when the screen is left (timers, subscriptions). */
export function onLeave(cleanup: () => void): void {
  teardown.push(cleanup);
}

export function startRouter(registered: Partial<Record<Route, View>>, routeChanged: (route: Route) => void): void {
  views = registered;
  onRoute = routeChanged;
  window.addEventListener('hashchange', render);
  render();
}

function render(): void {
  teardown.forEach(cleanup => cleanup());
  teardown = [];
  const { route, params } = currentRoute();
  const page = document.getElementById('page');
  const view = views[route];
  if (!page || !view) return;
  const screen = view(params);
  screen.classList.add('screen-enter');
  page.replaceChildren(screen);
  page.scrollTop = 0;
  onRoute(route);
}
