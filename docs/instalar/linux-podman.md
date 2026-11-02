# Linux con podman

La imagen de contenedor de la versión, como servicio de systemd con quadlet,
sin root. Hace falta podman 4.4 o más nuevo.

Antes: [Preparar qBittorrent](qbittorrent.md).

Cambia `X.Y.Z` por la versión. Todo como tu usuario.

## 1. Descargar y comprobar la imagen

```sh
podman pull ghcr.io/m4rz3r0/semillero:X.Y.Z
```

Con la [CLI de GitHub](https://github.com/cli/cli) compruebas que la imagen
salió de la CI del repositorio:

```sh
gh attestation verify oci://ghcr.io/m4rz3r0/semillero:X.Y.Z --repo m4rz3r0/semillero
```

## 2. El quadlet

El quadlet viene de `packaging/podman/` del repositorio, en la etiqueta de
la versión:

```sh
mkdir -p ~/.config/containers/systemd ~/.config/semillero
cd ~/.config/containers/systemd
curl -fLo semillero.container \
  https://github.com/m4rz3r0/semillero/raw/vX.Y.Z/packaging/podman/semillero.container
sed -i 's|^Image=.*|Image=ghcr.io/m4rz3r0/semillero:X.Y.Z|' semillero.container
```

El contenedor tiene la red normal de podman y sale con una sola dirección,
así que semillero entra en qBittorrent en el
[modo con contraseña](qbittorrent.md#modo-con-contraseña-recomendado).

## 3. Configurar

`semillero configurar`, desde la propia imagen. Escribe la configuración en
`~/.config/semillero/semillero.env` y la contraseña de qBittorrent (y la
clave de Jellyfin) en el volumen de datos, solo para el usuario del
contenedor:

```sh
podman volume create --ignore semillero-datos
podman run --rm -it --user 0 \
  -v semillero-datos:/data:Z -v ~/.config/semillero:/config:Z \
  ghcr.io/m4rz3r0/semillero:X.Y.Z configurar --env /config/semillero.env --secretos /data
```

Intro deja el valor propuesto y las contraseñas no se ven al escribirlas.
No pregunta dónde escucha ni la base de datos: los fija la imagen. Como
dirección de qBittorrent, `http://host.containers.internal:8080` es este
equipo visto desde el contenedor (podman 5.3 o más nuevo; si no, la IP del
equipo en la red local); con qBittorrent en otro equipo, su IP. Antes de
guardar comprueba qBittorrent (y Jellyfin) y dice qué falla. Para cambiar
algo, vuelve a ejecutarlo: solo cambia lo que cambies.

Las demás variables están en
[packaging/podman/semillero.env.example](../../packaging/podman/semillero.env.example)
y en la [tabla del README](../../README.md#configuración).

## 4. Arrancar

```sh
systemctl --user daemon-reload
systemctl --user start semillero
loginctl enable-linger "$USER"     # que arranque sin sesión abierta
podman logs semillero
curl -s http://127.0.0.1:8090/salud
```

El registro tiene que decir `canario de acceso: qBittorrent rechaza una
contraseña al azar`. Abre `http://127.0.0.1:8090` y entra con el usuario y
la contraseña de qBittorrent.

Como root vale igual, con `/etc/containers/systemd/semillero.container`,
`/etc/semillero/semillero.env` y `systemctl` sin `--user`.

## Dónde queda cada cosa

| Qué | Dónde |
|---|---|
| Quadlet | `~/.config/containers/systemd/semillero.container` |
| Configuración | `~/.config/semillero/semillero.env` |
| Contraseña de qBittorrent, clave de Jellyfin | volumen `semillero-datos`, `/data/qbt.clave` y `/data/jellyfin.clave` dentro |
| Base de datos | volumen `semillero-datos`, `/data/semillero.db` dentro |
| Registro | `podman logs semillero` |

El contenedor corre con el usuario 65532, sin capacidades, con el sistema
de ficheros en solo lectura salvo `/data`.

## Jellyfin (opcional)

Vuelve a ejecutar la orden del paso 3 y contesta a la pregunta de Jellyfin
con su dirección y la clave de su API. Luego
`systemctl --user restart semillero`.

## Desde otro equipo

Ponlo detrás de un proxy inverso con HTTPS en este equipo y, con la orden
del paso 3, da su dirección pública (`https://semillero.example`) y la IP
desde la que el contenedor ve llegar al proxy (`SEMILLERO_PROXY`; la dice
el registro si no cuadra). Sin HTTPS no se puede entrar desde otro equipo:
el navegador no guarda la cookie de sesión.

## Actualizar

Cambia la versión en la línea `Image=` y reinicia:

```sh
podman pull ghcr.io/m4rz3r0/semillero:X.Y.Z
sed -i 's|^Image=.*|Image=ghcr.io/m4rz3r0/semillero:X.Y.Z|' ~/.config/containers/systemd/semillero.container
systemctl --user daemon-reload && systemctl --user restart semillero
```

Para que se actualice solo dentro de una serie, usa la etiqueta `X.Y`
(por ejemplo `ghcr.io/m4rz3r0/semillero:1.2`), descomenta `AutoUpdate=registry`
en el quadlet y activa `systemctl --user enable --now podman-auto-update.timer`.

## Quitarlo

```sh
systemctl --user stop semillero
rm ~/.config/containers/systemd/semillero.container && systemctl --user daemon-reload
rm -r ~/.config/semillero
podman volume rm semillero-datos    # borra la base de datos
```
