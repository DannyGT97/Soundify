# Plan de migración de la lógica (Java → Tauri)

Objetivo: la misma funcionalidad que la app Java (`D:\Proyectos\soundifylocal`, v1.4.0) en este
stack, con sus tests, y **preparada desde el primer día para Android**, aunque Android se haga al
final. Las vistas ya existen y funcionan con `mock.ts`; este plan cambia lo que hay debajo.

Punto de partida medido (02/11/2026): 109 clases Java, ~480 tests JUnit en 45 ficheros (7 de ellos de
interfaz, con TestFX).

---

## 1. Decisiones de arquitectura

### 1.1 Dónde vive cada cosa

```
soundify-tauri/
  core/                 crate Rust SIN Tauri: modelo, archives, manifiesto, rutas, comandos de
                        yt-dlp, parseo de progreso... Toda la lógica pura y casi todos los tests.
  src-tauri/            la app: comandos #[tauri::command] finos que llaman a core/, procesos,
                        eventos al front, plugins. Sin lógica propia.
  src-tauri/plugins/    (más adelante) plugin Kotlin para Android
  src/                  el front de siempre. Habla con el back SOLO a través de src/api/
```

- **`core/` no conoce Tauri ni el sistema operativo.** Recibe por parámetro lo que en Java era
  global (`AppPaths`, `ToolManager`, el reloj). Así se prueba con `cargo test` en segundos y sirve
  igual en escritorio y en Android.
- **Los comandos de Tauri son wiring**, como los controllers en Java (regla 7 del CLAUDE.md de Java).
- **El front no llama a `invoke()` desde las vistas.** Todo pasa por `src/api/`, con dos
  implementaciones: `tauri.ts` (la real) y `mock.ts` (la de hoy, que se queda para `npm run dev`
  en el navegador y para los tests de interfaz). Cambiar de una a otra es una línea.

### 1.2 Las tres costuras para Android

Lo que en escritorio y Android funciona distinto va detrás de una interfaz, para que el resto no se
entere. `Downloader` y `Storage` son traits de `core/`; `PlayerBackend` es una interfaz del front
(`src/api/types.ts`), porque en escritorio quien suena es el propio webview:

| Trait | Escritorio | Android |
|---|---|---|
| `Downloader` (lanzar yt-dlp, leer su salida, pararlo) | yt-dlp + ffmpeg + deno como procesos (`std::process` / plugin shell) | plugin Kotlin sobre `youtubedl-android` (trae Python, yt-dlp y ffmpeg) |
| `Storage` (listar, existe, mover, borrar, escribir el manifiesto) | sistema de ficheros normal | Storage Access Framework / MediaStore: la carpeta es una URI `content://`, no una ruta |
| `PlayerBackend` | `<audio>`/`<video>` del webview con `convertFileSrc` | servicio nativo (Media3) para que siga sonando con la pantalla apagada |

Consecuencia que hay que respetar desde ya: **en el modelo, una carpeta no es un `PathBuf`**, es un
`Location` (ruta o URI). El criterio de Java «descargado = el fichero existe» se mantiene, pero
preguntado a `Storage`, nunca a `std::fs` directamente. Para los tests, `Storage` tiene una
implementación en memoria y otra sobre carpeta temporal.

### 1.3 Compatibilidad de datos

La app Tauri **lee los mismos ficheros que la Java**: `settings.json`, `data/history/archives/pl_*/…/data.json`
+ `archive.txt`, `tasks.json` y `soundify-playlist.json`. Quien pase de una a otra no pierde nada.
- serde con `#[serde(default)]` y sin `deny_unknown_fields` = el `Json` tolerante de Java.
- Tests de compatibilidad con **ficheros reales de ejemplo** copiados a `core/tests/fixtures/`
  (anonimizados). Nunca se lee el `data/` de verdad.
- Dónde vive `data/`: en Java cuelga de la carpeta de la app (portable). En Tauri escritorio se
  mantiene igual para el portable; en Android es el directorio privado de la app. Lo resuelve un
  único `AppPaths` en `src-tauri`, que se pasa a `core/`.

### 1.4 Comunicación back → front

- Peticiones: `invoke` (listar playlists, leer ajustes…).
- Progreso de descarga, actividad y estado de tareas: `Channel` de Tauri (ordenado, por tarea) o
  eventos globales para lo que escucha todo el armazón (`tasks`, `player`). Los temas de
  `state.ts` (`subscribe/emit`) se alimentan de ahí; las vistas no cambian.
- El estado de las tareas en segundo plano pasa a vivir en Rust (`BackgroundTasks`): si el webview
  se recarga, no se pierde la descarga en curso.

---

## 2. Tests

| Capa | Herramienta | Qué cubre | Equivale en Java a |
|---|---|---|---|
| Lógica pura | `cargo test` en `core/` (+ `tempfile`) | archives, manifiesto, rutas, comandos, parseo, versiones, historial… | los tests de `utils/` y `service/` |
| Comandos Tauri | `cargo test` en `src-tauri` con `tauri::test` (mock runtime) | que cada comando llama bien a `core/` y emite sus eventos | — (nuevo) |
| Lógica del front | **Vitest** | `PlayerQueue` si vive en TS, `RelativeTime`, índice del buscador de ajustes, claves de i18n, router | `I18nKeysTest`, helpers de controllers |
| Interfaz | **Playwright** contra `npm run dev` con la API mock | navegación, plegado del menú, Gestor, formularios, tareas, Ajustes, temas, avisos | `ui/*UiTest` (TestFX) |
| Humo de la app real | **WebdriverIO + tauri-driver** (Windows y Linux) | arranca, carga Inicio, la API real responde | — (nuevo, pocos casos) |
| Contra yt-dlp de verdad | `cargo test -- --ignored` | una descarga real corta, consulta de título | se hacía a mano |

Reglas:
- **Cada test de Java se porta con su nombre** (en inglés, como ya están). Al portar una clase, se
  portan sus tests en el mismo paso, no después.
- **Los tests «regresión:» se portan los primeros y 1:1.** Si uno falla en Rust, se ha reintroducido
  un bug que ya estaba arreglado en Java.
- Un fichero `TEST-MAP.md` lleva la cuenta: test Java → test nuevo (o «descartado: motivo», p. ej. los
  de `Motion` o `WindowChrome`, que no tienen equivalente).
- Nada de red ni de `data/` real en los tests normales; lo que lo necesite va marcado `#[ignore]`.
- `npm run check` = `tsc` + `vitest` + `cargo test` + `cargo clippy`. Es lo que se ejecuta antes de
  dar algo por terminado. Playwright va aparte (`npm run test:ui`): necesita el servidor y un
  navegador, y se ejecuta al tocar pantallas.

---

## 3. Fases

Cada fase termina con la app compilando, sus tests en verde y la parte correspondiente de
`mock.ts` sustituida por la real. Orden pensado para que lo que más tests tiene (y más riesgo de
romper datos) vaya primero y aislado.

### Fase 0 · Cimientos — HECHA (02/11/2026)
Resultado: `npm run check` → tsc ok · vitest 20 · cargo test 8 · clippy ok; `npm run test:ui` → 12.
Lo que hay está descrito en CLAUDE.md. Decisiones tomadas al hacerla:
- `PlayerBackend` vive en el front (ver 1.2) y `player.ts` ya habla con él: la fase 7 solo cambia
  `SimulatedPlayer` por el motor real.
- La biblioteca se carga una vez antes de la primera pantalla (`library.ts`) y las vistas la leen sin
  `await`; la API real avisará con el tema `library` cuando cambie.
- `DownloadRequest` ya lleva lo que cada pantalla pide (URL, formato, carpeta, numerar), aunque el
  mock lo ignore.

Lo que se pidió:
- ~~Repositorio~~ hecho: https://github.com/DannyGT97/Soundify (rama `main`, primer commit con las vistas).
- Workspace de Cargo con `core/` y `src-tauri/`. Vitest y Playwright instalados, `npm run check`.
- **`npm run check` con salida corta**: si todo pasa, una línea por herramienta con su recuento
  (`vitest: 12 ok`); si algo falla, solo lo que falla con su error. Nada de listar los tests que
  pasan ni el progreso de compilación (reporters `dot`/`line`, `cargo test -q`, `clippy -q`). Es lo
  que se lee después de cada cambio, y la salida completa cuesta miles de líneas de contexto.
- `src/api/` con la interfaz completa y la implementación mock (moviendo lo de `mock.ts`, `player.ts`
  y `tasks.ts` detrás de ella). Las vistas pasan a usar `api.*`.
- Los tres traits del 1.2 declarados, aunque solo tengan la implementación de escritorio.
- Tests: Playwright de navegación y de que cada pantalla carga (= `FxmlLoadUiTest`, `NavigationUiTest`).

### Fase 1 · Modelo y archives (el corazón) — HECHA (02/11/2026)
Resultado: `npm run check` → tsc ok · vitest 20 · cargo test 99 (95 de `core` + 4 de compatibilidad
con Java) · clippy ok; `npm run test:ui` → 12. Probado en solo lectura contra el `data/` real de la
app Java: todos los archives y `settings.json` cargan sin errores. Decisiones tomadas al hacerla:
- `core::archive::ArchiveStore` es la fachada (un `Storage`, la carpeta de archives y un `Clock`);
  `io`, `sync`, `query` y `maintenance` son sus cuatro piezas, como en Java.
- `core::library` traduce los archives a lo que pinta el front (`Playlist`/`FormatBlock`/`Track`).
  `seconds` va a 0 (el archive no guarda duraciones: las pondrá el reproductor) y `sync` siempre a
  `true` (en Java ya no vive en el archive).
- **Compatibilidad**: Java escribe `null` en los `String` que nunca rellenó (`"cookiesFile": null`)
  y eso hacía que serde rechazara `settings.json` ENTERO y se quedara con los valores de fábrica sin
  avisar. Los campos de texto no opcionales toleran `null`. Lo cubre `core/tests/java_compat.rs`.
- Carpeta de datos: `data/` junto al ejecutable (portable, como en Java); `SOUNDIFY_DATA_DIR` la
  cambia (p. ej. para abrir el `data/` de la app Java en desarrollo). **Ojo: el arranque corre
  `reconcile_all`, que ESCRIBE en esa carpeta.**
- El front elige la API real solo dentro de Tauri (`insideTauri()`); `npm run dev` y Playwright
  siguen con el mock. La descarga sigue en el mock hasta la fase 5.
- `RelativeTime` se queda en el front (`relative(addedDaysAgo)`); su test va con la fase 7.

Lo que se pidió:
`UserSettings`, `ArchiveData`, `ArchiveEntry`, `PlaylistManifest`, `TaskRecord`; `SettingsManager`;
`DownloadPathResolver`; `ArchiveIO`, `ArchiveSync`, `ArchiveQuery`, `ArchiveMaintenance` y la fachada
`ArchiveStore`; `TaskHistory`; `RelativeTime`.
- Tests: `ArchiveStoreTest` (942 líneas), `ArchiveIOTest`, `ArchiveSyncTest`, `ArchiveQueryTest`,
  `DownloadPathResolverTest`, `TaskHistoryTest` + compatibilidad con fixtures reales.
- Al acabar: el Gestor, «Ver archivos» e Inicio enseñan las playlists reales (solo lectura).

### Fase 2 · Copiar e importar — HECHA (02/11/2026)
Resultado: `npm run check` → tsc ok · vitest 39 · cargo test 143 · clippy ok; `npm run test:ui` → 15.
Decisiones tomadas al hacerla:
- `core::transfer` = `PlaylistTransfer`. La hora entra como `&str` (la de `ArchiveStore::now()`),
  igual que en el resto de `core`: los tests la fijan.
- Los errores llegan al front como **clave de i18n + argumentos** (`CommandError`, desde
  `TransferError::key/args`), no como texto: así siguen el idioma en uso. Las claves son las de Java.
- El manifiesto se escribe en los mismos cuatro momentos que en Java (registrar, `commit`,
  `syncPlaylistMembership` y la reconciliación del arranque); `relinkFromManifest` corre al registrar
  y al dar de alta ids nuevos.
- Espacio libre del destino con `fs4` (antes de copiar, como `usableSpace` en Java).
- Copiar va en un hilo aparte (`spawn_blocking`) con progreso por el evento `copy-progress` y un flag
  de cancelar; **registrar la copia en el Gestor lo hace el back** al terminar (como en Java), y el
  resultado dice cómo fue (`registered` / `alreadyRegistered` / `failed`).
- Los textos del resultado (copiar e importar) están en `src/transfer.ts`, lógica pura con tests; las
  ventanas en `src/overlays/transfer.ts`. Cerrar a mano la ventana de progreso cancela la copia (si no,
  seguiría copiando sin que se viera).
- Ya funcionan de verdad: «Copiar a una carpeta local» (menú del Gestor), «Explorar archivos» de la
  ayuda de Importar, «Abrir carpeta» en cada formato del Gestor y el botón «Carpeta local» de Descarga y
  Crear Playlist (diálogo nativo con `tauri-plugin-dialog`; abrir carpetas con `tauri-plugin-opener`).

Lo que se pidió:
`PlaylistTransfer` (manifiesto siempre en la carpeta, `relinkFromManifest`, `keepWhatWasKnown`,
`findImportable` con su tope, no copiar sobre sí misma). Diálogos con `tauri-plugin-dialog`, abrir
carpetas con `tauri-plugin-opener` (= `SystemOpener`).
- Tests: `PlaylistTransferTest` (826 líneas).

### Fase 3 · yt-dlp sin ejecutarlo — HECHA (02/11/2026)
Resultado: `npm run check` → tsc ok · vitest 43 · cargo test 219 · clippy ok; `npm run test:ui` → 15.
Decisiones tomadas al hacerla:
- Todo en `core::ytdlp` (`command`, `progress`, `cleaning`, `unavailable`, `client`, `query`) más
  `core::version` y `core::log_format`. **Nada ejecuta yt-dlp**: cada consulta da sus argumentos (sin
  el ejecutable, que lo pone el `Downloader`) y una función que lee la salida. Ejecutarlo es la fase 5.
- Los formatos son cadenas, no enums: audio o vídeo lo decide `library::is_video_format`. Los flags de
  audio por formato van en una tabla (en Java era un `switch`), con un test que fija los de Java.
- `ProgressParser::parse` devuelve el progreso en vez de llamar a un callback.
- `unavailable::reason_key` da una **clave de i18n** (`unavailable.*`), no el texto: el archive sigue
  guardando el error crudo de yt-dlp y el motivo se calcula al leer, como en Java. `Track.reason` lo
  lleva al front y «Ver archivos» dice «No disponible, vídeo privado» (`statusText`).
- `log_format` solo da el texto de cada línea (mismo formato de fecha que Java); escribir y rotar los
  ficheros llega con el registro de la app.

Lo que se pidió:
`YtDlpCommandBuilder`, `YtDlpProgressParser`, `OutputCleaning`, `UnavailableReason`,
`YtDlpClientResolver` (la parte de decidir), parseo de `YtDlpQueryService`, `VersionCompare`,
`ActivityLogFormatter`.
- Tests: los de cada una (todo lógica pura).

### Fase 4 · Herramientas externas — HECHA (03/11/2026)
Resultado: `npm run check` → tsc ok · vitest 46 · cargo test 263 · clippy ok; `npm run test:ui` → 15.
Decisiones tomadas al hacerla:
- `core::http` (trait `Http`, `ReqwestHttp`; los ficheros se escriben en `.part` y se renombran) y
  `core::tools` (`ToolPaths`, `zip_extract`, `health`, `updaters`). Los updaters reciben el `Http`, la
  fecha de hoy y el `ToolHealth`: se prueban con un `Http` falso y zips hechos en memoria, sin red.
- Las herramientas **no van dentro del ejecutable**: en Windows se bajan a `data/systemtools/` con la
  misma estructura que Java. Los updaters solo corren con herramientas gestionadas (Windows); en Linux
  se usan las del sistema y no se baja nada (Java corría los de yt-dlp y ffmpeg también ahí, para nada).
- `src-tauri/src/process.rs` = `YtDlpProcess` (el `Downloader` real: `--encoding utf-8`, variables de
  Python y Deno en el PATH). stdout y stderr se leen cada uno en su hilo, sin mezclar (el cuelgue de
  la playlist de 726 que Java resolvió mezclándolos). Parar mata el **árbol** entero (`taskkill /T`,
  o el grupo de procesos en Unix).
- `src-tauri/src/bootstrap.rs` = `Bootstrap`: `reconcile_all`, updaters y la caché de formatos, cada
  cosa en su hilo y nada fatal. Los formatos se preguntan DESPUÉS de actualizar yt-dlp; si el front
  pregunta antes, recibe todos (caché vacía = «no se pudo comprobar»).
- El aviso de herramientas es un toast **persistente** (`notify({persistent})`, evento `tool-health`
  y comando `tool_health` para lo que pasó antes de escuchar), no una franja.
- **Revisado tras probar la fase 5**: ffmpeg SÍ viaja con la app (como en Java). Bajarlo en el primer
  arranque tardaba muchos minutos (gyan.dev a ~140 KB/s) sin decir nada, y cerrar la app lo empezaba de
  cero. Ahora es un recurso del bundle que el arranque copia a `data/systemtools/` (`seed_ffmpeg`); la
  descarga queda de respaldo. Cualquier herramienta que se baje enseña su porcentaje en un toast vivo
  (`InstallProgress`, evento `tool-downloads`). Reanudar una descarga cortada: no se hizo.
- **Revisado otra vez (03/11/2026)**: el arranque ya no baja ninguna herramienta que falte. Se instalan
  desde el asistente del primer arranque, Ajustes → Herramientas o el aviso de Descarga (`tools.rs` +
  `src/tools.ts`, ver CLAUDE.md); con `autoUpdates` el arranque solo actualiza un yt-dlp existente.
- La versión de la app es la real (`getVersion()`, `src/appInfo.ts`); si no es válida, «desconocida».
- Nuevo test de i18n: toda clave que el back entrega en un literal de Rust existe en el diccionario.

Lo que se pidió:
`ToolManager`, `YtDlpUpdater`, `FfmpegUpdater`, `DenoUpdater`, `HttpDownloader` (`reqwest`),
`ZipExtractor` (`zip`), `ToolHealth`, `Bootstrap` (orden de arranque y `reconcileAll`).
- Decisión: yt-dlp **se descarga y actualiza solo en `data/systemtools/`** como en Java (cambia cada
  pocas semanas, no puede ir congelado dentro del ejecutable). Solo en escritorio.
- Tests: `YtDlpUpdaterTest`, `ZipExtractorTest` + uno `#[ignore]` que baja de verdad.

### Fase 5 · Motor de descarga — HECHA (03/11/2026)
Resultado: `npm run check` → tsc ok · vitest 58 · cargo test 312 · clippy ok.
Decisiones tomadas al hacerla:
- `core::engine`: `download` (= `DownloadCoordinator` + `DownloadThreadService`: `Engine::run(&Job, events)`,
  `Job::Single`/`Job::Playlist`, `Job::update` para el Actualizar/Sincronizar del Gestor), `safety_net`
  (= `SafetyNetService`), `tasks` (= `BackgroundTasks`) y `activity` (= `ActivityLog`). Nada de UI: lo que
  hay que enseñar sale por el trait `DownloadEvents`.
- `src-tauri/src/downloads.rs` lanza el motor en su hilo (una descarga a la vez; un segundo `start` se
  ignora) y lo convierte en eventos: `tasks` (la vista entera), `activity` (una línea: cruda de yt-dlp o
  clave de i18n + argumentos), `activity-reset` y `notice` (→ toast). `StopFlag` nuevo en cada descarga.
  El progreso se manda solo cuando cambia el porcentaje entero.
- Los textos de Actividad y del historial son las CLAVES de Java (`activity.*`, `retry.*`, `failReason.*`):
  un historial escrito por la app Java se sigue leyendo, y un resumen que Java guardó ya traducido se
  enseña tal cual.
- Historial en `data/history/tasks.json`, el mismo fichero que Java; estados OK, WARNING, STOPPED, FAILED.
- Sin vigilante de cuelgues: Java no lo tenía.
- Al acabar una tarea el front relee la biblioteca (Gestor y Reproductor al día sin reabrir).
- Tras la primera prueba a mano (03/11/2026):
  - «Crear» registra ANTES de descargar (como Java, `NewPlaylistController.doCreate`): comando
    `create_playlist` → `Engine::create_playlist`; «Crear y descargar» = eso + un `update`. Antes la
    playlist solo se registraba al final de la descarga y el Gestor no la enseñaba.
  - Descarga por URL va a la carpeta elegida, sin subcarpeta `soundify_<formato>` y **sin numerar**
    (cambio respecto a Java, donde decidía el offset de la carpeta: aquí la carpeta es cualquiera). Su
    limpieza solo borra restos que empiecen como lo que bajó; la de playlists nunca toca el manifiesto.
  - `--ffmpeg-location` apunta a la carpeta de `ffmpeg.exe`/`ffprobe.exe`, y sin ffmpeg no se descarga
    (se quedaban los .mp4 sin convertir y la limpieza los borraba).

Lo que se pidió:
`DownloadThreadService` + `DownloadCoordinator` + `SafetyNetService` detrás de `Downloader`;
`BackgroundTasks`, `ActivityLog`, avisos (`NoticeCenter` → `notify()` de `toasts.ts`). Parar una
descarga mata el árbol de procesos entero (en Windows, yt-dlp lanza ffmpeg).
- Tests: `SafetyNetServiceTest`, `YtDlpQueryServiceTest`, `BackgroundTasksTest`, `ActivityLogTest`, y
  el ciclo de una tarea con un `Downloader` falso que emite salida grabada de yt-dlp real.
- Al acabar: Descarga por URL, Crear Playlist, Sincronizar/Actualizar del Gestor y el indicador de
  tareas son reales. `tasks.ts` deja de simular.

### Fase 6 · Portadas — HECHA (03/11/2026)
Resultado (junto con la 7): `npm run check` → tsc ok · vitest 90 · cargo test 341 · clippy ok · Playwright 16.
Decisiones tomadas al hacerla:
- `core::cover_art` = `CoverArt`: portada elegida (`cover.jpg` en la carpeta `pl_...` del archive, la
  misma para todos los formatos), si no la miniatura del primer vídeo (por `playlistIndex`); miniaturas
  `hqdefault` bajadas UNA vez a `data/cache/thumbnails/<id>.jpg` en un solo hilo (`ThumbnailFetcher`),
  un fallo no se reintenta hasta el siguiente arranque. La imagen se valida por sus primeros bytes
  (JPEG/PNG/WebP/GIF), no por la extensión.
- **Sin crate `image`**: el recorte (franjas negras del 4:3 de YouTube + forma del cuadro) es lógica pura
  en `src/covers.ts` (`crop`/`placement`) y se aplica colocando el `<img>` en porcentajes sobre el cuadro
  con degradado, que queda debajo como respaldo. Nada se reescala en disco.
- Al front por `convertFileSrc`; el ámbito del protocolo `asset` se abre EN MARCHA (`allow_directory` /
  `allow_file`): la caché de miniaturas, los archives, cada carpeta de música que lista la biblioteca y la
  imagen recién elegida. En `tauri.conf.json` el ámbito va vacío.
- La miniatura que aún no está llega con el evento `thumbnail-ready`; el front se apunta a la espera
  ANTES de preguntar, para no perder una respuesta que llegue entre medias.
- La barra, el mini reproductor y Reproduciendo enseñan la miniatura del VÍDEO de la pista: la carátula
  incrustada en el archivo (Java la leía de los metadatos de JavaFX) no está al alcance del webview.
- Crear Playlist pone la portada sola con una URL válida (`first_video_id`, con espera de 700 ms al
  teclear); «Cambiar portada…» en el menú ⋯ del Gestor y en el resumen de Crear Playlist.

### Fase 7 · Reproductor — HECHA (03/11/2026)
Decisiones tomadas al hacerla:
- **Formatos, probados uno a uno** en Edge (el mismo motor que WebView2) con muestras hechas con ffmpeg:
  abren MP3, OPUS, M4A (AAC), FLAC, AAC (ADTS), VORBIS (.ogg) y WAV; en vídeo MP4, MKV, MOV y WEBM (H.264,
  VP9 y AV1 con Opus/AAC). **No** abren ALAC (.m4a), AVI (mpeg4 ni h264) ni FLV (flv1 ni h264).
  `PLAYABLE` (en `player.ts`) pasa de 4 formatos a 11; los que no abren **siguen atenuados con su motivo,
  sin convertir con ffmpeg** (lo mismo que decidió Java). En Linux (WebKitGTK) depende de los plugins de
  GStreamer instalados; Android tendrá su propio motor.
- Motor de escritorio: `src/api/htmlPlayer.ts`, UN `<video>` para audio y vídeo, en un contenedor oculto
  dentro de `<body>`. Reproduciendo lo MUEVE a su hueco y lo devuelve al salir en la misma tarea (un
  elemento multimedia sacado de la página se pausa solo). `object-fit: contain` y tamaño del hueco: el
  vídeo nunca empuja el tamaño de la pantalla (el fallo del `MediaView` de Java).
- `PlayerBackend` gana `onDuration` (la duración la da el motor al abrir; el archive no la guarda),
  `onError`, `view` y `park`. `SimulatedPlayer` sigue siendo el de `npm run dev` y Playwright.
- `player.ts` = `PlayerService` sobre `PlayerQueue` (TS): el salto al segundo guardado espera a que el
  archivo abra; un archivo que falla pasa al siguiente y, si fallan todos seguidos, se para; fin de la
  lista sin repetir = se queda cargada al principio; «anterior» pasados 3 s reinicia el archivo.
- La memoria (volumen y último archivo con su segundo) vive en `settings.json` con los mismos campos que
  Java (`playerVolume`, `lastPlayed*`): comandos `player_memory` / `save_player_memory`. Se guarda al
  cargar un archivo, en cada pausa, al soltar el volumen y al cerrar la ventana (`onCloseRequested`, que
  necesita el permiso `core:window:allow-destroy`). Al arrancar vuelve EN PAUSA; si ya no está en disco,
  no sale barra. El ✕ lo olvida.
- Buscador del Reproductor y de «Ver archivos» sin mayúsculas, sin acentos y sin espacios alrededor
  (`src/search.ts`, como Java).
- Sin probar a mano todavía: sonido y vídeo reales dentro de Tauri, volver al segundo tras reiniciar y
  las portadas reales.
- Añadido después (03/11/2026, a partir de `design/manager-active-download-prototype.html`): el Gestor
  enseña qué bloque se está descargando (contorno, insignia, progreso, Detener, hermanos en espera, chip
  en la fila padre, destello verde al acabar) y «Ver archivos» se actualiza archivo a archivo con el
  evento `file-downloaded`. «Ir a la descarga» lleva al bloque.

### Fase 8 · Ajustes y armazón — HECHA (03/11/2026)
Decisiones tomadas al hacerla:
- **Preferencias en `settings.json`** (comandos `preferences` / `save_preferences` / `reset_preferences`):
  tema, idioma, escala, sitio del reproductor, cookies y los interruptores. localStorage ya no guarda
  nada. `appearance` lleva el id completo del tema (hay más de dos) y `theme` sigue escribiendo
  `DARK`/`LIGHT` para que la app Java lo lea igual. `src/preferences.ts` las carga ANTES de pintar la
  primera pantalla (nada sale un instante en otro tema o idioma) y aplica cada cambio en caliente.
- **Idiomas**: `en` y `ca` en `src/locales/`, traídos de los `messages_*.properties`. Cambiar de idioma
  rehace menú, barra y la pantalla en la que se está (sin perder dónde estaba). En Java el idioma solo
  valía tras reiniciar.
- **Buscador** (`src/settingsSearch.ts`): igual que `SettingsSearchIndex`, desde las claves de i18n
  (`settings.<categoría>.<algo>` con su `.desc`), en el idioma en uso, sin acentos ni mayúsculas; el
  título pesa más que la descripción.
- **Logs**: logger propio en `core/src/logs.rs`, NO `tauri-plugin-log`: ese plugin pone la fecha en el
  nombre de los rotados y aquí se quieren los mismos `soundify-N.log` / `-activity-N` / `-retries-N`
  de Java (10 MB, 10 de cada).
- **Cookies**: selector real, «Quitar» y una línea de estado que no caduca (verde si el fichero está,
  ámbar si la ruta ya no lleva a nada, atenuada si no hay cookies).
- **Restablecer**: con confirmación, vuelve a los valores de fábrica y los aplica al momento (tema,
  idioma, escala), con aviso de hecho.
- **No portado por no hacer nada en Java**: `downloadThumbnails`, `autoUpdates` y `downloadsFolder` se
  guardaban y nadie los leía. El primero se conserva como interruptor (el campo existe) sin efecto,
  igual que en Java; los otros dos no salen en Ajustes.

### Fase 9 · Solo escritorio — HECHA (03/11/2026)
Decisiones tomadas al hacerla:
- **Actualización de la app SIN `tauri-plugin-updater`**: updater propio en `core/src/updater/`
  (`download`, `install`, `verify`, `release_notes`) + `src-tauri/src/app_update.rs`, con el diseño de
  Java: ZIP portable + su hash firmados con Ed25519 (la MISMA clave que la app Java), publicados como
  release de **GitHub** (`DannyGT97/Soundify`, `updater::REPOSITORY`). El plugin pide su propio
  manifiesto y su propio formato de firma: habría que cambiar cómo se publica. Comprueba 8 s después de
  arrancar (`STARTUP_DELAY`), con 30 s de tope por petición; eventos `app-update` y
  `app-update-progress`.
- **Los assets llevan sufijo `-tauri`**: las releases 1.x de Java tienen versión MAYOR que esta app y,
  sin el sufijo, la comprobación propondría «actualizar» a la app Java. Al revés también fallaba (el
  portable Java busca el ZIP por prefijo y casaría con `...-tauri.zip`, y el proyecto de GitLab ya tiene
  un tag `v1.0`): por eso las releases Tauri van en GitHub y no en el proyecto de GitLab. Solo se
  aceptan descargas de `github.com/DannyGT97/Soundify/releases/download/` (`verify::is_trusted_source`);
  borradores y pre-releases no se proponen nunca.
- **Publicar una versión** (la 1.0.0 fue la primera, 03/11/2026): subir la versión en
  `src-tauri/Cargo.toml`, `tauri.conf.json` y `package.json`; `npm run tauri build -- --no-bundle`;
  `node scripts/package-portable.mjs --key <clave privada .pem>` (deja en `release/` el ZIP, su
  `.sha256` y su `.sig`, y se niega si la clave no casa con `core/keys/update-signing.pub`); y
  `gh release create v<versión> -R DannyGT97/Soundify release/*` con las notas en Markdown.
- **Instalar** espera a que el `.exe` se pueda escribir (`wait_until_writable`: la app que se cierra
  aún lo tiene abierto) en vez de un `waitPidFromArgs` como Java, que no se porta. En una compilación
  de desarrollo se niega con `update.devBuild` (no hay portable que sustituir).
- **Notas de la versión** como texto plano (`release_notes`), nunca HTML de la release.
- **Pendiente antes de publicar**: firma de código. El problema de Smart App Control que resolvía
  `AppLauncher` vuelve con un `.exe` sin firmar.
- **Dispositivos MTP** con el crate `windows` 0.62 y COM tipado: `Shell.Application` para listar y
  navegar, `IFileOperation` para copiar y borrar (`src-tauri/src/mtp.rs`). Cada llamada va en su
  propio hilo y apartamento COM con un tope: conectividad 15 s, explorar 60 s, transferir 90 s (un
  teléfono que se bloquea no cuelga la app). Detrás del trait `PortableDevice` (`core/src/device/`),
  con `NoDevices` fuera de Windows y un `FakeDevice` para los tests.
- **La carpeta base de un archive de dispositivo** para el hash es `Uri("mtp:<id>:<ruta>")`: la misma
  playlist en el ordenador y en el teléfono son dos playlists locales distintas, como en Java.
- **DeviceSync simplificado** (`core/src/engine/download/device.rs`): un vídeo cada vez y un solo
  `player_client` por intento, preparado en local como `<id>.<ext>` y empujado después; lo que bajó y
  no llegó al teléfono se queda en la carpeta de preparación y la siguiente sincronización lo empuja
  sin volver a YouTube. NO es una rama dentro de `Engine::run` (su autorreparación daría por no
  borradas todas las entradas pendientes del teléfono).
- **«Eliminar todo» ya conectado** (antes solo borraba el registro): borra archivos y registro, en
  local y en el teléfono (`core/src/archive/device.rs`, rechequeos a 1 s y 2 s porque MTP tarda en
  reflejar el borrado), con una ventana de al menos 600 ms y aviso de lo que quede sin borrar.
- **«¿Problemas con el dispositivo?»** va plegado (`<details>`) dentro del selector: en Java era otra
  ventana, y aquí un segundo overlay cerraría el selector.
- **Sin probar con un teléfono real**: todo lo de MTP está cubierto con `FakeDevice` y el mock del
  frontend, no contra hardware.

### Fase 10 · Android
- Plugin Kotlin con `youtubedl-android` que implementa `Downloader` (actualiza yt-dlp con su propio
  `updateYoutubeDL`). Las descargas, en un *foreground service* con notificación.
- `Storage` sobre SAF: el usuario elige la carpeta de música una vez y se guarda el permiso.
- `PlayerBackend` nativo con Media3 (sonido en segundo plano, controles en la pantalla de bloqueo).
- Fuera en Android: MTP (el teléfono ES el dispositivo), autoactualización (va por tienda o APK) y deno.
- Diseño de pantalla móvil (ya anotado en Pendiente).

---

## 4. Lo que hay que decidir (no bloquea la fase 0–1)
0. **Dos variantes del instalable** (propuesta 03/11/2026): «completa» con yt-dlp, ffmpeg y Deno dentro, y
   «ligera» que los baja en el primer arranque (el asistente ya está hecho, `overlays/toolsSetup.ts`). El código ya
   vale para las dos (`seed_ffmpeg` copia lo incluido y si no hay nada se descarga): la diferencia es solo qué
   recursos lleva el bundle. A decidir en la fase 9: nombres de los ficheros, que el autoactualizador baje la
   MISMA variante que hay instalada, y la licencia de ffmpeg (GPL) junto al binario en la completa.
   **En móvil (fase 10) las herramientas son otras** (`youtubedl-android` trae Python, yt-dlp y ffmpeg; Deno no
   aplica): el asistente y la pestaña Herramientas tendrán que adaptarse entonces.
1. Dónde vive `data/` en escritorio: junto al ejecutable (portable, como hoy) o en `%APPDATA%`.
2. Firma de código del `.exe` (ver fase 9).
3. ~~Formatos reproducibles~~: decidido en la fase 7 (los que abre el webview; AVI, FLV y ALAC atenuados).
4. Prioridad de MTP frente a Android: ¿fase 9 antes o después de la 10?
5. Si la app Java sigue recibiendo cambios durante la migración, cada cambio hay que portarlo:
   conviene congelarla en lo funcional.

## 5. Reglas para quien implemente (Codex o quien sea)
- Una fase por encargo. No empezar la siguiente sin que la anterior esté en verde y revisada.
- Leer la clase Java y **su test** antes de escribir la versión nueva. Los comentarios de «por qué»
  se traen con el código (traducidos si hiciera falta, ya están en inglés).
- No inventar comportamiento: si el Java hace algo raro, tiene un motivo documentado; se copia.
- Código, comentarios y nombres de test en inglés. Textos visibles en español por `i18n.ts`;
  «archivos», nunca «canciones».
- Nada de tests que escriban en el `data/` real.
- Sin commits ni publicaciones salvo que se pidan. Nunca ejecutar `publish-release.ps1`.
- Al cerrar cada fase: actualizar CLAUDE.md, `TEST-MAP.md` y este plan (marcar la fase como hecha).
