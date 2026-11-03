# FreeBSD

semillero en una jail propia, con el paquete de la versión. Primero a mano;
después, si quieres, con el actualizador de `deploy/`, que instala solo cada
versión nueva y vuelve atrás si falla. Para FreeBSD 14 o más nuevo, x86_64.

Antes: [Preparar qBittorrent](qbittorrent.md).

Todo como root en el host. Los ejemplos usan direcciones de documentación;
pon las tuyas:

| Qué | Ejemplo |
|---|---|
| Interfaz de red del host | `em0` |
| qBittorrent (en otra jail o equipo) | `192.0.2.10:8080` |
| IP de semillero | `192.0.2.11` |
| IP para comprobar contraseñas (solo en modo lista blanca) | `192.0.2.12` |
| Proxy inverso con HTTPS | `192.0.2.1`, `https://semillero.home.arpa` |

Cambia `X.Y.Z` por la versión.

## 1. La jail

```sh
mkdir -p /usr/local/jails/containers
bsdinstall jail /usr/local/jails/containers/semillero
```

`bsdinstall` descarga el sistema base; elige solo `base`. Añade a
`/etc/jail.conf`:

```text
semillero {
	path = "/usr/local/jails/containers/semillero";
	host.hostname = "semillero";
	interface = "em0";
	ip4.addr = 192.0.2.11, 192.0.2.12;
	ip4.saddrsel = 0;
	exec.start = "/bin/sh /etc/rc";
	exec.stop = "/bin/sh /etc/rc.shutdown";
	exec.clean;
	mount.devfs;
}
```

La segunda IP y `ip4.saddrsel = 0` solo hacen falta en el
[modo lista blanca](qbittorrent.md#modo-lista-blanca): semillero habla con
qBittorrent desde la primera y comprueba contraseñas desde la segunda. En
el modo con contraseña (el recomendado), basta `ip4.addr = 192.0.2.11;`.

```sh
sysrc jail_enable=YES
service jail start semillero
jexec semillero pw useradd semillero -c semillero -d /nonexistent -s /usr/sbin/nologin
```

## 2. Descargar y comprobar

```sh
V=X.Y.Z
P=semillero-$V-x86_64-unknown-freebsd
cd "$(mktemp -d)"
fetch "https://github.com/m4rz3r0/semillero/releases/download/v$V/$P.tar.gz"
fetch "https://github.com/m4rz3r0/semillero/releases/download/v$V/SHA256SUMS"
sha256 -c "$(awk -v f="$P.tar.gz" '$2 == f { print $1 }' SHA256SUMS)" "$P.tar.gz"
```

Si el hash no cuadra, `sha256` lo dice y sale con error. Con la CLI de GitHub
(`pkg install gh`) compruebas además que salió de la CI del repositorio:

```sh
gh attestation verify "$P.tar.gz" --repo m4rz3r0/semillero
```

## 3. Instalar en la jail

Cada versión va en su carpeta y `current` apunta a la que corre:

```sh
J=/usr/local/jails/containers/semillero
mkdir -p "$J/usr/local/semillero/releases"
tar -xzf "$P.tar.gz" -C "$J/usr/local/semillero/releases"
mv "$J/usr/local/semillero/releases/$P" "$J/usr/local/semillero/releases/$V"
ln -sfh "releases/$V" "$J/usr/local/semillero/current"
```

## 4. El arranque (rc.d)

El paquete trae el arranque en `rc.d/semillero`: lo arranca con
`daemon(8)` como el usuario `semillero`, lee la configuración de
`semillero_env_file` y lo reinicia si se cae. En una jail recién creada
aún no existe `/usr/local/etc/rc.d` (la crea `pkg` al instalar el primer
paquete):

```sh
mkdir -p "$J/usr/local/etc/rc.d"
install -m 555 "$J/usr/local/semillero/releases/$V/rc.d/semillero" \
  "$J/usr/local/etc/rc.d/semillero"
```

## 5. Configurar y arrancar

`semillero configurar`, dentro de la jail, escribe la configuración en
`/usr/local/etc/semillero/semillero.env` (la que lee el rc.d) y la
contraseña de qBittorrent y la clave de Jellyfin en `/var/db/semillero`,
solo para el usuario `semillero`:

```sh
jexec semillero install -d -o semillero -m 700 /var/db/semillero
jexec semillero /usr/local/semillero/current/semillero configurar \
  --env /usr/local/etc/semillero/semillero.env --secretos /var/db/semillero
```

Intro deja el valor propuesto y las contraseñas no se ven al escribirlas.
Con los ejemplos de arriba:

| Pregunta | Respuesta |
|---|---|
| Dónde escucha | `192.0.2.11:8090` |
| Dirección en el navegador | `https://semillero.home.arpa` |
| qBittorrent | `http://192.0.2.10:8080` |
| Modo | `1` (con contraseña) |
| Usuario y contraseña | los de la interfaz web de qBittorrent |
| Proxy inverso | `192.0.2.1` |

Antes de guardar comprueba qBittorrent (y Jellyfin) y dice qué falla. Para
cambiar algo, vuelve a ejecutarlo: solo cambia lo que cambies.

```sh
jexec semillero sysrc semillero_enable=YES
jexec semillero service semillero start
tail "$J/var/log/semillero-daemon.log"
```

El registro tiene que decir `semillero escuchando en http://192.0.2.11:8090`
y `canario de acceso: qBittorrent rechaza una contraseña al azar`.

El proxy inverso (`192.0.2.1`) sirve `https://semillero.home.arpa` y
reenvía a `http://192.0.2.11:8090`, añadiendo la IP del cliente al final de
`X-Forwarded-For`. Sin HTTPS no se puede entrar: el navegador no guarda la
cookie de sesión.

Con el [modo lista blanca](qbittorrent.md#modo-lista-blanca) (`2`), la jail
necesita la segunda IP (`192.0.2.12`): en qBittorrent, la lista blanca lleva
solo `192.0.2.11/32`, y en la configuración va
`SEMILLERO_LOGIN_DESDE=192.0.2.12` (añádela a mano al fichero, que
`semillero configurar` respeta).

El fichero tiene que ser de root y que nadie más pueda escribirlo; solo
cuentan las variables `SEMILLERO_*`. Las demás están en la
[tabla del README](../../README.md#configuración). En `/etc/rc.conf` de la
jail también vale `semillero_env="VARIABLE=valor …"`, que manda sobre el
fichero.

## Dónde queda cada cosa (dentro de la jail)

| Qué | Dónde |
|---|---|
| Versiones | `/usr/local/semillero/releases/X.Y.Z` |
| La que corre | `/usr/local/semillero/current` |
| Configuración | `/usr/local/etc/semillero/semillero.env` |
| Contraseña de qBittorrent, clave de Jellyfin | `/var/db/semillero/qbt.clave`, `jellyfin.clave` |
| Base de datos | `/var/db/semillero/semillero.db` |
| Registro | `/var/log/semillero-daemon.log` |

## Actualizar a mano

Repite los pasos 2 y 3 con la versión nueva y reinicia:

```sh
jexec semillero service semillero restart
```

Para volver atrás, apunta `current` a la versión anterior y reinicia.

## Actualizar solo, con el actualizador

`deploy/freebsd/` instala en el host un cron que cada 15 minutos busca la
última etiqueta `vX.Y.Z` (con `git ls-remote`, sin clonar nada), toma el
paquete de FreeBSD de su release de GitHub (comprobando `SHA256SUMS` y la
atestación, como arriba) o de la release de Forgejo que publica un runner
de Forgejo Actions en un PC (con un token de solo lectura), para el servicio, copia la base de datos, cambia `current` y
arranca. Si `/salud` no responde con la versión nueva en 60 s, vuelve a la
anterior. El servidor no compila nunca: mientras no haya un paquete
comprobado, espera. Usa la misma jail, el mismo usuario y el mismo rc.d de
esta guía.

Los detalles de cada paso están en [deploy/README.md](../../deploy/README.md).
En resumen:

1. Una copia del repositorio en el host (y `git`, que el actualizador usa
   para ver las etiquetas):

   ```sh
   pkg install git gh jq
   git clone https://github.com/m4rz3r0/semillero.git /root/semillero
   cd /root/semillero && git checkout vX.Y.Z
   ```

2. Las versiones pasan a vivir en el host y la jail las ve en solo lectura.
   Con el servicio parado:

   ```sh
   jexec semillero service semillero stop
   mkdir -p /usr/local/semillero
   mv "$J/usr/local/semillero/releases" /usr/local/semillero/releases
   mkdir "$J/usr/local/semillero/releases"
   ```

3. `sh deploy/freebsd/install.sh`. La primera vez escribe
   `/usr/local/etc/semillero-actualizar.conf` y para. Rellénala; con el
   repositorio en GitHub:

   ```sh
   REPO_URL=https://github.com/m4rz3r0/semillero.git
   ESCUCHA=192.0.2.11:8090
   GITHUB_REPO=m4rz3r0/semillero
   ```

   `GITHUB_REPO`: instala el paquete de la release de GitHub si su
   atestación es de esa etiqueta y de `release.yml`; necesita
   `pkg install gh jq` en el host (sin sesión de `gh`). La release sale unos
   minutos después de la etiqueta: hasta entonces, el actualizador espera.
   Con `REPO_URL` por `https` no hace falta clave de despliegue. Más en
   [Versiones de GitHub](../../deploy/README.md#versiones-de-github).
4. Repite `install.sh` y sigue lo que escribe: el montaje en solo lectura
   (paso 3 de
   [Preparar el host](../../deploy/README.md#preparar-el-host-una-vez)). Con
   un repositorio público y sin runner en el PC no hace falta ni la clave
   de despliegue ni el token de Forgejo.
5. Arranca el servicio y prueba el actualizador:

   ```sh
   jexec semillero service semillero start
   semillero-actualizar --comprobar
   semillero-actualizar
   tail /var/log/semillero-update.log
   ```
