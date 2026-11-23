# CLAUDE.md — Soundify (Tauri)

Port de Soundify (la app Java/JavaFX, repositorio local `soundifylocal`) a Tauri 2, pensando en
llevarla a Android. Todas las pantallas están hechas; la lógica se está portando por fases según
[MIGRATION-PLAN.md](MIGRATION-PLAN.md) (hechas: fases 0 a 9). Dentro de Tauri la biblioteca ya es la
real (los archives de `data/`); lo que aún no ha llegado sale de la API de mentira (`src/api/mock/`),
que es también lo que usan `npm run dev` y Playwright.

Repositorio: https://github.com/DannyGT97/Soundify (GitHub, rama `main`, GPL-3.0).

## Stack
- Tauri 2 + Vite + TypeScript sin framework (igual que `lienzo`); Rust en un workspace de Cargo
- Dev: `npm run tauri dev` · solo el navegador: `npm run dev` (puerto 5180)
- APK de prueba: `npx tauri android build --debug --apk --target aarch64 --target x86_64` (con `ANDROID_HOME`,
  `NDK_HOME` y `CARGO_PROFILE_DEV_DEBUG=0`, si no pesa 420 MB; ~100 MB las dos arquitecturas. Borra antes el APK anterior: Gradle lo reescribe encima y conserva su tamaño; sale en `src-tauri/gen/android/app/build/outputs/apk/`).
  Firmado con la clave de depuración y con otro nombre de paquete (`com.dannygt97.soundify.debug`): NO es la app de las releases y convive con ella en el mismo aparato.
  El APK que se publica es el de RELEASE, firmado con el keystore propio (ver «Actualizar la app en Android»)
- **Comprobar: `npm run check`** (tsc + vitest + cargo test + clippy, salida corta: una línea por
  herramienta, o solo lo que falla). Interfaz aparte: `npm run test:ui` (Playwright). Build: `npm run build`
- **Actualizar la app en Android (11/11/2026, SIN comprobar en la tablet):** Ajustes → Actualizaciones también está en Android (sin el interruptor
  de herramientas, que allí vive en Herramientas). `app_update.rs` hace lo mismo que en escritorio (comprobar, bajar a `filesDir/updates`, verificar SHA-256 + Ed25519),
  pero en lugar de `stage`/`hand_over` entrega el APK al instalador del sistema: `SoundifyPlugin.installApk` (FileProvider + `ACTION_VIEW`;
  solo acepta `.apk` dentro de `filesDir/updates`, `file_paths.xml`) tras `REQUEST_INSTALL_PACKAGES`. Antes de bajar nada se comprueba el permiso «Instalar
  aplicaciones desconocidas» (`allowInstall`/`mayInstall`; si falta abre su pantalla y el error es `update.android.allowInstall`). Android pide confirmar y no
  se hace `app.exit(0)`. **Para que instale por encima hace falta el MISMO paquete, la MISMA clave de firma que el instalado y un `versionCode` mayor**
  (`tauri.properties`: 1.1.5 → 1001005, 1.1.6 → 1001006). Por eso la build de depuración (`.debug`, clave de depuración) NO se actualiza con
  un APK de release: para probar la actualización hay que instalar la build de RELEASE de la versión anterior. Ninguna release publicada
  lleva APK todavía: hasta entonces Android dice «al día». (**La 1.1.6, publicada el 13/11/2026, es la primera con APK**: la 1.1.5 instalada
  en una copia NO puede actualizarse a ella si se instaló con otra clave, p. ej. la de depuración.)
  **Keystore propio** (creado el 11/11/2026): `soundify-android.jks` + `soundify-android.properties` (contraseñas; `storeFile`, `alias`, `storePassword`,
  `keyPassword`) fuera de git, en la carpeta de claves (ruta en `CLAUDE.local.md`). No se abren ni se muestran: solo se pasa la ruta del `.properties`
  (`SOUNDIFY_ANDROID_KEYSTORE_PROPS`; `app/build.gradle.kts` lo lee, o un `keystore.properties` junto a él, ignorado por git; sin ninguno el APK de release sale SIN firmar).
  **Perder el keystore = ninguna copia instalada podrá volver a actualizarse** (hay que desinstalar e instalar de cero): tener copia de seguridad.
  Publicar el APK: con `ANDROID_HOME` y `NDK_HOME`, `node scripts/android-release-build.mjs --keystore-props "<ruta>"` (borra los APK
  anteriores, compila release con R8 para aarch64 y x86_64 y comprueba con `apksigner` que está firmado y NO con la clave de depuración) →
  `node scripts/package-apk.mjs --key "<ruta clave de actualización>"` → los tres `-tauri.apk*` de `release/` se suben a la MISMA release que el ZIP (seis archivos).
  El release usa R8: **comprobar en el aparato que arranca, descarga y suena** antes de publicar (reflexión de Tauri, youtubedl-android y Media3).
  Ya rompió una vez (11/11/2026): youtubedl-android desempaqueta Python con `commons-compress`, que crea clases por nombre, y R8 las quitaba
  (`ExceptionInInitializerError … AsiExtraField is not a concrete class`): las herramientas se quedaban en «Preparando…» para siempre y
  Ajustes mostraba la insignia amarilla «3» (= yt-dlp, ffmpeg y motor JS sin desempaquetar). Arreglado con las reglas `-keep` de
  `proguard-rules.pro` (`com.yausername.**`, jackson, `org.apache.commons.compress.**`); en Ajustes → Herramientas debe salir yt-dlp con su
  versión. Si vuelve a pasar algo así en release: añadir un instante `isDebuggable = true` al buildType release, instalar y leer
  `adb shell run-as com.dannygt97.soundify cat files/data/logs/soundify-0.log` (la tablet HONOR no deja ver logcat); quitarlo antes de publicar.
  La descarga real y el sonido con la pantalla apagada **siguen sin comprobarse en la build de release**.
- **Publicar una release: cuando el usuario lo pida, lo hace Claude entero, SIEMPRE QUE NO MIRE LA CLAVE PRIVADA.**
  La clave privada de firma (su ruta está en `CLAUDE.local.md`, que git ignora) no se abre, ni se lee, ni se muestra, ni se copia: solo se pasa su
  ruta al script. Pasos: `node scripts/release-build.mjs` desde el commit de la versión (mejor en un `git worktree` de esa etiqueta; envuelve
  `tauri build --no-bundle` con `--remap-path-prefix` para que el .exe NO lleve la carpeta del usuario de Windows: rustc
  incrusta las rutas de `.cargo`/`.rustup`; comprobar con `grep -a` que no sale el nombre de usuario) →
  `node scripts/package-portable.mjs --key "<ruta>"` (se niega si la clave no es la que acepta la app) →
  **NORMA (pedida el 15/11/2026): cada release lleva TAMBIÉN el instalador `.exe` de Windows** (además del ZIP portable y del APK; PENDIENTE de montar, ver «Pendiente») y los archivos siguen llamándose `-tauri.zip` / `-tauri.apk` (un solo juego; ver «Pendiente»); título y notas sin nombrar el stack.
  `gh release create vX.Y.Z -R DannyGT97/Soundify --target <hash COMPLETO> --title "Soundify X.Y.Z" --notes "..."` con solo
  los tres archivos de `release/` (`-tauri.zip`, `.sha256`, `.sig`) —más los tres `-tauri.apk*` si la release lleva Android (ver arriba)— y notas
  `## Novedades` en español. Comprobar después lo subido

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
  libre vía `fs4`, importar, `find_importable`); sus errores dan clave de i18n + argumentos. Una carpeta es de
  Soundify si su manifiesto es válido (`parse_manifest`: JSON, `schema`, `title` y `format`; uno de una versión más
  nueva cuenta, para avisar de actualizar). En el teléfono, buscar solo mira el nombre y se valida al importar
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
  para no casar con las releases Java 1.x; NO `tauri-plugin-updater`. `Package` = Zip (Windows) o Apk (Android, `Package::current()`):
  una release puede llevar los dos juegos de tres assets y cada plataforma coge el suyo (`-tauri.apk`, `.apk.sha256`, `.apk.sig`); una
  release sin el suyo no cuenta como actualización) y `device/` (trait
  `PortableDevice`, `NoDevices` fuera de Windows, `fake::FakeDevice` para tests) con lo que lo usa:
  `transfer/device` (copiar al/del teléfono, importar desde él), `engine/download/device` (=
  `DeviceSyncService`, un vídeo cada vez, preparado en local y empujado después) y `archive/device`
  («Eliminar todo» en el teléfono)
  `cover_embed` (crate `lofty`): la imagen elegida en «Descarga por URL» se escribe DENTRO del archivo, porque
  una descarga suelta no se registra en ningún sitio. En WAV, MKV, WEBM, AVI y FLV no se guarda; los reintentos
  de segunda pasada no la llevan. `engine/download/create.rs`: `url_thumbnail` (miniatura de una URL: la de
  la playlist con `--playlist-items 0`, si no la del primer vídeo. **Sin probar contra YouTube real**)
- `src-tauri/` — la app: comandos finos que llaman a `core/`, sin lógica propia (`commands.rs`).
  Un error de comando llega al front como `{key, args}` (`ApiError`; `errorText()` lo traduce).
  Plugins: `dialog` (elegir carpeta) y `opener` (abrir carpeta), con sus permisos en `capabilities/`.
  `app_paths.rs`: `data/` junto al ejecutable, o `SOUNDIFY_DATA_DIR`. El arranque corre
  `reconcile_all` en un hilo (lo lanza `lib.rs` y emite `file-downloaded` al acabar, para que el front relea la
  biblioteca), y **escribe** en esa carpeta. `reconcile_entries` enlaza también por el manifiesto de la carpeta:
  `data/` es de cada copia (dev y build tienen la suya sobre la misma carpeta de música), y lo que una bajó sin
  que la otra lo supiera (`NNN - Título.mp3`, sin id) se reconoce por el id del manifiesto. La ventana arranca
  maximizada (`"maximized": true` en `tauri.conf.json`, dev y build). `bootstrap.rs` arranca los updaters
  (solo con herramientas gestionadas), los archives de dispositivo y la caché de formatos; `process.rs` = `YtDlpProcess`, el
  `Downloader` real (UTF-8, Deno en el PATH, stdout/stderr en hilos aparte, parar mata el árbol). Los
  problemas de herramientas salen como evento `tool-health` → toast persistente (`main.ts`).
  **ffmpeg viaja con la app** (como en Java, que lo llevaba en el JAR): recurso de Tauri
  (`src-tauri/resources/ffmpeg/` → `tools/ffmpeg/` junto al exe; los `.exe` NO van en git, los pone
  `node scripts/fetch-ffmpeg.mjs`, o `--from <carpeta bin>`). El arranque lo copia a `data/systemtools/`
  (`updaters::seed_ffmpeg`). **El arranque ya NO baja nada que falte** (Java bajaba ~180 MB sin
  preguntar): solo copia el ffmpeg incluido y, con `autoUpdates` (Ajustes → Actualizaciones, independiente del
  de la app), actualiza yt-dlp, ffmpeg y Deno si YA están y hay versión nueva (`update_heavy_tools`). Instalar es cosa del usuario, por `tools.rs`: `tools_report` (estado y versión de
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
  bloque que se descarga; null en una suelta). Sin yt-dlp o ffmpeg no arranca nada, ni aunque la interfaz falle: `can_download` (`core/src/tools/status.rs`) lo rechaza en
  `Downloads::start`, `create_playlist` y `create_device_playlist` con `tools.error.missing`; en el front, `canDownload` (`tools.ts`).
  Las tres pantallas enseñan el mismo aviso (`components/toolsNotice.ts`).
  «Crear Playlist» es su propio comando (`create_playlist` → `Engine::create_playlist`): REGISTRA antes
  de descargar nada, y «Crear y descargar» lanza después un `update` normal. Si esa playlist ya está registrada en esa carpeta y formato (mismo `data.json`, hash de título+carpeta), el motor NO la reutiliza
  en silencio: devuelve `CreateError::AlreadyExists` → error `newPlaylist.alreadyExists` (args: título y clave del bloque) y el front
  (`views/newPlaylist.ts`) pregunta «Abrir en el Gestor» / «Continuar igualmente»; esto último repite con `reuseExisting` (= el comportamiento
  heredado de Java: suma los vídeos que falten y conserva la numeración). Test e2e en `e2e/newPlaylist.spec.ts`. **Sin ver en la app real**. Una descarga suelta va a la
  carpeta elegida tal cual (sin subcarpeta, sin numerar) y su limpieza solo toca restos de lo que bajó
  `covers.rs`: comandos de portadas y el ámbito del protocolo `asset`, que se abre EN MARCHA
  (`allow_folder`: caché de miniaturas, archives, cada carpeta de música que lista `list_playlists` y
  la imagen recién elegida; en `tauri.conf.json` va vacío). Evento `thumbnail-ready`.
  Cerrar la ventana guarda antes el segundo del reproductor (`onCloseRequested` → permiso
  `core:window:allow-destroy`)
  «Abrir carpeta» (Reproductor y Gestor, `openFolder` de `manager.ts`) llama a `open_folder` (`commands.rs`, `opener().open_path`): abre la carpeta MISMA, no su padre con ella seleccionada (`revealItemInDir`, que ya no se usa ni tiene permiso); si ya no existe, error `manager.openFolder.missing`. **En Android** el plugin opener no abre carpetas: lo hace Kotlin (`SoundifyPlugin.openFolder`, `mobile::open_folder_now`): `ACTION_VIEW` sobre el documento de directorio de DocumentsUI (`primary:<ruta relativa>`) y, si el móvil no tiene app para eso, el selector de carpetas abierto en ella; solo el almacenamiento principal. **Sin comprobar en la tablet**.
  La imagen de «Cambiar portada» / «Descarga por URL» (`pick_cover_image`) tampoco puede usar el plugin `dialog` en Android (devuelve `content://`, no un archivo): `SoundifyPlugin.pickImage` (`ACTION_OPEN_DOCUMENT`, `image/*`) copia la elegida a `cacheDir/picked-cover-<ms>.<ext>` y devuelve esa ruta (`mobile::pick_image_now`). **Sin comprobar en la tablet**.
  `app_update.rs`: comprobar (8 s tras arrancar), descargar e instalar → eventos `app-update` y
  `app-update-progress`; en desarrollo se niega (`update.devBuild`). `devices.rs`: los comandos de
  dispositivo (listar, explorar, crear playlist, copiar en los dos sentidos, importar; las copias
  emiten `copy-progress` y paran con `cancel_copy`). `mtp.rs`: MTP real con el crate `windows`
  (`Shell.Application` para listar, `IFileOperation` para copiar y borrar), cada llamada en su hilo y
  apartamento COM con tope de 15/60/90 s. **Sin probar con un teléfono real**
  `window_menu.rs`: el menú del sistema al pulsar con el botón derecho en la barra de título propia
- `src-tauri/gen/android/` — el proyecto Android que genera `tauri android init`, con dos cosas propias: `MainActivity.kt`
  (aparta el webview de las barras del sistema y del teclado, por eso el CSS no usa `safe-area`) y los iconos de
  `tauri icon`. `tauri.android.conf.json` quita de Android el ffmpeg incluido. `app_paths.rs` en Android: la carpeta
  privada, sacada de `/proc/self` porque la necesita el logger antes de que arranque Tauri.
  Además, plugin Kotlin `SoundifyPlugin` (registrado en `mobile.rs`/`lib.rs`) y: `BusyService` (foreground `dataSync` mientras
  se descarga), `PlaybackService` + `NativePlayer` (Media3: sonido en segundo plano, notificación con anterior/siguiente; la portada de la notificación es la miniatura del archivo, recortada a cuadrado en Kotlin (`NativePlayer.coverBytes`) y pasada en `artwork` por `AndroidPlayer`; sin ella Media3 pone el logo de la app. **Sin comprobar en la tablet**).
  **Android usa rutas reales, no SAF:** permiso «Acceso a todos los archivos» (`MANAGE_EXTERNAL_STORAGE`), música por defecto en
  `Music/Soundify`; `pick_phone_folder` convierte el selector de carpetas a ruta (solo el almacenamiento principal). Sin
  permiso el arranque NO concilia y el front (`storage.ts`) avisa con «Dar acceso»; al concederlo se concilia. yt-dlp, Python,
  QuickJS y ffmpeg vienen en el APK (youtubedl-android 0.18.1, `useLegacyPackaging`) y Kotlin solo los desempaqueta
  (`prepareRuntime`); Rust los lanza con `Launcher` (`ToolPaths`) y hasta entonces cuentan como ausentes (`can_download` false,
  evento `tools-ready`). Ya sin esto `can_download` NO es false en Android. `mobile.rs`: `storage_info`, `request_all_files`,
  `pick_phone_folder`, `set_busy`, `native_player` (lista blanca de acciones) y `listen_player` (reemite como evento `native-player`)
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
  descargados primero). «Ver archivos» sale agrupado de serie, con icono de estado en cada fila y en su filtro (`MARK` en `overlays/files.ts`) y la miniatura de cada archivo (la fila es
  `components/trackRow.ts`, la misma del Reproductor); solo rehace las filas cuando cambia lo que suena, no con
  cada `player` del reloj (varios por segundo)
- `keys.ts` — la barra espaciadora reproduce/pausa en toda la app salvo donde escribe (`spaceTypes`,
  con test). Sin esto el espacio volvía a «pulsar» el último botón con el foco: reabría «Cambiar
  portada» o «Ver archivos». Pulsar un archivo en el Reproductor lo pone y se queda en la lista; pulsar el
  que ya suena lo pausa (no lo reinicia); Reproduciendo se abre desde la barra. Reproduciendo lleva el volumen (`buildVolume`, el mismo de la barra) cuando la barra
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
- **Fragmentos simultáneos (fase 2 de `PLAN-DESCARGA-SIMULTANEA.md`):** `concurrentFragments` en `settings.json` (apagado de serie; Ajustes →
  Avanzado). Encendido, `BuildConfig.concurrent_fragments` = `fragments_for(true)` = 2 (`MAX_CONCURRENT_FRAGMENTS`, tope duro) y
  `command.rs` añade `--concurrent-fragments 2`; solo en la descarga principal (`engine/download.rs`), no en la segunda pasada ni en el
  dispositivo. Solo ayuda con vídeo en fragmentos (DASH/HLS), no con un MP3, y suma peticiones. **Sin probar contra YouTube**
- **Cola de descargas (fase 1 de `PLAN-DESCARGA-SIMULTANEA.md`, concurrencia 1):** una sola descarga a la vez, las demás esperan en
  orden (`BackgroundTasks.enqueue/start_ready/remove_queued/clear_queue/is_busy_with`, `core/src/engine/tasks.rs`; la petición real
  vive en `Downloads.pending`, por id, en `downloads.rs`, cuyo hilo cierra su tarea y pide al planificador lo siguiente BAJO UN MISMO
  cierre de `tasks`: orden de bloqueos `tasks` y luego `pending`/`stops`). Pedir otra mientras corre una la encola y avisa (`tasks.queue.added`); el mismo
  archive en marcha o en cola se rechaza (`tasks.queue.duplicate`). Detener para solo la activa; «Quitar» / «Quitar todas»
  (`remove_queued`, `clear_queue`) no deshacen el registro de una playlist recién creada. La vista lleva `queued` y el front
  `ui.queue` (Tareas: sección «En cola» con posición; anillo/Tareas «+N en cola»; Gestor y «Ver archivos»: insignia «En cola» y «Quitar
  de la cola» en lugar de Sincronizar; los hermanos del bloque activo ya no se atenúan ni se desactivan). Las copias a dispositivo
  MTP pasan por la misma cola. Probado con el mock (`e2e/manager.spec.ts`) y los tests de `tasks.rs`; **sin probar contra
  YouTube ni en la tablet**
- **Tareas con id y reservas (fase 3 de `PLAN-DESCARGA-SIMULTANEA.md`; la concurrencia de serie sigue siendo 1, `MAX_RUNNING`; la fase 5 la sube a 2 si el usuario lo activa):** cada tarea
  tiene un `TaskId` desde que se encola; `BackgroundTasks` guarda una lista de en marcha (`TasksView.running` es un array, el front
  enseña la primera: `ui.task`) y todo lo que cambia una tarea (título, bloque, paso, progreso, parada, aviso, cierre) lleva su id. Lo que
  una tarea necesita para sí son `ResourceKey`s (`core/src/engine/resources.rs`): el `data.json` de su archive, su carpeta de música
  (normalizadas, sin tocar el disco; ante la duda cuentan como iguales) y «el teléfono» (una copia a la vez). `start_ready()` es el
  planificador: arranca en orden lo que cabe y no comparte nada con lo que corre NI con una tarea anterior que sigue esperando (nadie
  adelanta). En `downloads.rs`: un hilo y un `StopFlag` por tarea (`stops`), `stop_download(taskId?)` (sin id, todas), los eventos
  `activity` y `notice` llevan `taskId`, y `set_busy` de Android refleja si queda algo en marcha. `ArchiveStore` tiene un candado de
  escritura compartido (`write_lock`) en cada cambio «leer → modificar → guardar» (los pases largos —conciliación, teléfono— no lo toman: ya
  los cubre la reserva del archive). No se tocó el JSON ni `archive.txt` (sigue siendo de UN proceso a la vez). Probado con `tasks.rs`,
  `resources.rs`, `archive/tests.rs` (cambios concurrentes sin pérdidas) y `java_compat.rs`; **sin probar con más de una descarga real**
- **Dos procesos en una playlist (fase 4 de `PLAN-DESCARGA-SIMULTANEA.md`):** `parallelDownloads` en `settings.json` (apagado de serie; Ajustes →
  Avanzado). Encendido y con ≥2 elementos pendientes, `engine/download/lanes.rs` reparte la tanda en dos (numerada: mitades contiguas, la segunda
  con `--autonumber-start` = desfase + los de la primera; sin numerar: grupos por `cleaned_title`) y lanza dos `Downloader.run` (hilos con
  `thread::scope`, líneas por un canal al hilo llamante, que es el único que escribe archive y UI). El segundo trabaja sobre
  `archive.lane2.txt` y `cookies.lane2.txt` (copias en la carpeta del archive, borradas al acabar). Detener / bot-check paran los dos; un
  429 para el segundo y sus elementos van a la segunda pasada (`activity.workers.backoff`). Una descarga suelta, una playlist con un solo
  pendiente o el reintento no se reparten. Máximo 2 procesos (`MAX_WORKERS`). Tests: `lanes.rs`, `engine/download/tests.rs`. **Sin probar contra
  YouTube ni en la tablet** (la prueba real de 8 vídeos tardó 97 s seguidos y 51 s con dos; no vale para las +700)
- **Un solo interruptor para lo rápido (10/11/2026):** Ajustes → Avanzado → «Descargar más rápido (experimental)» (`settings.advanced.faster`, apagado de serie) sustituye a los tres de las fases 2, 4 y 5 (`views/settings.ts`, `fasterSwitch`): al cambiarlo guarda a la vez `concurrentFragments`, `parallelDownloads` y `parallelBlocks`, y se ve encendido si CUALQUIERA lo está (un `settings.json` viejo sigue mostrando lo que de verdad hace). El texto es sin jerga a propósito. Los tres campos siguen existiendo en el core con su significado de abajo.
- **Dos bloques a la vez (fase 5 de `PLAN-DESCARGA-SIMULTANEA.md`, EXPERIMENTAL):** `parallelBlocks` en `settings.json` (apagado de serie; Ajustes →
  Avanzado, «Descargar más rápido (experimental)»; la de la fase 4 también lleva «(experimental)»). Apagado, el comportamiento es el de siempre: UNA descarga y
  las demás en cola. Encendido, `BackgroundTasks.set_limit` sube el tope de tareas en marcha de `MAX_RUNNING` (1) a `MAX_RUNNING_PARALLEL` (2);
  el teléfono (MTP) sigue siendo de uno en uno y el mismo archive o la misma carpeta nunca corren dos veces (reservas de la fase 3). **Presupuesto
  global de procesos de yt-dlp = `MAX_PROCESSES` (2)**: cada tarea tiene uno y puede PEDIR prestado el segundo para los carriles de la fase 4
  (`claim_extra_process` / `release_extra_process`; se niega si el total pasaría de 2 o si una tarea en espera cabe en el hueco), así que las
  fases 4 y 5 no se multiplican. El motor lo pide por `DownloadEvents::extra_process()` / `release_extra_process()` (de serie, sí) alrededor de
  `lanes::run`. Puente (`downloads.rs`): `run_limit` lee el ajuste, `replan` (también tras `save_preferences` y al soltar el segundo proceso)
  arranca lo que ahora cabe; `begin_run` solo vacía la actividad si no corre otra tarea; el directorio temporal de cada ejecución es único.
  Front: `ui.running` (todas las tareas en marcha; `ui.task` = la primera), `runningFor(blockKey)` y `overallPercent` (`tasks.ts`); lo vivo
  lleva `data-task` y `patchLive` actualiza cada pieza con SU tarea; el Gestor ilumina los dos bloques, Tareas lleva una tarjeta por tarea
  con «Detener todas», y el anillo dice «N descargas · %». El mock lo imita con un temporizador por tarea. Límites conocidos: `last_finished`
  solo deja corregir con un aviso tardío a la ÚLTIMA tarea cerrada; el registro de Actividad mezcla las líneas de las dos; `sync_busy` enseña
  solo el título de la primera. Tests: `engine/tasks.rs`, `engine/download/tests.rs`, `settings.rs`, `tasks.test.ts` y `e2e/manager.spec.ts` /
  `settings.spec.ts`. **Sin probar contra YouTube ni en la tablet** (duplica la carga: riesgo de bot-check en listas largas)
- Gestor con una descarga en marcha (`views/manager.ts`): el bloque que descarga lleva contorno con un
  brillo que lo recorre, insignia «Descargando», «Archivo X de Y» y Detener en lugar de Sincronizar; sus
  hermanos se atenúan y no se pueden sincronizar; la fila padre lleva un chip en vivo y, plegada, una barra
  fina; se despliega sola al empezar y al acabar bien el bloque destella en verde ~3 s. Solo se rehace
  cuando cambia el bloque o el paso (`liveShape`); el resto es `patchLive` sobre `[data-live]`, y las
  animaciones conservan su fase entre rehechos (`--live-phase`). «Ir a la descarga» abre el Gestor con
  `open` + `focus` y lleva el bloque a la vista
- `urls.ts` — `isWebUrl` / `isPlaylistUrl` (cuenta también un vídeo abierto desde una lista, `watch?v=…&list=…`: Descarga por URL
  lo rechaza y lleva a Crear Playlist; `Downloads::start` lo rechaza igualmente con `download.error.isPlaylist`)
- `transfer.ts` — los textos y el tono con que acaban «Copiar playlist» e «Importar» (lógica pura);
  las ventanas de esos dos flujos están en `overlays/transfer.ts` (carpeta o teléfono en los dos
  sentidos; `progressWindow` = barra + Cancelar, cerrar la ventana también cancela)
- `*.test.ts` — Vitest, solo lógica pura del front; `i18n.test.ts` comprueba que toda clave usada existe
- `main.ts` — el armazón: menú + página + barra del reproductor + capa de ventanas superpuestas
- `platform.ts` — `onAndroid` (el aparato: lo que el back aún no puede, `?mockAndroid` lo imita) e `isPhone()` (el
  sitio en pantalla, misma media query que `styles/phone.css`). **Móvil = el mismo DOM recolocado**: `phone.css`
  convierte el menú lateral en barra de pestañas (`display: contents`, 5 pestañas; Crear Playlist comparte la de
  Descargar, con `formSwitch` arriba), reduce la barra del reproductor a portada, título, play, siguiente y cerrar con
  la línea de progreso arriba, (sin píldora de tareas: su sitio es el anillo del título). Reproduciendo lleva allí los controles (sin volumen:
  son los botones del teléfono). En Android además se oculta el destino MTP (`.desktop-only`) y la pestaña Actualizaciones
  (Herramientas solo enseña yt-dlp, para actualizarlo); Importar sí está (por carpeta). `api/androidPlayer.ts`: `AndroidPlayer` envuelve a `HtmlPlayer` (audio → servicio
  nativo, vídeo → webview). El volumen es el del aparato (`DeviceVolume.kt`, `STREAM_MUSIC`): el deslizador lo mueve
  (`playerSetVolume`) y los botones del aparato mueven el deslizador (evento `volume`, `PlayerBackend.onVolume`; en el arranque
  no se aplica el volumen guardado: `externalVolume`). `onVolume` NO se fía solo de los eventos: un 0 se confirma preguntando 300 ms
  después y el deslizador se resincroniza al volver a la app (`visibilitychange`/`focus`) y cada 3 s mientras se ve (el síntoma:
  deslizador «en silencio» con el sonido bien; causa sin demostrar, **sin comprobar en la tablet**; `DeviceVolume.kt` deja en
  logcat `SoundifyVolume` cada cambio que ve) **Teclado en Android** (`platform.ts`, `watchKeyboard`): con un campo de texto enfocado y la página `>120 px` más baja que su máximo, la raíz lleva `data-typing` y `phone.css` oculta menú, barra del reproductor, píldora de tareas y el pie del formulario (resumen + Descargar), y el campo se centra; al cerrar el teclado vuelve todo. En una tablet apaisada con teclado el campo salía cortado. **Sin probar en la tablet** `storage.ts`: estado del acceso a archivos (`?mockNoStorage` lo niega)
- `router.ts` — rutas por hash (`#/manager`, `#/player?b=pl1:MP3`); cada visita rehace la pantalla,
  lo que tiene que sobrevivir vive en `state.ts`. `onLeave()` para timers y suscripciones
- `state.ts` — estado de la UI (menú plegado, tema, ubicación del reproductor, lo que suena, la tarea); no guarda nada: lo que dura entre arranques va por `preferences.ts`. `subscribe`/`emit` por tema (`library`, `player`, `tasks`, `theme`, `player-placement`)
- `i18n.ts` — TODO texto visible sale de aquí con `t(clave)`; las claves son las de la app Java
  cuando existen. Español aquí; inglés y catalán en `locales/` (mismas claves, comprobado
  por test). `setLanguage` avisa a `onLanguageChange` y `main.ts` rehace menú, barra y pantalla
- `dom.ts` — `h()` para construir nodos (nunca HTML en cadenas: los títulos vienen de YouTube)
- `icons.ts` — TODOS los iconos, de Lucide (`lucide`, ISC): SVG con `currentColor`, nítidos a cualquier
  escala. Se importan por nombre (solo los usados entran en el build). Icono nuevo = una línea en `ICONS`.
  La S ancha de Inicio se dibuja como SVG inline con el acento; el icono de la app es esa misma S (verde `#1ed760` sobre `#3a3a3a`, el tema Oscuro): se generó con `tauri icon` desde un PNG de 1024 y en Android el primer plano adaptativo es solo la S sobre el color `ic_launcher_background`; el PNG de Ajustes → Acerca de y el favicon están en `public/icons/`.
- `scale.ts` — la escala de la interfaz: en la app es el zoom NATIVO del webview (`setZoom`, permiso
  `core:webview:allow-set-webview-zoom`), que recoloca todo; un `zoom` de CSS en la raíz hacía crecer
  la pantalla por debajo de la ventana. En el navegador, `zoom` sobre `#app` (alto en %, no `vh`).
  Ctrl + / Ctrl − / Ctrl 0 van por los mismos pasos que la lista de Ajustes; se guarda en `settings.json`.
  **En Android también hay escala** (cada móvil o tableta tiene su pantalla): su webview no tiene zoom nativo, así que ahí
  va por la rama del navegador (`zoom` de CSS sobre `#app`, `onAndroid`). Las media queries de `phone.css` miran la ventana
  real, no la ampliada; lo que se adapta al ancho ampliado son las container queries. **Sin probar en la tablet**
- `preferences.ts` — las preferencias de `settings.json`: `loadPreferences()` antes de la primera
  pantalla (tema, idioma, escala y sitio del reproductor aplicados ya), `save(patch)`, los `choose*`
  y `resetPreferences()` (vuelve a fábrica y lo aplica en caliente). localStorage ya no se usa
- `settingsSearch.ts` — el buscador de Ajustes (= `SettingsSearchIndex`): catálogo desde las claves
  `settings.<categoría>.<algo>` con `.desc`, en el idioma en uso
- `views/` — Inicio, Descarga por URL, Crear Playlist, Gestor, Reproductor, Reproduciendo, Ajustes
- `components/` — menú lateral (UNO para toda la app), barra del reproductor, portada, ventana
  superpuesta y piezas de formulario. `form.ts` lleva lo común de Descarga por URL y Crear Playlist:
  `formatPicker` (botón + panel flotante), `urlCover` (miniatura manual, o automática al escribir una URL
  válida; `keepsPickedCover` = en qué formatos se puede guardar), `destinations` (carpeta o teléfono) y
  `footerSummary`. `floating.ts` = menú flotante colgado de la capa de ventanas (el «⋯»), `live.ts` = la
  descarga en curso como chip y barra
- `overlays/` — Tareas, Ver archivos (filtro, estado y reproducción por archivo o playlist), Ayuda de
  Importar (tamaño fijo: desplegar «¿Qué carpeta elijo?» no cambia su tamaño), Actividad, confirmación,
  `deletePlaylist.ts` (Eliminar, con sus opciones), el **explorador de archivos propio** (`explorer.ts`, 17/11/2026, fases 1 a 3; sustituye a `devicePicker.ts` y al diálogo del sistema):
  una ventana para carpetas del PC y teléfonos MTP (raíl con accesos rápidos, discos con espacio libre y teléfonos que se refrescan cada 4 s; migas, filtro, orden, lista o cuadrícula, nueva carpeta,
  teclado ↑↓ Intro Retroceso Ctrl+F; estados lento / desconectado / bloqueada / vacía). `openExplorer(purpose, onPicked, options)`, con `purpose` = `destination` (PC o teléfono), `copy-folder`,
  `copy-device`, `import-folder`, `import-device`, `cover` (solo imágenes) y `cookies` (solo .txt); en modos de archivo hay panel de detalles y «Mostrar todos». En los formularios de descarga hay UNA tarjeta «Carpeta de destino» (`form.ts`, `destinations`) que abre el explorador para carpetas del PC o de un teléfono MTP; en modo archivo de imagen el panel de detalles enseña la imagen (`showPreview`, vía `allow_picked_file` + `fileUrl`). `pickers.ts` lo envuelve
  (`pickLocalFolder`, `pickImportFolders`, `pickCoverFile`, `pickCookiesFile`). Lógica pura y probada en `explorerLogic.ts`; backend en `core/src/browse.rs`
  (`list`/`make_folder` por `Storage`, `drives`, `places`) y `src-tauri/src/browse.rs` (`browse_roots`, `browse_list`, `browse_mkdir`, `allow_picked_file`); mock en `api/mock/browse.ts`. «Explorador del sistema…» queda
  como plan B. **Android (fase 2):** el mismo explorador sobre rutas reales (`MANAGE_EXTERNAL_STORAGE`); `drives()` da `internal` y las tarjetas `sd:<id>` (`/storage/*`, `readOnly`: solo importar, no destino) y los sitios de `/storage/emulated/0`; sin permiso muestra «Dar acceso» (`grantStorage`); sin sección de teléfonos MTP. **Favoritas y recientes (fase 3):** estrella en la barra, secciones del raíl; van a `settings.json` (`explorerFavorites`/`explorerRecents`, comandos `explorer_memory`/`save_explorer_memory`; `Reset` no las borra); un destino nuevo empieza en la última carpeta elegida. **«Buscar en subcarpetas»** (importar de una carpeta, `browse_find_playlists`, hasta 4 niveles): lista con casillas e «Importar (N)»; devuelve `{paths}` y `importFromFolder` mira cada una. **Sin probar con un teléfono MTP real ni en Windows ni Android reales** (solo mock y tests). Falta: favoritas de carpetas de un teléfono MTP, miniaturas de imagen, y el selector antiguo de Kotlin (`pick_phone_folder`, `pickImage`) queda solo como plan B. Prototipo: `design/file-explorer-prototype.html`
- `styles/theme.css` — nueve bloques `[data-theme="…"]` con los tokens `--sp-*`: Oscuro (oficial, el
  predeterminado) y Claro (oficial), sus variantes neutras (`dark-neutral`, `light-neutral`: sin acento verde
  y sin el tono propio de cada playlist, `--h`; eso se anula al final de `screens.css`), más Oscuro rojo,
  Oscuro azul, AMOLED, Claro rojo y Claro azul. Incluye
  `--sp-success`, `--sp-cover-a` y `--sp-cover-b`; el éxito no usa el acento y el vídeo conserva su
  degradado morado. `--sp-live`/`--sp-on-live` = algo en marcha (la descarga del Gestor): el acento, salvo en
  los temas rojos, donde un contorno rojo parecía un error; `--sp-glint` = el brillo que recorre ese
  contorno, más claro que él en los temas oscuros y más oscuro en los claros (pálido parecía un hueco). `styles/app.css` usa variables para los colores y contiene los selectores de tema y avisos;
  `styles/screens.css` = las pantallas rediseñadas (Inicio, Reproductor, Gestor, formularios, explorador).
- `components/titleBar.ts` — la barra de título propia (`decorations: false`), en los colores del tema. `api.windowControls` es
  null donde no hay ventana que dibujar (navegador y Android; `?mockWindow` la enseña para los tests). Arrastrar, doble clic y ajustar a
  los bordes vienen de `data-tauri-drag-region`; redimensionar, de Tauri (lo hace solo en una ventana sin marco). El rectángulo de la
  ventana incluye 11 px invisibles de sombra a cada lado. Sin el menú de Snap Layouts al pasar por el botón de maximizar.
- `components/pip.ts` — imagen en imagen: un panel dentro de la app (no una ventana) con el MISMO `<video>` de `htmlPlayer.ts`, para
  vídeo. Cerrarlo pausa en el mismo segundo; `pipPanel.ts` = dónde puede estar (lógica pura: `pipSize` 320×180, o 208×117 en
  móvil, `startSpot`, `clampPanel`, `overDock`). **Sale solo al dejar Reproduciendo con un vídeo** (`ui.autoPip`, ajuste
  `autoPip` de `settings.json`, por defecto sí; se quita con el chip «Imagen en imagen automática» de Reproduciendo o en Ajustes →
  Interfaz) y vuelve solo al entrar (`ui.pipAuto` distingue el panel abierto solo del abierto a mano, que se queda). En el hueco
  de Reproduciendo (`.np-pip-here`) se puede arrastrar el panel para devolverlo (`.dock-over`) o usar «Traerlo aquí». También
  funciona en móvil y tableta (solo el botón del reproductor de la barra sigue oculto en móvil)
- **Portada editable en el Reproductor:** la portada grande del detalle (`.hero-cover` en `views/player.ts`) lleva un lápiz al pasar el ratón (o foco), como las filas del Gestor, y abre el mismo `changeCover`; en pantallas táctiles (`hover: none`) el lápiz es una insignia siempre visible. **Sin ver en la tablet**
- **Buscadores con «x» para borrar:** todos son `type="search"` (Gestor, Reproductor y su búsqueda de archivos, Ver archivos, Ajustes, filtro del explorador) y la «x» es el botón nativo (`::-webkit-search-cancel-button`, dibujado con máscara en `app.css`; más grande en Android, `phone.css`). Un buscador nuevo = `attrs: { type: 'search' }`; `keys.ts` ya lo trata como campo de texto. **Sin ver en la tablet**
- `components/themePicker.ts` — rejilla reutilizada por el botón del menú y Ajustes → Interfaz.
- **Avisos y actividad (16/11/2026, sustituye a la píldora de tareas):** `components/toasts.ts` (`notify` / `liveNotice`): los avisos viven en `.page-col`
  (columna de la página, ARRIBA de `#page`) y EMPUJAN la pantalla hacia abajo, nunca la tapan (ni cabecera ni reproductor); se quitan solos (`TOAST_MS`: 5 s, aviso 6,5 s, error 8 s),
  se arrastran de lado, máximo 3 a la vez, y `persistent` = hasta atenderlos (bot-check, herramientas). Todo `notify` queda en `ui.notices` (`notices.ts`, solo memoria,
  últimos 50, tema `notices`); `liveNotice` (progreso de herramientas) no. `components/activityButtons.ts`: anillo con el % de lo que descarga (solo mientras corre y ~4,5 s con el
  resultado, `watchActivity`/`FLASH_MS`) + campana con punto de no vistos, AL FINAL DE CADA `titleRow` (`dom.ts` `setTitleTools`; también en la cabecera de Reproduciendo; NO en los detalles de
  playlist de Reproductor/remota); ambos abren `openTasksOverlay` (ahora «Tareas y avisos»: en curso, cola, Avisos con «Vaciar avisos», terminadas; abrirlo marca los avisos como vistos).
  `components/downloadStrip.ts`: línea «Descargando · N %» + «Ver en el Gestor» bajo el título de Descarga por URL, SOLO para la descarga pedida desde esa pantalla (`origin === 'url'`); una sincronización del Gestor no la enseña ahí, ni en Crear Playlist (que no lleva franja). El Gestor conserva su bloque en vivo; Inicio, su sección «En curso».
  Prototipo: `design/notifications-prototype.html`. Tests: `e2e/notices.spec.ts`, `e2e/player.spec.ts`, `e2e/manager.spec.ts`. **Sin ver en la tablet.**
- `appInfo.ts` — la versión real de la app (`loadAppInfo()` al arrancar, `appVersion()`), «desconocida»
  si no es válida.

## Norma de trabajo (leer primero)
- **Al empezar: leer `context.md`** (resumen del proyecto, comandos, trampas y estado; local, ignorado por git) y NO releer este archivo entero ni explorar a ciegas: buscar con `grep` la
  sección que haga falta (`grep -n "palabra" CLAUDE.md HISTORIAL.md`). El detalle histórico de decisiones y fallos está en `HISTORIAL.md` (local, ignorado).
- **Al terminar algo: actualizar SIEMPRE `context.md`** (estado, trampas nuevas) **y `CLAUDE.md` si cambia la estructura, un comando, una regla o lo pendiente.** Lo que solo es historia de un arreglo va a `HISTORIAL.md`, no aquí.
- Mantener `CLAUDE.md` corto: es lo que se carga en cada sesión. Una nota nueva de más de 3–4 líneas va a `HISTORIAL.md` con un puntero de una línea.

## Pendiente
- **Batería en Android (15/11/2026; tests en verde, SIN comprobar en la tablet):** (a) EINTR: `core/src/connect/resilient.rs` reintenta lo interrumpido bajo los WebSockets y en HTTP (era lo que hacía rebotar Connect en la tablet); (b) `SoundifyPlugin.kt` `applyLocks`: `MulticastLock` y `WifiLock` solo con Connect activo Y (app delante O sonando); (c) cola de Connect: cada controlador tiene un buzón con solo el ÚLTIMO estado y la última playlist (`Mailbox` en `server.rs`), ya no se le suelta por lento.
- **Nombres de los archivos de una release (decisión FINAL del usuario, 15/11/2026): se quedan como siempre, `Soundify-X.Y.Z-tauri.zip` y `Soundify-X.Y.Z-tauri.apk` (+ `.sha256`, `.sig`), UN solo juego por plataforma, sin copias con otros nombres.** Motivo: las apps instaladas
  solo buscan `-tauri` y renombrar rompe su «Buscar actualización» (se probó el 15/11: una app 1.2.0 sí actualiza con el puente, pero no se quieren dos juegos). La v1.2.2 SÍ lleva los dos juegos (`-tauri.*` y `-windows.zip`/`-android.apk`) por ese cambio de idea; de la 1.2.3 en adelante solo `-tauri`.
  El actualizador sigue entendiendo ambos nombres (`Package::names()`, `-tauri` primero). En el título y en las notas de la release no se nombra el stack. El instalador de Windows se llama `Soundify-X.Y.Z-setup.exe` (+ `.sha256`).
- **Instalador `.exe` en cada release (norma del usuario, 15/11/2026; HECHO, probado instalando y desinstalando en una carpeta de prueba):** `scripts/release-build.mjs` compila con `--bundles nsis` (el `.exe` suelto del ZIP y el
  instalador salen de la MISMA compilación, con las rutas remapeadas) y `node scripts/package-installer.mjs` deja en `release/` `Soundify-X.Y.Z-setup.exe` + `.sha256` (NSIS por usuario en `%LOCALAPPDATA%\Programs`, que se puede escribir:
  `data/` va junto al `.exe`; lleva `tools/ffmpeg`; `bundle.windows.nsis` en `tauri.conf.json`, es/en). NO lo usa el actualizador (solo toma `-windows.zip`/`-android.apk` + `.sha256` + `.sig`): se sube como un asset más.

- **Rendimiento (16/11/2026, sin commit):** `PlaybackService.kt`: solo wake lock (la planificación dinámica de ExoPlayer de la 1.2.3 CORTABA el audio con auriculares Bluetooth: no reactivarla; offload quitado); Connect ya no se conecta a sí mismo (`discovery::instance_id`, id en el TXT); comandos con disco con `#[tauri::command(async)]` (los `save_*` de `settings.json` siguen síncronos a propósito). Detalle en `context.md`.
- **Sin comprobar en aparatos reales (lo más valioso):** descarga real contra YouTube en la build de release de Android, sonido con la pantalla apagada, notificación multimedia, importar una
  carpeta, vídeo por `asset://`, abrir carpeta y elegir portada/cookies en Android; Connect con tres aparatos en 1.2.1 (actualizar la tablet y el S24+ y repetir `node scripts/diagnostics.mjs`:
  no debe salir `flooding` ni el corte de 15 s de la tablet). La tablet suele llevar la copia `.debug`: desinstalarla e instalar la release.
- **Móvil:** a escala 150–175 % la barra de pestañas recorta «Descargar»/«Reproductor» (decidir: limitar la escala a 150 % o acortar etiquetas); sin nada sonando el icono de dispositivos no
  está en el menú; probar la regla táctil de `PHONE_QUERY` (la altura solo cuenta con `pointer: coarse`).
- **Logs:** falta el punto (6): formato común `área | evento | clave=valor` y contadores fuera de Connect (reproductor, descargas, herramientas).
- **Fecha «Descargado» al importar (16/11/2026):** `transfer::to_archive_data` fecha cada entrada con la fecha de modificación del archivo (`Storage::modified`, solo `LocalStorage`; `MemoryStorage` y el teléfono MTP no la dan y usan el día de la importación). Antes todo salía «hoy» tras importar. Una playlist ya importada así no se corrige sola: hay que quitarla del Gestor (sin borrar archivos) e importarla de nuevo. Si la copia no conserva la fecha (Android/Linux), sale el día de la copia.
- **Connect, controladores (16/11/2026):** cada controlador de «Pueden controlar este dispositivo» lleva `last_seen` (`Trusted`, segundos; se apunta al emparejar, al conectar, cada 15 s con la conexión abierta y al cerrarla; a disco como mucho 1 vez por minuto, `Pairing::touch`) y la lista lo enseña («Conectado ahora» / «Visto hace 3 días»). `Pairing::prune` quita los que llevan más de 30 días sin conectar (`FORGET_AFTER_SECS`; se llama desde `Connect::controllers()`; los de ficheros viejos sin fecha empiezan a contar en la primera limpieza). Es por los duplicados que deja un aparato con id nuevo (reinstalado, datos borrados, copia `.debug` + release). **Sin comprobar con aparatos reales.**
- **1.2.7 publicada (23/11, incluye las mejoras de bajo riesgo del 21/11: guías de primera vez, tokens sellados DPAPI/Keystore, teclas multimedia, botones de la barra de tareas, receptor que nota la caída del origen y CI; ver `context.md`). Su APK de release NO se probó en un aparato: instalarlo y comprobar que arranca, yt-dlp con versión y los tokens sellados en Android.**
- **1.2.6 publicada (18/11): incluye lo siguiente. Recibir (hecho, probado PC↔S24+ debug): vista del receptor `castQueue.ts`, «Escuchar aquí» (`PassMe`), aviso al origen al dejar de escuchar y avisos con «No volver a mostrar» (`notice.ts`, Ajustes → Avisos y guías); detalle en `context.md`.**
- **«Escuchar en» (pasar lo que suena a otro aparato por streaming, separado de Controlar):** plan y requisito de rendimiento en `PLAN-CONNECT.md`, prototipo `design/connect-cast-prototype.html`. Hechas la fase 1 (core: `connect/cast.rs`, rutas `/media` y `/cover` con ticket y Range, `Command::Cast`/`CastEnd`) y la 2 (destino: relé por trozos de 1 MiB, protocolo `soundify-cast` en `src-tauri/src/cast.rs`, `player.playCast`, cola `CAST_KEY` que no toca `PlayerMemory`); y la 3 (origen: `src/cast.ts`, `connect_cast`/`connect_cast_end`, botón «Pasar aquí» en el selector); y la 4 (selector con «Escuchar en» en verde y «Controlar» en azul, `--sp-control`; aparatos de otra versión marcados) y la 5 (Android: lo recibido suena por Media3 con la pantalla apagada a través de `cast::LocalRelay` en 127.0.0.1 con secreto; `network_security_config.xml` solo deja texto en claro a 127.0.0.1 en release; el que pasa mantiene el Wi-Fi despierto con `castingLock`). Probado en release (S24+, 1.2.5): arranca, suena con la pantalla apagada y recibe de un PC; sin probar pasando DESDE un Android.
- **Seguridad de Connect (protocolo 2):** todo menos `GET /info` va por TLS 1.3 (`core/src/connect/tls.rs`, certificado propio por aparato y huella FIJADA al emparejar); el PIN pasa por SPAKE2 y la confirmación incluye la huella (`pairing.rs`); PIN de serie (el modo abierto lleva aviso). Las 1.2.x ya no conectan con esta versión. Detalle en `context.md`.
- **Seguridad, mejoras futuras (18/11/2026; nada bloquea):** claves opacas en la biblioteca remota (`LibraryEntry.key` son hoy rutas internas de los archives, visibles para los emparejados); `csp` en `tauri.conf.json` (hoy `null`); `trusted.json`/`devices.json` guardan tokens sin cifrar en la carpeta de datos; el límite de intentos de emparejado es global (alguien en la red puede bloquearlo 60 s o llenar `MAX_EXCHANGES`): pasarlo a por origen; `npm audit` avisa solo de vitest/esbuild (desarrollo, no van en la app).
- **Connect:** faltan copiar una playlist de otro dispositivo (`PLAN-CONNECT.md`) y DLNA/Chromecast. (Hecho el 15/11: el controlador manda su nombre actual en la cabecera `X-Soundify-Name` y el destino actualiza el guardado — `Pairing::rename`, `protocol::encode_name/decode_name`; y cada controlador tiene un buzón con solo lo último, `Mailbox`.)
- **Descargas:** sin probar contra YouTube el interruptor «Descargar más rápido (experimental)» ni con cookies / en el móvil; YouTube corta con bot-check a los ~100–300 vídeos sin cookies.
- **Herramientas:** «Cancelar» una descarga de herramienta a medias y la comprobación de yt-dlp «una vez al día» no se hicieron.
- **Migración de la lógica:** fases 1–10 en `MIGRATION-PLAN.md` (local).

## graphify

This project has a knowledge graph at graphify-out/ with god nodes, community structure, and cross-file relationships.

Rules:
- When graphify-out/graph.json exists, run `graphify query "<question>"` before your first grep, search or source-file read for a codebase question, and put this rule in every subagent prompt that explores code. Read raw files only for lines the graph or an earlier read already located. Use `graphify path "<A>" "<B>"` for relationships and `graphify explain "<concept>"` for focused concepts. When the answer is under a Markdown heading, `graphify section "<heading>"` prints that section's text, so the .md need not be opened. These return a scoped subgraph, usually much smaller than GRAPH_REPORT.md or raw grep output.
- If graphify-out/wiki/index.md exists, use it for broad navigation instead of raw source browsing.
- Read graphify-out/GRAPH_REPORT.md only for broad architecture review or when query/path/explain do not surface enough context.
- After modifying code, run `graphify update .` to keep the graph current (AST-only, no API cost).
- A committed graphify-out/graph.json is rewritten whole on every build, so a large diff to it in a pull request is expected. Do not flag it as churn or suggest skipping the graph rebuild or its hooks.
