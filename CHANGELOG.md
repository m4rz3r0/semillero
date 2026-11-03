# Registro de cambios

Lo que cambia en cada versión publicada. Lo que viene después, en
[ROADMAP.md](ROADMAP.md).

## 0.5.0

El programa, la base de datos, los presets y la configuración de semillero
no cambian. El que cambia es el actualizador de FreeBSD (`deploy/`): si ya
lo usas, sigue «Migrar una instalación existente» en
[deploy/README.md](deploy/README.md) (y, si tenías buzón, «Migrar desde el
buzón») y vuelve a pasar `deploy/freebsd/install.sh`.

- Hace falta qBittorrent 5.0 o posterior. Con la 4.6 semillero no puede
  parar ni reanudar torrents (su API aún no tiene `torrents/stop` ni
  `torrents/start`); el README dice qué versiones se prueban.
- La integración continua prueba ahora donde se instala:
  - de punta a punta contra un qBittorrent de verdad (5.1 y 5.3): entrar,
    clasificar, avisar, cancelar un aviso y borrar en modo real;
  - una máquina FreeBSD (14 y 15) que sigue la guía con el `rc.d`;
  - en Windows, todas las pruebas, `semillero configurar`, el arranque y
    que la web contesta en `127.0.0.1`;
  - la actualización desde la versión publicada anterior, con su
    configuración y sus datos.
- Guías: la de FreeBSD crea `/usr/local/etc/rc.d` en la jail (una jail
  nueva no lo trae) y la de Windows cuenta todas las preguntas de
  `semillero configurar`. El ejemplo de configuración de Windows pone la
  contraseña de qBittorrent donde la deja `configurar`.
- Actualizador de FreeBSD:
  - Ya no compila en el servidor: fuera la jail de compilación,
    `PRECOMPILADAS_ESPERA` y `TRABAJOS_COMPILACION`. Las etiquetas las
    pregunta con `git ls-remote`, sin clon; con un `REPO_URL` por ssh, con
    una clave de despliegue de solo lectura (`CLAVE_DESPLIEGUE`,
    `HOSTS_CONOCIDOS`).
  - Ya no hay buzón. La versión de FreeBSD la compila, al subir una
    etiqueta `vX.Y.Z` a Forgejo, un runner de Forgejo Actions en el PC
    (`.forgejo/workflows/freebsd.yml`) y la publica como la release de
    Forgejo de esa etiqueta; con el PC apagado, el trabajo espera en cola.
    `semillero-actualizar` la descarga con un token de solo lectura
    (`FORGEJO_URL`, `FORGEJO_REPO`, `FORGEJO_TOKEN_FICHERO`; necesita
    `curl`) y la comprueba como antes; GitHub (`GITHUB_REPO`) sigue como
    segunda fuente.
  - Si ninguna fuente tiene la versión, espera sin límite y sin contarlo
    como fallo.
  - El registro dice por qué falla una descarga, y si `gh` no puede
    comprobar la atestación (por ejemplo, sin red) la versión de GitHub se
    espera en lugar de darse por rechazada.
  - Fuera `recibir-precompilada.sh`, `lanzar.sh`, `entregar.sh`,
    `compilar-freebsd.sh --entregar` y `--ultima`, el temporizador
    `semillero-cross` y `deploy/config.example`. Al migrar, quita el usuario
    `smentrega`, su línea de `authorized_keys` y de `AllowUsers`.

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
