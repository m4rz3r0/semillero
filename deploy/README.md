# Despliegue en FreeBSD (jail y actualizador)

Una forma de tener semillero al día en un host FreeBSD: el servicio corre en
una jail y un cron de root instala cada etiqueta `vX.Y.Z` nueva del
repositorio, ya compilada, con vuelta atrás si la versión nueva no responde.

Para instalarlo desde cero (la jail, el paquete de la versión, el rc.d del
servicio y cómo pasar luego a este actualizador), sigue la guía
[FreeBSD](../docs/instalar/freebsd.md). Aquí está el detalle del
actualizador.

Cada versión es una etiqueta `vX.Y.Z` con la misma versión en
`crates/semillero/Cargo.toml` (la que dice `/salud`). **El servidor no
compila nunca**: instala versiones ya compiladas, de una de estas dos
fuentes (la primera que la tenga y cuadre):

- **Forgejo** (`FORGEJO_URL`, `FORGEJO_REPO`): al subir la etiqueta, un
  runner de Forgejo Actions en un PC Linux la compila con `cross` y la
  publica como la release `vX.Y.Z` de Forgejo
  (`.forgejo/workflows/freebsd.yml`). Ver
  [Runner de Forgejo Actions en el PC](#runner-de-forgejo-actions-en-el-pc).
- **GitHub** (`GITHUB_REPO`): el paquete de FreeBSD de la release `vX.Y.Z`,
  con su atestación de origen comprobada. Ver
  [Versiones de GitHub](#versiones-de-github).

Cada 15 minutos `semillero-actualizar` pregunta las etiquetas al
repositorio con `git ls-remote` desde el host (no clona, no trae nada y no
ejecuta nada del repositorio) y se queda con la más alta y su commit. Si es
nueva, la descarga de la release de Forgejo con un token de solo lectura y
la comprueba (sha256, solo ficheros normales, el commit de la etiqueta, un
ejecutable de FreeBSD x86-64); si no está o no cuadra, la trae de GitHub
(con `GITHUB_REPO`). Si ninguna vale todavía, apunta una vez `esperando la
versión precompilada de X.Y.Z` y lo vuelve a probar en cada pasada, sin
límite y sin contarlo como fallo. La deja en
`/usr/local/semillero/releases/X.Y.Z` (montada en solo lectura en la jail
del servicio), para el servicio, copia la base de datos, cambia el enlace
`current`, arranca y espera hasta 60 s a que `/salud` (en `ESCUCHA`)
responda con la versión nueva. Si no, vuelve a la anterior con la base de
datos de antes.

## Qué hay aquí

| Fichero | Dónde corre |
|---|---|
| `freebsd/install.sh` | host, como root: instala el actualizador y escribe lo que queda a mano |
| `freebsd/actualizar.sh` | host: `/usr/local/sbin/semillero-actualizar` (cron) |
| `../.forgejo/workflows/freebsd.yml` | runner de Forgejo Actions del PC: al subir una etiqueta, compila y publica la release |
| `cross/publicar-forgejo.sh` | PC (lo lanza el flujo): compila la etiqueta y la sube a la release de Forgejo |
| `cross/compilar-freebsd.sh` | PC: compila y empaqueta una etiqueta (o, con `--prueba`, una referencia) |
| `lib/` | funciones de sh compartidas |
| `pruebas/` | pruebas sin servidor |

## Configuración del host

`/usr/local/etc/semillero-actualizar.conf` (root, 0600). La primera vez
`install.sh` escribe una plantilla y para: rellénala y repítelo. Si falta un
valor obligatorio, `install.sh` y el actualizador dicen cuál y no hacen nada.

| Clave | Para qué | Por defecto |
|---|---|---|
| `REPO_URL` | repositorio git con las etiquetas, p. ej. `ssh://git@192.0.2.20:2200/usuario/semillero.git` o `https://git.example/usuario/semillero.git` (público) | obligatoria |
| `CLAVE_DESPLIEGUE` | clave privada de despliegue del host (solo con `REPO_URL` por ssh; root, 0600) | `/usr/local/etc/semillero-actualizar/id_ed25519` |
| `HOSTS_CONOCIDOS` | `known_hosts` con la clave del servidor git (solo por ssh) | `/usr/local/etc/semillero-actualizar/known_hosts` |
| `REPO_HOST_CLAVE` | línea de `known_hosts` del servidor git; `install.sh` la escribe en `HOSTS_CONOCIDOS` si no existe | vacía |
| `ESCUCHA` | dirección:puerto de semillero en su jail, p. ej. `192.0.2.10:8090` o `[2001:db8::10]:8090` | obligatoria |
| `JAIL`, `JAIL_RAIZ` | jail del servicio y su raíz en el host | `semillero`, `/usr/local/jails/containers/<JAIL>` |
| `FORGEJO_URL` | URL de Forgejo, sin barra al final, p. ej. `https://forja.example` (vacía: no se mira Forgejo); necesita `curl` | vacía |
| `FORGEJO_REPO` | repositorio en Forgejo, `propietario/nombre` (vacía: no se mira Forgejo) | vacía |
| `FORGEJO_TOKEN_FICHERO` | fichero con el token de solo lectura de Forgejo (root, 0600) | `/usr/local/etc/semillero-actualizar/forgejo-token` |
| `GITHUB_REPO` | repositorio de GitHub `propietario/repositorio` del que traer la versión publicada, con su atestación (vacía: no se mira GitHub); necesita `gh` y `jq` | vacía |

Las claves de antes (`JAIL_COMPILAR`, `JAIL_COMPILAR_RAIZ`, `JAIL_COMPILAR_IP`,
`PRECOMPILADAS_ESPERA`, `TRABAJOS_COMPILACION`, `REPO_IP`, `REPO_PUERTO`,
`DNS_IP`, `RED_LAN`, `INTERFAZ_EXT`, `USUARIO_COMPILAR`, `UID_COMPILAR`,
`USUARIO_BUZON`, `UID_BUZON`) ya no se usan; si siguen en la configuración,
no molestan.

Con Forgejo, el actualizador llama a `curl` con la cabecera
`Authorization: token …` por la entrada estándar (`curl -K -`), nunca en
los argumentos ni en la URL; el fichero del token tiene que ser de root y
que nadie más lo pueda leer, o Forgejo no se usa (y lo dice una vez).

Por ssh, el actualizador llama a `git ls-remote --tags` con

```sh
GIT_SSH_COMMAND="ssh -F /dev/null -i <CLAVE_DESPLIEGUE> -o IdentitiesOnly=yes \
    -o StrictHostKeyChecking=yes -o UserKnownHostsFile=<HOSTS_CONOCIDOS> \
    -o GlobalKnownHostsFile=/dev/null -o BatchMode=yes -o ConnectTimeout=15"
```

sin la configuración de git ni la de ssh de root. La clave tiene que ser
de root y que nadie más la pueda leer; el `known_hosts`, de root y que
nadie más lo pueda escribir. Si no, no se conecta y lo dice.

## Preparar el host (una vez)

Como root, desde una copia del repositorio:

1. `pkg install git` (y `curl` si usas Forgejo, `gh jq` si usas
   `GITHUB_REPO`).
2. `sh deploy/freebsd/install.sh`; rellena la configuración y repítelo.
   Instala el actualizador, el cron, newsyslog, `/usr/local/semillero/releases`
   y el punto de montaje en la jail; con `REPO_URL` por ssh, crea la clave
   de despliegue del host y, con `REPO_HOST_CLAVE`, el `known_hosts`; con
   Forgejo, la carpeta del token (0700). Escribe ya rellenados los pasos
   siguientes. No crea ninguna jail de compilación.
3. **Montaje**: la línea de `/etc/fstab.<JAIL>` y la orden `mount -t nullfs`
   que escribe `install.sh` (solo lectura).
4. **Clave de despliegue** (solo por ssh): da de alta la clave pública que
   escribe `install.sh` como clave de solo lectura del repositorio. Si no
   pusiste `REPO_HOST_CLAVE`, fija la clave del servidor git en
   `HOSTS_CONOCIDOS` (`ssh-keyscan`, comparando la huella con
   `ssh-keygen -lf`).
5. **Token de Forgejo** (solo con Forgejo): crea en Forgejo un token de
   acceso con permiso de solo lectura del repositorio (`read:repository`)
   y guárdalo en `FORGEJO_TOKEN_FICHERO`, de root y 0600:

   ```sh
   install -m 600 -o root -g wheel /dev/null /usr/local/etc/semillero-actualizar/forgejo-token
   ee /usr/local/etc/semillero-actualizar/forgejo-token
   ```
6. **Servicio**: el `rc.d` de semillero en la jail arranca
   `/usr/local/semillero/current/semillero` escuchando en `ESCUCHA`. Publica
   una versión y pasa el actualizador a mano:

   ```sh
   semillero-actualizar --comprobar
   semillero-actualizar
   tail /var/log/semillero-update.log
   ```

Si cambia algo de `deploy/`, repite `install.sh`.

## Migrar una instalación existente

Las instalaciones de antes leían las etiquetas y compilaban en una jail de
compilación (`JAIL_COMPILAR`, por defecto `semillero-build`). Ahora todo lo
que hace falta está en el host. Como root, con `JAIL_COMPILAR` y
`USUARIO_COMPILAR` los de tu configuración (por defecto `semillero-build` y
`smbuild`):

1. `pkg install git` en el host (y `gh jq` si usas `GITHUB_REPO`).
2. Pasa la clave de despliegue y el `known_hosts` de la jail al host. Léelos
   como el usuario de la jail (`jexec -U`), no abriendo sus rutas desde el
   host: lo que se compilaba allí podía haber dejado enlaces.

   ```sh
   install -d -m 700 /usr/local/etc/semillero-actualizar
   cd /usr/local/etc/semillero-actualizar
   ( umask 077
     jexec -U smbuild semillero-build cat /home/smbuild/.ssh/id_ed25519 >id_ed25519
     jexec -U smbuild semillero-build cat /home/smbuild/.ssh/id_ed25519.pub >id_ed25519.pub
     jexec -U smbuild semillero-build cat /home/smbuild/.ssh/known_hosts >known_hosts )
   chmod 600 id_ed25519; chmod 644 id_ed25519.pub known_hosts
   ssh-keygen -y -f id_ed25519        # tiene que coincidir con id_ed25519.pub
   ssh-keygen -lf known_hosts         # compara la huella con la del servidor git
   ```

   El `known_hosts` de la jail debe tener solo la línea del servidor git; si
   tiene más, deja solo esa. Otra forma: no copies nada, deja que
   `install.sh` cree una clave nueva, dala de alta en el servidor git y
   borra allí la de la jail.
3. Repite `sh deploy/freebsd/install.sh`: no toca la configuración, la
   clave ni el `known_hosts` que ya están, instala el actualizador nuevo y
   avisa de que la jail de compilación sobra.
4. Comprueba que llega al repositorio sin la jail:

   ```sh
   semillero-actualizar --comprobar
   ```

   Tiene que decir `al día: X.Y.Z` o la versión nueva, no `sin acceso al
   repositorio`.
5. Ya se puede quitar la jail: `service jail stop semillero-build`, su
   bloque de `/etc/jail.conf`, sus reglas de `/etc/pf.conf` (la `nat` y el
   filtro de `JAIL_COMPILAR_IP`; comprueba con `pfctl -nf /etc/pf.conf` y
   cárgalas con `pfctl -f /etc/pf.conf`) y su raíz (p. ej.
   `zfs destroy -r <dataset de la jail>` o `chflags -R noschg` y `rm -rf`).
   Si el servidor git le daba acceso a ella por IP, quítalo. También sobran
   `/var/log/semillero-build.log` y, si ningún otro proyecto lo usa,
   `/var/run/compilar.lock`.
6. Opcional: quita de la configuración las claves que ya no se usan (ver
   [Configuración del host](#configuración-del-host)).

### Migrar desde el buzón

Antes el PC dejaba las versiones en un buzón del servidor (el usuario
`smentrega`, `recibir-precompilada.sh` y el temporizador
`semillero-cross.timer` del PC). Ahora las publica el runner como release de
Forgejo y el servidor las descarga. Como root en el servidor, con
`smentrega` el `USUARIO_BUZON` de tu configuración:

1. `pkg install curl`. Añade a la configuración `FORGEJO_URL`,
   `FORGEJO_REPO` y, si no usas la ruta por defecto,
   `FORGEJO_TOKEN_FICHERO`, y repite `sh deploy/freebsd/install.sh`.
2. Pon el token de solo lectura (paso 5 de
   [Preparar el host](#preparar-el-host-una-vez)) y comprueba:
   `semillero-actualizar --comprobar` tiene que decir
   `versiones de Forgejo: … (curl y token listos)`.
3. Quita el buzón: su línea de `authorized_keys` (la que fuerza
   `recibir-precompilada.sh`, en `/var/db/semillero-buzon/.ssh/` o donde la
   pusieras), el usuario y su casa, y la orden:

   ```sh
   pw userdel smentrega
   rm -rf /var/db/semillero-buzon
   rm -f /usr/local/libexec/semillero-actualizar/recibir-precompilada.sh
   ```

4. Si `sshd` tenía `smentrega` en `AllowUsers` (o su grupo en
   `AllowGroups`), quítalo de `/etc/ssh/sshd_config`, compruébalo con
   `sshd -t` y `service sshd reload`.
5. En el PC: `systemctl --user disable --now semillero-cross.timer`, borra
   `~/.config/systemd/user/semillero-cross.{service,timer}`,
   `~/.config/semillero-cross.env`, la clave `~/.ssh/semillero-buzon*` (y la
   línea del servidor en `~/.ssh/known_hosts` si solo era para el buzón) y
   `~/semillero-cross/bin`. Si diste al PC una clave de despliegue en el
   servidor git para el temporizador, bórrala también. `deploy/config.local`
   del equipo de trabajo ya no se usa.

## Runner de Forgejo Actions en el PC

Al subir una etiqueta `v*` a Forgejo, `.forgejo/workflows/freebsd.yml`
lanza un trabajo con `runs-on: pc`: trae la etiqueta (solo git, con el
token del trabajo en una cabecera que va por el entorno) y corre
`deploy/cross/publicar-forgejo.sh`, que compila con
`compilar-freebsd.sh --release` y sube el paquete y `SHA256SUMS` a la
release de la etiqueta (la crea si no existe; si se repite, reemplaza los
ficheros). No usa acciones de terceros. Que exista `.forgejo/workflows/`
hace que Forgejo no corra los flujos de `.github/workflows/`.

En Forgejo: Actions activado en la instancia (`[actions] ENABLED = true` en
`app.ini`) y en el repositorio (Configuración > Unidades > Actions).

En el PC (Linux con `rustup`, `cargo`, `cross`, `podman`, `git`, `curl`,
`jq`, `rsync`, `file` y `flock`), como tu usuario:

1. Instala `forgejo-runner` (el paquete de tu distribución o el binario de
   las releases de forgejo-runner).
2. Regístralo con la etiqueta `pc` y el ejecutor host (los trabajos corren
   como tu usuario, sin contenedor). El token de registro sale de Forgejo
   (Configuración > Actions > Runners, del repositorio, de tu usuario o de
   la instancia):

   ```sh
   mkdir -p ~/.config/forgejo-runner && cd ~/.config/forgejo-runner
   forgejo-runner generate-config >config.yml   # deja runner.capacity: 1
   forgejo-runner register --no-interactive --instance https://forja.example \
       --token <token de registro> --name pc --labels pc:host
   ```

   `register` deja el fichero `.runner` en esa carpeta.
3. Hazlo servicio de usuario, `~/.config/systemd/user/forgejo-runner.service`
   (el `PATH` tiene que llevar a `cargo`, `cross` y `rustup`):

   ```ini
   [Unit]
   Description=Runner de Forgejo Actions
   After=network-online.target
   Wants=network-online.target

   [Service]
   WorkingDirectory=%h/.config/forgejo-runner
   Environment=PATH=%h/.cargo/bin:/usr/local/bin:/usr/bin
   ExecStart=/usr/bin/forgejo-runner daemon --config %h/.config/forgejo-runner/config.yml
   Restart=on-failure

   [Install]
   WantedBy=default.target
   ```

   ```sh
   systemctl --user daemon-reload
   systemctl --user enable --now forgejo-runner
   loginctl enable-linger "$USER"   # que arranque sin abrir sesión
   ```

La compilación es incremental: `~/semillero-cross` (el clon, la fuente con
`target/` y `salida/`) se conserva entre trabajos. Prueba a compilar sin
publicar nada con `deploy/cross/compilar-freebsd.sh --prueba` desde una
copia del repositorio.

**Con el PC apagado** la etiqueta espera: Forgejo deja el trabajo en cola
hasta que el runner se conecta, y el servidor apunta una vez que espera la
versión. Un trabajo que nadie recoge se da por abandonado tras
`ABANDONED_JOB_TIMEOUT` (`[actions]` de `app.ini`; por defecto 24 h):
súbelo (p. ej. `720h`, 30 días) si el PC puede pasar días apagado. Si se
abandona, vuelve a lanzarlo desde la pestaña Actions o borra y sube otra
vez la etiqueta.

## Publicar una versión

1. Pasa las pruebas (`cargo test --workspace --features semillero/ssr` y
   `deploy/pruebas/`).
2. Sube `version` en `crates/semillero/Cargo.toml`, haz commit, etiqueta y
   súbelo: `git tag vX.Y.Z && git push && git push origin vX.Y.Z`.
3. Súbela a Forgejo: el runner del PC la compila y publica la release
   (`publicar-forgejo.sh` se niega si la etiqueta no es la versión de
   `Cargo.toml`). Con el PC apagado, el trabajo espera en cola.

El servidor la instala en la primera pasada del cron en que la release de
Forgejo (o la de GitHub, con `GITHUB_REPO`) esté completa.

## Versiones de GitHub

Con `GITHUB_REPO=propietario/repositorio` (el repositorio público donde
`release.yml` publica las versiones, p. ej. `m4rz3r0/semillero`), el
actualizador puede instalar el paquete de FreeBSD de la release `vX.Y.Z`.
Necesita en el host `pkg install gh jq`; `install.sh` lo comprueba y, si
falta, dice qué paquete y para. No hace falta sesión de `gh` ni token.

Para una etiqueta nueva, en este orden:

1. **Forgejo**, si la release ya está completa y cuadra.
2. **GitHub**: descarga (con `fetch`, límites de tamaño y de tiempo) el
   paquete `semillero-X.Y.Z-x86_64-unknown-freebsd.tar.gz` y `SHA256SUMS`,
   comprueba el sha256 con la línea del paquete, trae la atestación de
   `https://api.github.com/repos/<GITHUB_REPO>/attestations/sha256:<sha256>`
   (sin token; `jq` saca cada `bundle`) y la comprueba con esto (`gh` baja
   la raíz de confianza de Sigstore: el host necesita salida HTTPS a GitHub
   y a Sigstore):

   ```sh
   gh attestation verify <paquete> --bundle <atestaciones.jsonl> \
       --repo <GITHUB_REPO> --source-ref refs/tags/vX.Y.Z \
       --signer-workflow <GITHUB_REPO>/.github/workflows/release.yml
   ```

   Luego, las mismas comprobaciones que la de Forgejo (solo ficheros normales, una sola
   carpeta, un ejecutable de FreeBSD x86-64) y la web completa. No se mira
   que `COMMIT` sea el de la etiqueta del repositorio de `REPO_URL`: el
   repositorio público es una copia aplanada y sus commits son otros; lo que
   ata el paquete a la etiqueta es la atestación. De lo que trae solo se
   instalan `semillero`, `site/` y `COMMIT` (no `LICENSE`, los README ni
   `rc.d/`).
3. **Esperar**: si ninguna de las dos la tiene o cuadra, se espera.

La release sale unos minutos después de la etiqueta: mientras tanto, el
registro dice una vez `esperando la versión precompilada de X.Y.Z (Forgejo
… o GitHub …; Forgejo: la release vX.Y.Z aún no está publicada; GitHub: no
se puede descargar …)` y el actualizador la vuelve a buscar en cada pasada.
Si el sha256, el paquete o la atestación no cuadran, es un `ERROR`
(`versión de Forgejo de vX.Y.Z rechazada: …` o `versión de GitHub de vX.Y.Z
rechazada: …`, una vez por etiqueta, fuente y motivo), no se instala nada
de esa fuente y sigue esperando; no cuenta como fallo de la etiqueta. El
motivo de una descarga de GitHub que falla es lo que contesta `fetch`. `gh` sale con
1 tanto si la atestación no cuadra como si no puede comprobarla: el
actualizador solo toma por «no se puede comprobar» (se espera, sin `ERROR`)
su `error creating Sigstore verifier` (sin red para la raíz de Sigstore) y un
`gh` que no termina o no arranca; cualquier otro fallo es un rechazo. Si una versión de GitHub
falla al desplegarse, la pasada siguiente lo vuelve a intentar; tras dos
fallos seguidos, la etiqueta queda como fallida.

`gh` y `jq` corren como root, pero solo leen lo descargado (como `tar` y
`file` con lo de Forgejo), en una carpeta propia y con una casa vacía; nada de lo
descargado se ejecuta antes de comprobarlo.

## Qué ha pasado

- `/var/log/semillero-update.log`: espera (una vez por etiqueta), instalada
  o rechazada (y por qué), despliegue, vuelta atrás y errores.
- `semillero-actualizar --comprobar`: qué haría, sin cambiar nada; también
  si Forgejo está configurado y si están `curl` y el token (y, con
  `GITHUB_REPO`, el repositorio y si están `gh` y `jq`). No descarga nada.
- En Forgejo, la pestaña Actions del repositorio: el registro de cada
  trabajo del runner.
- En el PC, `~/semillero-cross/compilacion.log` y `~/semillero-cross/salida/`.

## A mano

- **Una rama o un commit:** el servidor no compila; ya no hay `--ref`.
  Compílala en el PC (`deploy/cross/compilar-freebsd.sh --prueba`) y
  despliégala a mano como cualquier versión (ver
  [la guía](../docs/instalar/freebsd.md#actualizar-a-mano)).
- **Volver atrás:** en `<JAIL_RAIZ>/usr/local/semillero`,
  `ln -sfh releases/X.Y.Z current.new && mv -fh current.new current` y
  `service -j <JAIL> semillero restart`. La base de datos de antes del
  último despliegue está en `/var/db/semillero/antes-de-actualizar/` dentro
  de la jail. Para quedarte ahí, marca la última como fallida
  (`echo vX.Y.Z >/var/db/semillero-actualizar/fallida`).
- **Etiqueta fallida:** tras 2 fallos seguidos queda en
  `/var/db/semillero-actualizar/fallida` y no se reintenta hasta que haya
  otra. Para reintentarla: `rm /var/db/semillero-actualizar/fallida
  /var/db/semillero-actualizar/intentos`.

## Pruebas

Sin servidor ni PC, en una carpeta temporal y con direcciones de
documentación:

- `deploy/pruebas/actualizar.sh`: el actualizador e `install.sh`.
- `deploy/pruebas/precompiladas.sh`: `lib/`, la descarga de Forgejo y los
  guiones del PC (`compilar-freebsd.sh`, `publicar-forgejo.sh`).
- `deploy/pruebas/sintaxis.sh`: `sh -n`/`bash -n` (y `shellcheck`, si está).
