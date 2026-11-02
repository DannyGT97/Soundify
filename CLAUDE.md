# CLAUDE.md — Soundify (Tauri)

Port de Soundify (la app Java/JavaFX de `D:\Proyectos\soundifylocal`) a Tauri 2, pensando en
llevarla a Android. Todas las pantallas están hechas; la lógica se está portando por fases según
[MIGRATION-PLAN.md](MIGRATION-PLAN.md) (hechas: fases 0 a 3). Dentro de Tauri la biblioteca ya es la
real (los archives de `data/`); lo que aún no ha llegado sale de la API de mentira (`src/api/mock/`),
que es también lo que usan `npm run dev` y Playwright.

Repositorio: https://github.com/DannyGT97/Soundify (GitHub, rama `main`, GPL-3.0).

## Stack
- Tauri 2 + Vite + TypeScript sin framework (igual que `lienzo`); Rust en un workspace de Cargo
- Dev: `npm run tauri dev` · solo el navegador: `npm run dev` (puerto 5180)
- **Comprobar: `npm run check`** (tsc + vitest + cargo test + clippy, salida corta: una línea por
  herramienta, o solo lo que falla). Interfaz aparte: `npm run test:ui` (Playwright). Build: `npm run build`

## Estructura (raíz)
- `core/` — crate `soundify-core`: la lógica, **sin Tauri y sin plataforma** (se prueba con `cargo test`
  y sirve tal cual en Android). Hoy: `Location` (ruta o URI `content://`: una carpeta nunca es un
  `PathBuf`), el trait `Storage` (`LocalStorage` y `MemoryStorage` para tests, con el mismo contrato
  probado contra las dos; `write` NUNCA crea la carpeta) y el trait `Downloader` (yt-dlp por
  argumentos y salida línea a línea, con `StopFlag`; `ScriptedDownloader` reproduce salida grabada).
  Fase 1: `model` (lee y escribe los JSON de Java tal cual, `null` incluidos), `paths`
  (= `DownloadPathResolver`), `archive/` (fachada `ArchiveStore` + `io`/`sync`/`query`/`maintenance`,
  como en Java), `library` (archives → tipos del front), `settings`, `task_history` y `clock`
  (la hora se inyecta: los tests usan `clock::fixed`). Compatibilidad con Java en `core/tests/java_compat.rs`.
  Fase 2: `transfer` (= `PlaylistTransfer`: manifiesto `soundify-playlist.json`, copiar con espacio
  libre vía `fs4`, importar, `find_importable`); sus errores dan clave de i18n + argumentos.
  Fase 3: `ytdlp/` (comando, progreso, limpieza, motivos de no disponible como clave de i18n, prueba de
  `player_client`, consultas que no descargan), `version` y `log_format`. **Nada ahí ejecuta yt-dlp**:
  dan argumentos (sin el ejecutable) y leen la salida; ejecutarlo es cosa del `Downloader`
- `src-tauri/` — la app: comandos finos que llaman a `core/`, sin lógica propia (`commands.rs`).
  Un error de comando llega al front como `{key, args}` (`ApiError`; `errorText()` lo traduce).
  Plugins: `dialog` (elegir carpeta) y `opener` (abrir carpeta), con sus permisos en `capabilities/`.
  `app_paths.rs`: `data/` junto al ejecutable, o `SOUNDIFY_DATA_DIR`. El arranque corre
  `reconcile_all` en un hilo, y **escribe** en esa carpeta
- `e2e/` — tests de interfaz (Playwright) contra `npm run dev` con la API de mentira
- `scripts/check.mjs` — el `npm run check`

## Estructura (`src/`)
- `api/` — **la ÚNICA puerta al back**: las vistas nunca llaman a `invoke()`. `index.ts` declara la
  interfaz `Api` y elige implementación: `tauri.ts` dentro de la app (`insideTauri()`), `mock/` en
  el navegador. Lo que una fase aún no ha movido, `tauri.ts` lo sigue pasando al mock.
  `types.ts` = lo que se dicen front y back, incluido `PlayerBackend` (el motor que suena: el
  `<audio>` del webview en escritorio, un servicio nativo en Android). El progreso no vuelve por las
  llamadas: el back rellena `ui` y emite su tema (`tasks`, `player`)
- `api/mock/` — **DATOS Y SIMULACIONES DE MENTIRA**: la biblioteca (`data.ts`), la descarga que solo
  sube un contador y `SimulatedPlayer` (un reloj, no suena nada)
- `library.ts` — la biblioteca ya cargada (`loadLibrary()` antes de la primera pantalla y emite
  `library`), con `findBlock`, `countBy` y `onDisk`
- `player.ts` — cola, aleatorio y repetir sobre el `PlayerBackend`; `PLAYABLE` y `formatTime`
- `tasks.ts` — lo que el front deduce de la tarea (porcentaje, origen, marcar como vista)
- `urls.ts` — `isWebUrl` / `isPlaylistUrl`
- `transfer.ts` — los textos y el tono con que acaban «Copiar playlist» e «Importar» (lógica pura);
  las ventanas de esos dos flujos están en `overlays/transfer.ts`
- `*.test.ts` — Vitest, solo lógica pura del front; `i18n.test.ts` comprueba que toda clave usada existe
- `main.ts` — el armazón: menú + página + barra del reproductor + capa de ventanas superpuestas
- `router.ts` — rutas por hash (`#/manager`, `#/player?b=pl1:MP3`); cada visita rehace la pantalla,
  lo que tiene que sobrevivir vive en `state.ts`. `onLeave()` para timers y suscripciones
- `state.ts` — estado de la UI (menú plegado, resumen oculto, tema, ubicación del reproductor, lo que suena, la tarea); tema y ubicación se guardan en localStorage. `subscribe`/`emit` por tema (`library`, `player`, `tasks`, `theme`, `player-placement`)
- `i18n.ts` — TODO texto visible sale de aquí con `t(clave)`; las claves son las de la app Java
  cuando existen. Solo español por ahora
- `dom.ts` — `h()` para construir nodos (nunca HTML en cadenas: los títulos vienen de YouTube)
- `icons.ts` — TODOS los iconos, de Lucide (`lucide`, ISC): SVG con `currentColor`, nítidos a cualquier
  escala. Se importan por nombre (solo los usados entran en el build). Icono nuevo = una línea en `ICONS`.
  La S ancha de Inicio se dibuja como SVG inline con el acento; el icono de la app sigue siendo verde.
- `scale.ts` — la escala de la interfaz: en la app es el zoom NATIVO del webview (`setZoom`, permiso
  `core:webview:allow-set-webview-zoom`), que recoloca todo; un `zoom` de CSS en la raíz hacía crecer
  la pantalla por debajo de la ventana. En el navegador, `zoom` sobre `#app` (alto en %, no `vh`).
  Ctrl + / Ctrl − / Ctrl 0 van por los mismos pasos que la lista de Ajustes; se recuerda en
  localStorage hasta que exista `settings.json`
- `views/` — Inicio, Descarga por URL, Crear Playlist, Gestor, Reproductor, Reproduciendo, Ajustes
- `components/` — menú lateral (UNO para toda la app), barra del reproductor, portada, ventana
  superpuesta, piezas de formulario y el panel de resumen
- `overlays/` — Tareas, Ver archivos (filtro, estado y reproducción simulada por archivo o playlist),
  Ayuda de Importar, Actividad, confirmación, elegir dispositivo
- `styles/theme.css` — siete bloques `[data-theme="…"]` con los tokens `--sp-*`: Oscuro (predeterminado)
  y Claro (oficiales), más Oscuro rojo, Oscuro azul, AMOLED, Claro rojo y Claro azul. Incluye
  `--sp-success`, `--sp-cover-a` y `--sp-cover-b`; el éxito no usa el acento y el vídeo conserva su
  degradado morado. `styles/app.css` usa variables para los colores y contiene los selectores de tema y avisos.
- `components/themePicker.ts` — rejilla reutilizada por el botón del menú y Ajustes → Interfaz.
- `components/toasts.ts` — `notify({ tone, text })`, avisos apilados que caducan individualmente y viven
  en el armazón para sobrevivir a la navegación; los estados persistentes siguen en sus tarjetas.

## Pendiente
- **La migración de la lógica sigue [MIGRATION-PLAN.md](MIGRATION-PLAN.md)** (fases 1–10), y cada
  test portado se apunta en [TEST-MAP.md](TEST-MAP.md)
- Idiomas en/ca, y adaptar el diseño a pantalla de móvil
