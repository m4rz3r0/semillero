# Presets de las versiones publicadas

Cada archivo es el preset que exportó una versión publicada de semillero, tal
cual lo escribió. `tests/presets_publicados.rs` comprueba que la versión de
ahora los carga todos con las mismas normas y reglas.

**No se editan nunca**: son lo que esas versiones escribieron. Si uno deja de
cargar, el fallo está en el código de ahora, no en el archivo.

## Cómo se hicieron

Con el código de cada versión, no a mano: en una copia de la versión (su
etiqueta `vX.Y.Z`; la 0.4.0, de la copia pública de GitHub) se añadió una
prueba desechable que crea un `Preset` y guarda lo que devuelve
`Preset::escribir`, la misma función que usa la descarga del preset
(`descargar_preset` en `crates/semillero/src/api/servidor.rs`).

| Archivo | Versión | Commit | Formato |
|---|---|---|---|
| `0.3.0.toml` | v0.3.0 | `add5c0a` | 1 |
| `0.3.1.toml` | v0.3.1 | `8ff255e` | 1 |
| `0.3.2.toml` | v0.3.2 | `0265ad1` | 1 |
| `0.4.0.toml` | v0.4.0 | `0b18b11` (GitHub) | 1 |
| `0.4.1.toml` | v0.4.1 | `5d74552` | 1 |
| `0.5.0.toml` | v0.5.0 | `46d67a3` | 1 |
| `0.6.0.toml` | v0.6.0 | `98818a0` | 1 (sin normas por tracker) |
| `0.6.0-trackers.toml` | v0.6.0 | `98818a0` | 2 (con normas para `tracker.ejemplo.example` y `segundo.example`) |

Todos llevan los mismos valores, distintos de los de por defecto: el nombre
«Exportado por semillero X.Y.Z», las normas (umbral 12,5 %, 11 días, ratio
1,25, 45 días, pausa a 2,75, aviso de 36 horas) y reglas en las tres
categorías con cada clave al menos una vez.

## Al publicar una versión

Haz su preset igual, con su propio código y los mismos valores, guárdalo aquí
como `X.Y.Z.toml`, añade la versión a `PUBLICADAS` y su prueba a
`presets_publicados!` en `tests/presets_publicados.rs`. Las pruebas fallan si
falta el archivo de una versión publicada o si hay un archivo sin prueba.
