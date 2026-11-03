# CLAUDE.md — Soundify (Tauri)

Port de Soundify (la app Java/JavaFX de `D:\Proyectos\soundifylocal`) a Tauri 2, pensando en
llevarla a Android. Todas las pantallas están hechas; la lógica se está portando por fases según
[MIGRATION-PLAN.md](MIGRATION-PLAN.md) (hechas: fases 0 a 9). Dentro de Tauri la biblioteca ya es la
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
  Fase 4: `http` (trait `Http` + `ReqwestHttp`, escribe en `.part` y renombra) y `tools/` (`ToolPaths`:
  en Windows `data/systemtools/`, fuera el binario del sistema; `zip_extract`; `health` = `ToolHealth`;
  `updaters` de yt-dlp, ffmpeg y Deno, probados con `Http` falso)
  Fase 5: `engine/` (`download` = `DownloadCoordinator`, `safety_net`, `tasks` = `BackgroundTasks`,
  `activity` = `ActivityLog`); el motor no toca UI, todo sale por `DownloadEvents`
  Fase 6: `cover_art` (= `CoverArt`: portada elegida `cover.jpg` en la carpeta `pl_...`, miniaturas
  `hqdefault` a `data/cache/thumbnails/` con `ThumbnailFetcher`, imagen validada por sus bytes). El
  RECORTE no está aquí: lo hace el front (`src/covers.ts`), no hay crate `image`
  Fase 7: `settings::player_memory`/`set_player_memory` (volumen y último archivo con su segundo, en los
  campos `playerVolume`/`lastPlayed*` de Java)
  Fase 8: `settings` guarda también las preferencias de Ajustes (`appearance` = id del tema, que tira
  de `theme` DARK/LIGHT para Java; `playerPlacement`, idioma, escala, cookies, interruptores) y `logs`
  (logger propio: `soundify-N.log`, `-activity-N`, `-retries-N`, 10 MB y 10 de cada, como Java;
  `tauri-plugin-log` pone fechas en los rotados y por eso no se usa)
  Fase 9: `updater/` (= `AppUpdater` + `UpdateDownloader`/`UpdateInstaller`/`UpdateVerifier`/
  `ReleaseNotes`: ZIP portable de la release de GitHub (`DannyGT97/Soundify`) firmado con Ed25519, assets con sufijo `-tauri`
  para no casar con las releases Java 1.x; NO `tauri-plugin-updater`) y `device/` (trait
  `PortableDevice`, `NoDevices` fuera de Windows, `fake::FakeDevice` para tests) con lo que lo usa:
  `transfer/device` (copiar al/del teléfono, importar desde él), `engine/download/device` (=
  `DeviceSyncService`, un vídeo cada vez, preparado en local y empujado después) y `archive/device`
  («Eliminar todo» en el teléfono)
- `src-tauri/` — la app: comandos finos que llaman a `core/`, sin lógica propia (`commands.rs`).
  Un error de comando llega al front como `{key, args}` (`ApiError`; `errorText()` lo traduce).
  Plugins: `dialog` (elegir carpeta) y `opener` (abrir carpeta), con sus permisos en `capabilities/`.
  `app_paths.rs`: `data/` junto al ejecutable, o `SOUNDIFY_DATA_DIR`. El arranque corre
  `reconcile_all` en un hilo, y **escribe** en esa carpeta. `bootstrap.rs` arranca eso, los updaters
  (solo con herramientas gestionadas) y la caché de formatos; `process.rs` = `YtDlpProcess`, el
  `Downloader` real (UTF-8, Deno en el PATH, stdout/stderr en hilos aparte, parar mata el árbol). Los
  problemas de herramientas salen como evento `tool-health` → toast persistente (`main.ts`).
  **ffmpeg viaja con la app** (como en Java, que lo llevaba en el JAR): recurso de Tauri
  (`src-tauri/resources/ffmpeg/` → `tools/ffmpeg/` junto al exe; los `.exe` NO van en git, los pone
  `node scripts/fetch-ffmpeg.mjs`, o `--from <carpeta bin>`). El arranque lo copia a `data/systemtools/`
  (`updaters::seed_ffmpeg`). **El arranque ya NO baja nada que falte** (Java bajaba ~180 MB sin
  preguntar): solo copia el ffmpeg incluido y, con `autoUpdates` (Ajustes → Herramientas), actualiza un
  yt-dlp que YA esté. Instalar es cosa del usuario, por `tools.rs`: `tools_report` (estado y versión de
  cada una), `install_tool(id, reinstall)` (una a la vez por herramienta: `AppState.installing`, error
  `tools.error.busy`; reinstalar baja aparte y solo sustituye si sale bien), `check_ytdlp_update` y
  `open_tools_folder`. En el front, `tools.ts` es el estado único (informe, en cola/instalando/fallidas,
  porcentajes) y lo leen tres sitios: el **asistente** del primer arranque (`overlays/toolsSetup.ts`,
  se abre solo si falta yt-dlp o ffmpeg — Deno es recomendado, no imprescindible; «Ahora no» no se
  recuerda y deja un aviso con «Ir a Ajustes → Herramientas»), la **pestaña Herramientas** de Ajustes
  (Instalar / Actualizar o Buscar actualización / Reinstalar, insignia con lo que falta) y el **aviso
  de Descarga** (con «Instalar ahora»; el botón Descargar se desactiva mientras falte lo
  imprescindible). Lo que se esté bajando sale en vivo: `InstallProgress` → evento `tool-downloads` →
  `watchTools()` → toast con porcentaje y «Ver» (oculto mientras el asistente está abierto, que ya lo
  enseña). En el navegador, `?mockTools=missing` arranca sin herramientas y `?mockTools=failing`
  además hace fallar Deno una vez. Hecho a partir de `design/tools-setup-prototype.html`; del
  prototipo quedaron fuera «Cancelar» por herramienta y la comprobación «una vez al día».
  `downloads.rs`: la descarga en curso en su hilo → eventos `tasks`, `activity`, `activity-reset` y
  `notice` (los escucha `api.watchDownloads`; `tasks.ts` los pasa a `ui`). Al acabar, el front relee
  la biblioteca; mientras corre, cada archivo registrado emite `file-downloaded` (`api.onFileDownloaded`)
  y el front la relee con `loadLibrarySoon()` (400 ms, junta las ráfagas): «Ver archivos» y el Gestor
  enseñan cada archivo en cuanto está en disco. La tarea en curso lleva `blockKey` (el archive del
  bloque que se descarga; null en una suelta). Sin ffmpeg (aún sin instalar) no arranca: aviso `activity.error.ffmpegMissing`.
  «Crear Playlist» es su propio comando (`create_playlist` → `Engine::create_playlist`): REGISTRA antes
  de descargar nada, y «Crear y descargar» lanza después un `update` normal. Una descarga suelta va a la
  carpeta elegida tal cual (sin subcarpeta, sin numerar) y su limpieza solo toca restos de lo que bajó
  `covers.rs`: comandos de portadas y el ámbito del protocolo `asset`, que se abre EN MARCHA
  (`allow_folder`: caché de miniaturas, archives, cada carpeta de música que lista `list_playlists` y
  la imagen recién elegida; en `tauri.conf.json` va vacío). Evento `thumbnail-ready`.
  Cerrar la ventana guarda antes el segundo del reproductor (`onCloseRequested` → permiso
  `core:window:allow-destroy`)
  `app_update.rs`: comprobar (8 s tras arrancar), descargar e instalar → eventos `app-update` y
  `app-update-progress`; en desarrollo se niega (`update.devBuild`). `devices.rs`: los comandos de
  dispositivo (listar, explorar, crear playlist, copiar en los dos sentidos, importar; las copias
  emiten `copy-progress` y paran con `cancel_copy`). `mtp.rs`: MTP real con el crate `windows`
  (`Shell.Application` para listar, `IFileOperation` para copiar y borrar), cada llamada en su hilo y
  apartamento COM con tope de 15/60/90 s. **Sin probar con un teléfono real**
- `e2e/` — tests de interfaz (Playwright) contra `npm run dev` con la API de mentira
- `scripts/check.mjs` — el `npm run check`

## Estructura (`src/`)
- `api/` — **la ÚNICA puerta al back**: las vistas nunca llaman a `invoke()`. `index.ts` declara la
  interfaz `Api` y elige implementación: `tauri.ts` dentro de la app (`insideTauri()`), `mock/` en
  el navegador. Lo que una fase aún no ha movido, `tauri.ts` lo sigue pasando al mock.
  `types.ts` = lo que se dicen front y back, incluido `PlayerBackend` (el motor que suena:
  `htmlPlayer.ts` en escritorio, un servicio nativo en Android; `SimulatedPlayer` en el navegador).
  `htmlPlayer.ts` es UN `<video>` (audio y vídeo) en un contenedor oculto de `<body>`: Reproduciendo lo
  MUEVE a su hueco y lo devuelve al salir (`park`), porque sacado de la página se pausa solo. El progreso no vuelve por las
  llamadas: el back rellena `ui` y emite su tema (`tasks`, `player`)
- `api/mock/` — **DATOS Y SIMULACIONES DE MENTIRA**: la biblioteca (`data.ts`), la descarga que solo
  sube un contador y `SimulatedPlayer` (un reloj, no suena nada)
- `library.ts` — la biblioteca ya cargada (`loadLibrary()` antes de la primera pantalla y emite
  `library`), con `findBlock`, `countBy`, `onDisk` y `groupByStatus` (el orden de «Agrupar por estado»:
  descargados primero). «Ver archivos» sale agrupado de serie y con la miniatura de cada archivo; solo
  rehace las filas cuando cambia lo que suena, no con cada `player` del reloj (varios por segundo)
- `keys.ts` — la barra espaciadora reproduce/pausa en toda la app salvo donde escribe (`spaceTypes`,
  con test). Sin esto el espacio volvía a «pulsar» el último botón con el foco: reabría «Cambiar
  portada» o «Ver archivos». Pulsar en el Reproductor el archivo que ya suena solo abre Reproduciendo
  (no lo reinicia). Reproduciendo lleva el volumen (`buildVolume`, el mismo de la barra) cuando la barra
  no se enseña
- `player.ts` — `PlayerService`: la cola es `playerQueue.ts` (aleatorio = orden fijo, «anterior» vuelve
  a lo que sonó), la duración la da el motor (`ui.duration`), el segundo guardado se aplica al abrir,
  fallos seguidos pasan al siguiente hasta parar. `restoreLast()` al arrancar (en pausa) y `savePosition()`
  en cada pausa y al cerrar. `PLAYABLE`/`isPlayable`: 11 formatos probados en WebView2; AVI, FLV y ALAC
  no abren y salen atenuados con su motivo (sin convertir)
- `covers.ts` — las imágenes de las portadas encima del cuadro con degradado (`showPicture`), recorte
  `crop`/`placement` (lógica pura) y espera de miniaturas
- `search.ts` — el buscador del Reproductor y de «Ver archivos» (sin acentos ni mayúsculas);
  `relativeTime.ts` — el «Descargado» relativo
- `tasks.ts` — lo que el front deduce de la tarea (porcentaje, origen, marcar como vista, `liveText` y
  `progressLine` del Gestor)
- Gestor con una descarga en marcha (`views/manager.ts`): el bloque que descarga lleva contorno con un
  brillo que lo recorre, insignia «Descargando», «Archivo X de Y» y Detener en lugar de Sincronizar; sus
  hermanos se atenúan y no se pueden sincronizar; la fila padre lleva un chip en vivo y, plegada, una barra
  fina; se despliega sola al empezar y al acabar bien el bloque destella en verde ~3 s. Solo se rehace
  cuando cambia el bloque o el paso (`liveShape`); el resto es `patchLive` sobre `[data-live]`, y las
  animaciones conservan su fase entre rehechos (`--live-phase`). «Ir a la descarga» abre el Gestor con
  `open` + `focus` y lleva el bloque a la vista
- `urls.ts` — `isWebUrl` / `isPlaylistUrl`
- `transfer.ts` — los textos y el tono con que acaban «Copiar playlist» e «Importar» (lógica pura);
  las ventanas de esos dos flujos están en `overlays/transfer.ts` (carpeta o teléfono en los dos
  sentidos; `progressWindow` = barra + Cancelar, cerrar la ventana también cancela)
- `*.test.ts` — Vitest, solo lógica pura del front; `i18n.test.ts` comprueba que toda clave usada existe
- `main.ts` — el armazón: menú + página + barra del reproductor + capa de ventanas superpuestas
- `router.ts` — rutas por hash (`#/manager`, `#/player?b=pl1:MP3`); cada visita rehace la pantalla,
  lo que tiene que sobrevivir vive en `state.ts`. `onLeave()` para timers y suscripciones
- `state.ts` — estado de la UI (menú plegado, resumen oculto, tema, ubicación del reproductor, lo que suena, la tarea); no guarda nada: lo que dura entre arranques va por `preferences.ts`. `subscribe`/`emit` por tema (`library`, `player`, `tasks`, `theme`, `player-placement`)
- `i18n.ts` — TODO texto visible sale de aquí con `t(clave)`; las claves son las de la app Java
  cuando existen. Español aquí; inglés y catalán en `locales/` (mismas claves, comprobado
  por test). `setLanguage` avisa a `onLanguageChange` y `main.ts` rehace menú, barra y pantalla
- `dom.ts` — `h()` para construir nodos (nunca HTML en cadenas: los títulos vienen de YouTube)
- `icons.ts` — TODOS los iconos, de Lucide (`lucide`, ISC): SVG con `currentColor`, nítidos a cualquier
  escala. Se importan por nombre (solo los usados entran en el build). Icono nuevo = una línea en `ICONS`.
  La S ancha de Inicio se dibuja como SVG inline con el acento; el icono de la app sigue siendo verde.
- `scale.ts` — la escala de la interfaz: en la app es el zoom NATIVO del webview (`setZoom`, permiso
  `core:webview:allow-set-webview-zoom`), que recoloca todo; un `zoom` de CSS en la raíz hacía crecer
  la pantalla por debajo de la ventana. En el navegador, `zoom` sobre `#app` (alto en %, no `vh`).
  Ctrl + / Ctrl − / Ctrl 0 van por los mismos pasos que la lista de Ajustes; se guarda en `settings.json`
- `preferences.ts` — las preferencias de `settings.json`: `loadPreferences()` antes de la primera
  pantalla (tema, idioma, escala y sitio del reproductor aplicados ya), `save(patch)`, los `choose*`
  y `resetPreferences()` (vuelve a fábrica y lo aplica en caliente). localStorage ya no se usa
- `settingsSearch.ts` — el buscador de Ajustes (= `SettingsSearchIndex`): catálogo desde las claves
  `settings.<categoría>.<algo>` con `.desc`, en el idioma en uso
- `views/` — Inicio, Descarga por URL, Crear Playlist, Gestor, Reproductor, Reproduciendo, Ajustes
- `components/` — menú lateral (UNO para toda la app), barra del reproductor, portada, ventana
  superpuesta, piezas de formulario y el panel de resumen
- `overlays/` — Tareas, Ver archivos (filtro, estado y reproducción simulada por archivo o playlist),
  Ayuda de Importar, Actividad, confirmación, elegir dispositivo (`devicePicker.ts`: refresca los
  teléfonos cada 4 s; «¿Problemas con el dispositivo?» plegado dentro, no en otra ventana)
- `styles/theme.css` — siete bloques `[data-theme="…"]` con los tokens `--sp-*`: Oscuro (predeterminado)
  y Claro (oficiales), más Oscuro rojo, Oscuro azul, AMOLED, Claro rojo y Claro azul. Incluye
  `--sp-success`, `--sp-cover-a` y `--sp-cover-b`; el éxito no usa el acento y el vídeo conserva su
  degradado morado. `--sp-live`/`--sp-on-live` = algo en marcha (la descarga del Gestor): el acento, salvo en
  los temas rojos, donde un contorno rojo parecía un error; `--sp-glint` = el brillo que recorre ese
  contorno, más claro que él en los temas oscuros y más oscuro en los claros (pálido parecía un hueco). `styles/app.css` usa variables para los colores y contiene los selectores de tema y avisos.
- `components/themePicker.ts` — rejilla reutilizada por el botón del menú y Ajustes → Interfaz.
- `components/toasts.ts` — `notify({ tone, text })`, avisos apilados que caducan individualmente y viven
  en el armazón para sobrevivir a la navegación; los estados persistentes siguen en sus tarjetas.
  `persistent: true` = sin caducidad (el aviso de herramientas); `notify()` devuelve con qué quitarlo.
- `appInfo.ts` — la versión real de la app (`loadAppInfo()` al arrancar, `appVersion()`), «desconocida»
  si no es válida.

## Pendiente
- **La migración de la lógica sigue [MIGRATION-PLAN.md](MIGRATION-PLAN.md)** (fases 1–10), y cada
  test portado se apunta en [TEST-MAP.md](TEST-MAP.md)
- Adaptar el diseño a pantalla de móvil
- Herramientas: «Cancelar» una descarga de herramienta a medias y la comprobación de yt-dlp «una vez al
  día» del prototipo (`design/tools-setup-prototype.html`, fuera de git) no se hicieron
