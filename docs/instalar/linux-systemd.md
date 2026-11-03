# Linux con systemd

El paquete de la versión, en `/opt/semillero`, como servicio de systemd con
un usuario dinámico sin privilegios. Para Linux x86_64 o aarch64 con glibc
2.36 o más nueva (Debian 12, Ubuntu 22.10, Fedora 37 o posteriores).

Antes: [Preparar qBittorrent](qbittorrent.md).

Todo como root. Cambia `X.Y.Z` por la versión.

## 1. Descargar y comprobar

```sh
V=X.Y.Z
O=x86_64-unknown-linux-gnu        # o aarch64-unknown-linux-gnu
P=semillero-$V-$O
cd "$(mktemp -d)"
curl -fLO "https://github.com/m4rz3r0/semillero/releases/download/v$V/$P.tar.gz"
curl -fLO "https://github.com/m4rz3r0/semillero/releases/download/v$V/SHA256SUMS"
sha256sum -c SHA256SUMS --ignore-missing
```

Tiene que decir `…: OK`. Con la [CLI de GitHub](https://github.com/cli/cli)
compruebas además que el paquete salió de la CI del repositorio:

```sh
gh attestation verify "$P.tar.gz" --repo m4rz3r0/semillero
```

## 2. Instalar

```sh
tar -xzf "$P.tar.gz"
rm -rf /opt/semillero && mv "$P" /opt/semillero
install -m 0644 /opt/semillero/semillero.service /etc/systemd/system/semillero.service
systemctl daemon-reload
systemctl start semillero
```

El primer arranque, con los valores por defecto, crea la carpeta de datos
del servicio (`/var/lib/semillero`), donde irán la contraseña y las claves.

## 3. Configurar

```sh
/opt/semillero/semillero configurar \
  --env /etc/semillero/semillero.env --secretos /var/lib/semillero
```

Pregunta dónde escucha, la dirección con la que lo abres en el navegador,
la de qBittorrent y cómo entra en él (ver
[Preparar qBittorrent](qbittorrent.md)), el proxy inverso y Jellyfin. Intro
deja el valor propuesto. Las contraseñas no se ven al escribirlas.

Antes de guardar comprueba qBittorrent (y Jellyfin) y dice qué falla.
Escribe `/etc/semillero/semillero.env` y deja la contraseña de qBittorrent
y la clave de Jellyfin en `/var/lib/semillero`, cada una en su fichero,
solo para el usuario del servicio. Para cambiar algo, vuelve a ejecutarlo:
solo cambia lo que cambies.

El resto de variables están en `semillero.env.example` y en la
[tabla del README](../../README.md#configuración).

## 4. Arrancar

```sh
systemctl enable semillero
systemctl restart semillero
journalctl -u semillero -n 20
curl -s http://127.0.0.1:8090/salud
```

El registro tiene que decir `semillero escuchando en http://127.0.0.1:8090`
y `canario de acceso: qBittorrent rechaza una contraseña al azar`. Abre
`http://127.0.0.1:8090` en este equipo y entra con el usuario y la
contraseña de qBittorrent.

## Dónde queda cada cosa

| Qué | Dónde |
|---|---|
| Programa y web | `/opt/semillero` (solo lectura para el servicio) |
| Configuración | `/etc/semillero/semillero.env` |
| Contraseña de qBittorrent, clave de Jellyfin | `/var/lib/semillero/qbt.clave`, `jellyfin.clave` |
| Base de datos | `/var/lib/semillero/semillero.db` |
| Registro | `journalctl -u semillero` |

La unidad limita lo que puede hacer el servicio: sin privilegios, sin
acceso a `/home`, solo escribe en `/var/lib/semillero`.

## Jellyfin (opcional)

Vuelve a ejecutar `semillero configurar` (paso 3) y contesta a la pregunta
de Jellyfin con su dirección y la clave de su API. Luego
`systemctl restart semillero`.

## Desde otro equipo

semillero escucha solo en `127.0.0.1`. Para entrar desde fuera, ponlo
detrás de un proxy inverso con HTTPS en este equipo y, con
`semillero configurar`, da su dirección pública
(`https://semillero.example`) y la IP desde la que se conecta el proxy
(`127.0.0.1`), y solo esa. Sin HTTPS no se puede entrar desde otro equipo:
el navegador no guarda la cookie de sesión.

## Actualizar

Repite el paso 1, cambia el programa y reinicia (la configuración y los
datos se quedan):

```sh
tar -xzf "$P.tar.gz"
rm -rf /opt/semillero && mv "$P" /opt/semillero
install -m 0644 /opt/semillero/semillero.service /etc/systemd/system/semillero.service
systemctl daemon-reload && systemctl restart semillero
```

La base de datos se queda en `/var/lib/semillero` y se migra sola al
arrancar: la CI prueba que cualquier versión publicada desde la 0.3.0 se
actualiza así, sin pasos a mano.

## Quitarlo

```sh
systemctl disable --now semillero
rm -f /etc/systemd/system/semillero.service && systemctl daemon-reload
rm -rf /opt/semillero /etc/semillero
rm -rf /var/lib/private/semillero    # borra la base de datos
```
