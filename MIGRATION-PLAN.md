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

### Fase 4 · Herramientas externas
`ToolManager`, `YtDlpUpdater`, `FfmpegUpdater`, `DenoUpdater`, `HttpDownloader` (`reqwest`),
`ZipExtractor` (`zip`), `ToolHealth`, `Bootstrap` (orden de arranque y `reconcileAll`).
- Decisión: yt-dlp **se descarga y actualiza solo en `data/systemtools/`** como en Java (cambia cada
  pocas semanas, no puede ir congelado dentro del ejecutable). Solo en escritorio.
- Tests: `YtDlpUpdaterTest`, `ZipExtractorTest` + uno `#[ignore]` que baja de verdad.

### Fase 5 · Motor de descarga
`DownloadThreadService` + `DownloadCoordinator` + `SafetyNetService` detrás de `Downloader`;
`BackgroundTasks`, `ActivityLog`, avisos (`NoticeCenter` → `notify()` de `toasts.ts`). Parar una
descarga mata el árbol de procesos entero (en Windows, yt-dlp lanza ffmpeg).
- Tests: `SafetyNetServiceTest`, `YtDlpQueryServiceTest`, `BackgroundTasksTest`, `ActivityLogTest`, y
  el ciclo de una tarea con un `Downloader` falso que emite salida grabada de yt-dlp real.
- Al acabar: Descarga por URL, Crear Playlist, Sincronizar/Actualizar del Gestor y el indicador de
  tareas son reales. `tasks.ts` deja de simular.

### Fase 6 · Portadas
`CoverArt`: portada elegida, miniatura del primer vídeo, caché en `data/cache/thumbnails/`, recorte de
franjas negras (crate `image`). Las imágenes al front por `convertFileSrc`.
- Tests: `CoverArtTest`.

### Fase 7 · Reproductor
`PlayerService` + `PlayerQueue` + recordar la última pista y el segundo. Escritorio con el `<audio>`/
`<video>` del webview.
- **Mejora gratis**: WebView2 abre OPUS, FLAC, WEBM y AAC, que JavaFX no abría. `PlayableFormats` se
  rehace probando cada formato de verdad, y se decide si se quitan los atenuados.
- `PlayerQueue` en TypeScript (Vitest). Cuando llegue Android se pasa la lista al servicio nativo.
- Tests: `PlayerQueueTest`, `PlayerControllerTest`, `PlayerBarControllerTest`, Playwright de la barra.

### Fase 8 · Ajustes y armazón
`SettingsSearchIndex`, idiomas (es/en/ca: traer los `messages_*.properties` a `i18n.ts`), escala y
tema guardados en `settings.json` (deja de usarse localStorage), logs con `tauri-plugin-log`
(mismos ficheros y rotación), cookies, carpeta de descargas, «Restablecer» aplicado en caliente.
- Tests: `SettingsSearchIndexTest`, `I18nKeysTest` (toda clave usada existe en los tres idiomas),
  Playwright de Ajustes.

### Fase 9 · Solo escritorio
- **Actualización de la app**: `tauri-plugin-updater` sustituye a `UpdateDownloader`,
  `UpdateInstaller`, `UpdateVerifier` y `AppLauncher` (ya firma y verifica). Se mantiene el origen
  fijado a GitLab (proyecto 85813290) y `ReleaseNotes`.
  **Riesgo**: el problema de Smart App Control que resolvía `AppLauncher` vuelve con un `.exe` de Tauri
  sin firmar. Hay que decidir firma de código antes de publicar.
- **Dispositivos MTP** (`DeviceSyncService`, `DevicePlaylistTransfer`, `WindowsPortableDevice`: ~2350
  líneas): Rust con el crate `windows` (WPD / `IFileOperation`). Es la fase más grande y la última de
  escritorio; detrás del trait `PortableDevice` con un no-op fuera de Windows, igual que en Java.
- Tests: `DevicePlaylistTransferTest`, `UpdateVerifierTest`, `AppUpdaterTest`, `ReleaseNotesTest`.

### Fase 10 · Android
- Plugin Kotlin con `youtubedl-android` que implementa `Downloader` (actualiza yt-dlp con su propio
  `updateYoutubeDL`). Las descargas, en un *foreground service* con notificación.
- `Storage` sobre SAF: el usuario elige la carpeta de música una vez y se guarda el permiso.
- `PlayerBackend` nativo con Media3 (sonido en segundo plano, controles en la pantalla de bloqueo).
- Fuera en Android: MTP (el teléfono ES el dispositivo), autoactualización (va por tienda o APK) y deno.
- Diseño de pantalla móvil (ya anotado en Pendiente).

---

## 4. Lo que hay que decidir (no bloquea la fase 0–1)
1. Dónde vive `data/` en escritorio: junto al ejecutable (portable, como hoy) o en `%APPDATA%`.
2. Firma de código del `.exe` (ver fase 9).
3. Formatos reproducibles: ¿se reproducen todos los que abra el webview?
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
