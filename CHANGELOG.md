# Registro de cambios

Lo que cambia en cada versión publicada. Lo que viene después, en
[ROADMAP.md](ROADMAP.md).

## 0.4.1

Al actualizar no hay que hacer nada.

- Arreglado: desde la 0.3.0, sin `SEMILLERO_JELLYFIN_CLAVE` la clave de
  Jellyfin se busca junto a la base de datos, y una instalación que venía de
  la 0.2 (con la clave en `/usr/local/etc/semillero/jellyfin.clave`, la ruta
  por defecto de entonces) no arrancaba. Si la clave no está junto a la base
  de datos y sí en esa ruta, ahora se usa esa y el registro avisa de que
  conviene moverla o poner `SEMILLERO_JELLYFIN_CLAVE`.

## 0.4.0

Al actualizar no hay que hacer nada: la base de datos, los presets y las
variables de configuración no cambian, y lo nuevo es opcional.

- Si qBittorrent no acepta la contraseña con la que entra semillero (modo
  con contraseña), la web lo dice y explica que se cambia con
  `semillero configurar`. Antes decía «qBittorrent no responde».
- `semillero --env FICHERO` arranca con la configuración de ese fichero, en
  cualquier shell (sh, fish, PowerShell). Las variables ya puestas en el
  entorno mandan sobre él. La guía «a mano» lo usa en lugar de `set -a`.
- `semillero configurar` ofrece borrar el fichero de la clave que ya no se
  usa al quitar Jellyfin o al pasar a la lista blanca (sin «s», se queda).
- Actualizador de FreeBSD: con `GITHUB_REPO` en su configuración, puede
  traer la versión de la release de GitHub, comprobando `SHA256SUMS` y la
  atestación de GitHub (`gh attestation verify`), antes de compilarla en el
  servidor. Hace falta `pkg install gh jq`. Sin `GITHUB_REPO`, todo sigue
  igual. Para usarlo, vuelve a pasar `deploy/freebsd/install.sh`.

## 0.3.2

- La prueba del actualizador pasa shellcheck (aviso SC2209). El programa no
  cambia.

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
