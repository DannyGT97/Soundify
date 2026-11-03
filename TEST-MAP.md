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
| — | `src/urls.test.ts`, `src/scale.test.ts`, `src/library.test.ts` (`groupByStatus` incluido), `src/player.test.ts`, `src/keys.test.ts` (dónde escribe el espacio) | nuevos |
| — | `e2e/player.spec.ts`: regresiones de pulsar el archivo que ya suena (no reinicia), espacio = pausa/reproduce sin repetir el último botón, volumen en Reproduciendo con el reproductor solo en el menú | nuevos |
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

## Fase 4
| Java | Tauri | Estado |
|---|---|---|
| `YtDlpUpdaterTest` | `core/src/tools/updaters.rs` | portado (`isStale`, plan, versión local) + nuevos: flujos de actualizar con `Http` falso, instaladores de ffmpeg y Deno |
| `ZipExtractorTest` | `core/src/tools/zip_extract.rs` (8) | portado + 1 nuevo (barra invertida con contenido sigue siendo fichero) |
| `AppInfoTest` | `src/appInfo.test.ts` (2) | portado, con la regresión de `${project.version}` |
| — | `core/src/tools/status.rs` (5: qué hay y qué versión), `updaters.rs` (+5: última versión de yt-dlp, instalarlo, reinstalar ffmpeg sin perder el que funciona), `src/tools.test.ts` (6: qué falta, tamaño, chip de cada herramienta), `e2e/tools.spec.ts` (4: asistente hasta «Todo listo», «Ahora no» → Descarga bloqueada → pestaña Herramientas, reintentar tras un fallo, nada con todo instalado) | nuevos: puesta en marcha de las herramientas |
| — | `core/src/tools/health.rs` (5), `core/src/tools/progress.rs` (4), `updaters.rs` (+4: progreso y ffmpeg incluido), `src/toolDownloads.test.ts` (2), `core/src/http.rs` (5, uno `#[ignore]` que baja de verdad), `core/src/tools/mod.rs` (2), `query::filter_supported` (2), `src-tauri/src/process.rs` (4), `src/i18n.test.ts` (claves que entrega el back) | nuevos |

## Fase 5
| Java | Tauri | Estado |
|---|---|---|
| `SafetyNetServiceTest` | `core/src/engine/safety_net.rs` (18) | portado (`cleanErrorLine`) + nuevos: reintento por `player_client`, marcas de fallo, parar a medias |
| `BackgroundTasksTest` | `core/src/engine/tasks.rs` (7) | portado |
| `ActivityLogTest` | `core/src/engine/activity.rs` (11) | portado |
| — (el ciclo de `DownloadCoordinator` no tenía) | `core/src/engine/download/tests.rs` (18) | nuevo: descarga entera con un `Downloader` falso que emite salida de yt-dlp real; «Crear» registra sin descargar, título genérico, playlist que no se puede listar, numeración ya fijada; descarga suelta en la carpeta elegida |
| — | `core/src/ytdlp/cleaning.rs` (+3) | nuevo: el manifiesto nunca es un resto; una descarga suelta solo limpia lo suyo |
| `NewPlaylistControllerTest` (parte) | `src/createPlaylist.test.ts` (5) | portado: qué URL vale y el aviso de «registrada» (título genérico, modo de numeración heredado) |
| `ui/BackgroundTasksUiTest` (parte) | `src/tasks.test.ts` | nuevo: vista del back → `ui`, progreso, fechas, textos con claves |

## Fase 6
| Java | Tauri | Estado |
|---|---|---|
| `CoverArtTest` | `src/covers.test.ts` (7: recorte de franjas y forma, qué imagen lleva una playlist) + `core/src/cover_art.rs` (9: id válido, portada elegida, primer vídeo por `playlistIndex`, validar por bytes, caché) | portado; el recorte pasó al front (no hay crate `image`) |

## Fase 7
| Java | Tauri | Estado |
|---|---|---|
| `PlayerQueueTest` | `src/playerQueue.test.ts` (9) | portado, con un aleatorio con semilla |
| `PlayerControllerTest` | `src/search.test.ts` (3: buscador sin acentos ni mayúsculas) | portado |
| `PlayerBarControllerTest` | `src/player.test.ts` (`formatTime`, NaN incluido) + `e2e/player.spec.ts` (duración, siguiente, repetir, anterior reinicia) | portado |
| — | `src/player.test.ts` (`isPlayable`: lo decidido con la prueba de formatos en WebView2) | nuevo |
| — | `core/src/settings.rs` (+3: memoria del reproductor, ✕ olvida el último archivo, volumen acotado) | nuevo |
| `RelativeTimeTest` | `src/relativeTime.test.ts` (4) | portado; el back ya manda los días contados, así que se prueba desde el número de días (antes no había semana/mes/año en singular ni años) |
| `ui/PlayerUiTest` (resto) | `e2e/player.spec.ts` | portado; volver al segundo tras reiniciar se prueba a mano (la API de mentira no guarda entre recargas) |
| — | `e2e/manager.spec.ts` (3: bloque en descarga y hermanos en espera, «Ver archivos» en vivo, «Ir a la descarga») + `src/tasks.test.ts` (`blockKey`, `liveText`, `progressLine`) + `core/src/engine/tasks.rs` (`the_block_being_downloaded_is_known_only_while_it_runs`) y `download/tests.rs` (aviso `downloaded` por archivo) | nuevo: descarga en vivo en el Gestor |

## Fase 8
| Java | Tauri | Estado |
|---|---|---|
| `SettingsSearchIndexTest` | `src/settingsSearch.test.ts` (9, uno contra el diccionario real) | portado |
| `i18n/I18nKeysTest` (resto) | `src/i18n.test.ts` (los tres idiomas con las mismas claves y ninguna vacía; los `{n}` se conservan) | portado |
| `ui/SettingsUiTest` | `e2e/settings.spec.ts` (4: idioma que rehace la app y se recuerda, buscador, tema tras recargar, cookies y «Restablecer») | portado |
| — | `core/src/settings.rs` (preferencias: `appearance` ↔ `theme`, restablecer) y `core/src/logs.rs` (rotación) | nuevos |

## Fase 9
| Java | Tauri | Estado |
|---|---|---|
| `UpdateVerifierTest` | `core/src/updater/verify.rs` (18: firma Ed25519, hash, asset equivocado, solo releases del repositorio de GitHub) | portado |
| `UpdateInstallerTest` | `core/src/updater/install.rs` (16: sustituir el portable, esperar a que el exe se pueda escribir) | portado; `waitPidFromArgs` no se porta (se espera al fichero, no al proceso) |
| `AppUpdaterTest` | `core/src/updater/mod.rs` (19: versión, sufijo `-tauri`, qué release se propone, nunca borradores ni pre-releases) + `download.rs` (6) + `src/appUpdate.test.ts` (5) + `e2e/settings.spec.ts` (ventana con notas, fallo al instalar) | portado |
| `ReleaseNotesTest` | `core/src/updater/release_notes.rs` (7) | portado; las notas salen como texto |
| `AppLauncherTest` | — | descartado: el lanzador por Smart App Control no se porta (pendiente: firma de código) |
| `DevicePlaylistTransferTest` | `core/src/transfer/device/tests.rs` (32) + `core/src/device/mod.rs` (7) contra `FakeDevice` + `e2e/transfer.spec.ts` (copiar al teléfono, importar desde él) | portado |
| — | `core/src/engine/download/device/tests.rs` (15: sincronizar con el teléfono, lo que sobrevive a una desconexión) | nuevo (en Java, `DeviceSyncService` no tenía test) |
| — | `core/src/archive/device.rs` (8) + `e2e/transfer.spec.ts` («Eliminar todo») | nuevo |
| — | `src-tauri/src/mtp.rs` (1, `#[ignore]`: lista los dispositivos de la máquina) | MTP real: se lanza a mano; sin probar aún con un teléfono |

## Pendientes
| Java | Fase |
|---|---|
| `ui/BackgroundTasksUiTest` (resto: Playwright de la ventana de tareas) | 8 |
| `ui/FormsUiTest`, `ui/HomeAndManagerUiTest`, `HomeControllerTest`, `NewPlaylistControllerTest`, `PlaylistManagerControllerTest`, `SongDetailsControllerTest` | 8 (o la fase de su pantalla) |
| `MotionTest` | descartado: las animaciones son CSS, no hay clase que probar |
