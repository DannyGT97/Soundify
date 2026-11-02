# CLAUDE.md — Soundify (Tauri)

Port de Soundify (la app Java/JavaFX de `D:\Proyectos\soundifylocal`) a Tauri 2, pensando en
llevarla a Android. **De momento SOLO las vistas**: todas las pantallas enlazadas entre sí, con
datos de mentira. La lógica (yt-dlp, ffmpeg, deno, archives, reproductor real) viene después.

## Stack
- Tauri 2 + Vite + TypeScript sin framework (igual que `lienzo`)
- Dev: `npm run tauri dev` · solo el navegador: `npm run dev` (puerto 5180)
- Comprobar: `npm run build` (tsc + vite)

## Estructura (`src/`)
- `main.ts` — el armazón: menú + página + barra del reproductor + capa de ventanas superpuestas
- `router.ts` — rutas por hash (`#/manager`, `#/player?b=pl1:MP3`); cada visita rehace la pantalla,
  lo que tiene que sobrevivir vive en `state.ts`. `onLeave()` para timers y suscripciones
- `state.ts` — estado de la UI (menú plegado, resumen oculto, tema, ubicación del reproductor, lo que suena, la tarea); tema y ubicación se guardan en localStorage. `subscribe`/`emit` por tema (`player`, `tasks`, `theme`, `player-placement`)
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
- `mock.ts` — **DATOS DE MENTIRA**, en un solo fichero para sustituirlo de golpe por la lógica real
- `player.ts` / `tasks.ts` — **SIMULACIONES** (un temporizador) para ver la barra y el indicador
  de tareas funcionando; las acciones se quedan, el temporizador se cambia por el motor real
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
- **La migración de la lógica sigue [MIGRATION-PLAN.md](MIGRATION-PLAN.md)**: fases, tests y Android
- Lógica: comandos Rust en `src-tauri/src/lib.rs` (yt-dlp/ffmpeg/deno como sidecars en escritorio;
  en Android, youtubedl-android vía plugin Kotlin)
- Diálogos nativos (carpeta, portada, cookies): `tauri-plugin-dialog`
- Idiomas en/ca, y adaptar el diseño a pantalla de móvil
