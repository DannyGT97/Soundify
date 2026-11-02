# Mapa de tests (Java → Tauri)

Cada test de la app Java (`D:\Proyectos\soundifylocal\src\test\java`) y dónde quedó aquí. Un test se
apunta al portar su clase, en la misma fase. Los «regresión:» se portan 1:1 y con su nombre.

Estados: **portado** · **pendiente (fase N)** · **descartado** (con el motivo).

## Fase 0
| Java | Tauri | Estado |
|---|---|---|
| `ui/FxmlLoadUiTest` | `e2e/navigation.spec.ts` (cada pantalla carga con su título) | portado |
| `ui/NavigationUiTest` | `e2e/navigation.spec.ts` (menú, marca, plegado entre pantallas) | portado |
| `i18n/I18nKeysTest` | `src/i18n.test.ts` (solo español; en/ca en la fase 8) | portado en parte |
| — | `src/urls.test.ts`, `src/scale.test.ts`, `src/library.test.ts`, `src/player.test.ts` | nuevos |
| — | `core`: contrato de `Storage` (disco, memoria con rutas y con URIs), `Location`, `ScriptedDownloader` | nuevos |
| `ui/PlayerUiTest` | `e2e/player.spec.ts` (barra, reloj, pausa, cerrar) — el resto en la fase 7 | portado en parte |
| `ui/BackgroundTasksUiTest` | `e2e/player.spec.ts` (la descarga aparece y termina en el indicador) — el resto en la fase 5 | portado en parte |

## Fase 1
| Java | Tauri | Estado |
|---|---|---|
| `ArchiveStoreTest` | `core/src/archive/tests.rs` (pendientes, numeración, `commit`, archive.txt, estados, alta de nuevos, enlazar con lo que ya hay en disco) y `maintenance.rs` («Eliminar todo», olvidar un formato) | portado; los de dispositivo (MTP) en la 9 |
| `ArchiveIOTest` | `core/src/archive/io.rs` | portado |
| `ArchiveSyncTest` | `core/src/archive/sync.rs` (choques de título, número de orden) | portado |
| `ArchiveQueryTest` | `core/src/archive/query.rs` (orden, portada, estados) | portado |
| `DownloadPathResolverTest` | `core/src/paths.rs` (los casos parecidos van juntos en un test) | portado; los iconos 🎵/🎬 son del front |
| `TaskHistoryTest` | `core/src/task_history.rs` | portado |
| — | `core/tests/java_compat.rs` (archive, `settings.json` y `tasks.json` con la forma exacta que escribe Java), `core/src/library.rs`, `settings.rs` | nuevos |

## Fase 2
| Java | Tauri | Estado |
|---|---|---|
| `PlaylistTransferTest` | `core/src/transfer/tests.rs` (43: manifiesto, copiar, reintentar, cancelar, espacio libre, importar, `findImportable`, `relinkFromManifest`, `keepWhatWasKnown`) | portado; los casos de `null` en argumentos no aplican (Rust no los admite) |
| — (estaba en `PlaylistManagerController`) | `src/transfer.test.ts` (textos y tono del resultado de copiar e importar, lista de la confirmación con su tope) | nuevo |
| — | `e2e/transfer.spec.ts` (copiar con confirmación y progreso, cancelar, importar desde la ayuda) | nuevo |

## Fase 3
| Java | Tauri | Estado |
|---|---|---|
| `YtDlpCommandBuilderTest` | `core/src/ytdlp/command.rs` (13) | portado + 3 nuevos (tabla de flags de metadatos por formato, la URL va la última, `--playlist-items`/pausas solo en playlist) |
| `YtDlpProgressParserTest` | `core/src/ytdlp/progress.rs` (14) | portado, con las dos regresiones |
| `OutputCleaningTest` | `core/src/ytdlp/cleaning.rs` (4) | portado |
| `UnavailableReasonTest` | `core/src/ytdlp/unavailable.rs` (8) + `src/library.test.ts` (`statusText`) | portado; el motivo es una clave de i18n y el texto lo pone el front |
| `VersionCompareTest` | `core/src/version.rs` (13) | portado |
| `YtDlpQueryServiceTest` | `core/src/ytdlp/query.rs` (15) | portado (las 3 regresiones de `--alias`) + nuevos: entradas de la playlist, título sin líneas de yt-dlp, id, cookies, error de un vídeo |
| — (`YtDlpClientResolver` no tenía) | `core/src/ytdlp/client.rs` (5) | nuevo |
| `ActivityLogFormatterTest`, `CustomLogFormatterTest` | `core/src/log_format.rs` (3) | portado; el caso «nunca imprime clase/método» no aplica (la función no los recibe) |
| — | `core/src/library.rs` (el motivo de un no disponible llega como clave) | nuevo |

## Pendientes
| Java | Fase |
|---|---|
| `YtDlpUpdaterTest`, `ZipExtractorTest`, `AppInfoTest` | 4 |
| `SafetyNetServiceTest`, `BackgroundTasksTest`, `ActivityLogTest`, `ui/BackgroundTasksUiTest` (resto) | 5 |
| `CoverArtTest` | 6 |
| `PlayerQueueTest`, `PlayerControllerTest`, `PlayerBarControllerTest`, `RelativeTimeTest`, `ui/PlayerUiTest` (resto) | 7 |
| `SettingsSearchIndexTest`, `ui/SettingsUiTest`, `ui/FormsUiTest`, `ui/HomeAndManagerUiTest`, `HomeControllerTest`, `NewPlaylistControllerTest`, `PlaylistManagerControllerTest`, `SongDetailsControllerTest` | 8 (o la fase de su pantalla) |
| `UpdateVerifierTest`, `UpdateInstallerTest`, `AppUpdaterTest`, `AppLauncherTest`, `ReleaseNotesTest`, `DevicePlaylistTransferTest` | 9 |
| `MotionTest` | descartado: las animaciones son CSS, no hay clase que probar |
