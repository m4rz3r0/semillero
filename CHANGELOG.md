# Registro de cambios

Lo que cambia en cada versión publicada. Lo que viene después, en
[ROADMAP.md](ROADMAP.md).

## 0.3.1

- Las pruebas del actualizador de FreeBSD pasan también en Debian y Ubuntu,
  donde `sh` no tiene `pipefail`. El programa no cambia.
- Hoja de ruta hacia la 1.0 ([ROADMAP.md](ROADMAP.md)) y este registro de
  cambios.

## 0.3.0

Primera versión pública.

- Clasificación de los torrents según las normas del tracker, reglas de
  borrado por categoría con aviso previo, presets, historial y aviso a
  Jellyfin tras borrar.
- `semillero configurar`: pide las direcciones y las claves, comprueba que
  qBittorrent y Jellyfin contestan y guarda la configuración y cada clave en
  su fichero, solo legible por su dueño.
- Modo con contraseña para entrar en qBittorrent desde otra máquina, podman
  o Windows.
- Binarios para Linux (x86_64 y aarch64), FreeBSD y Windows, e imagen de
  contenedor, con sumas SHA256 y atestación de GitHub.
