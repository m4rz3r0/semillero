# Despliegue en FreeBSD (jail y actualizador)

Una forma de tener semillero al día en un host FreeBSD: el servicio corre en
una jail y un cron de root instala cada etiqueta `vX.Y.Z` nueva del
repositorio, con vuelta atrás si la versión nueva no responde.

Para instalarlo desde cero (la jail, el paquete de la versión, el rc.d del
servicio y cómo pasar luego a este actualizador), sigue la guía
[FreeBSD](../docs/instalar/freebsd.md). Aquí está el detalle del
actualizador.

Cada versión es una etiqueta `vX.Y.Z` con la misma versión en
`crates/semillero/Cargo.toml` (la que dice `/salud`). Hay dos formas de
compilarla:

- **En un PC Linux** (opcional): la compila con `cross` y la deja en un
  **buzón** del servidor (`deploy/cross/lanzar.sh vX.Y.Z`).
- **En el propio servidor**, en una jail de compilación sin privilegios, si
  no llega al buzón a tiempo (o si no usas el PC).

Y, si quieres, el servidor puede tomar la versión que publica GitHub
(`GITHUB_REPO`): la de la release `vX.Y.Z`, con su atestación de origen
comprobada. Ver [Versiones de GitHub](#versiones-de-github-opcional).

Cada 15 minutos `semillero-actualizar` trae las etiquetas, se queda con la
más alta y, si es nueva, espera `PRECOMPILADAS_ESPERA` minutos a que llegue
al buzón. La comprueba (sha256, solo ficheros normales, el commit de la
etiqueta, un ejecutable de FreeBSD x86-64); si no llega o no cuadra, la trae
de GitHub (con `GITHUB_REPO`) y, si tampoco, la compila. La deja en `/usr/local/semillero/releases/X.Y.Z` (montada en solo
lectura en la jail del servicio), para el servicio, copia la base de datos,
cambia el enlace `current`, arranca y espera hasta 60 s a que `/salud` (en
`ESCUCHA`) responda con la versión nueva. Si no, vuelve a la anterior con la
base de datos de antes.

## Qué hay aquí

| Fichero | Dónde corre |
|---|---|
| `freebsd/install.sh` | host, como root: instala el actualizador y escribe lo que queda a mano |
| `freebsd/actualizar.sh` | host: `/usr/local/sbin/semillero-actualizar` (cron) |
| `freebsd/recibir-precompilada.sh` | host: la única orden del usuario del buzón |
| `freebsd/{jail.conf,pf.conf}.plantilla` | lo que `install.sh` rellena para copiarlo a mano |
| `cross/lanzar.sh` | equipo de trabajo: manda una etiqueta al PC y lanza la compilación |
| `cross/compilar-freebsd.sh`, `cross/entregar.sh` | PC de compilación: compila, empaqueta y sube al buzón |
| `cross/semillero-cross.{service,timer}` | PC, opcional: compilar y entregar la última etiqueta cada 15 min |
| `lib/` | funciones de sh compartidas |
| `config.example` | configuración del equipo de trabajo; cópiala a `config.local` (git la ignora) |
| `pruebas/` | pruebas sin servidor |

## Configuración del host

`/usr/local/etc/semillero-actualizar.conf` (root, 0600). La primera vez
`install.sh` escribe una plantilla y para: rellénala y repítelo. Si falta un
valor obligatorio, `install.sh` y el actualizador dicen cuál y no hacen nada.

| Clave | Para qué | Por defecto |
|---|---|---|
| `REPO_URL` | repositorio git con las etiquetas, p. ej. `ssh://git@192.0.2.20:2200/usuario/semillero.git` | obligatoria |
| `REPO_HOST_CLAVE` | línea de `known_hosts` del servidor git | se fija la primera |
| `ESCUCHA` | dirección:puerto de semillero en su jail, p. ej. `192.0.2.10:8090` o `[2001:db8::10]:8090` | obligatoria |
| `JAIL`, `JAIL_RAIZ` | jail del servicio y su raíz en el host | `semillero`, `/usr/local/jails/containers/<JAIL>` |
| `JAIL_COMPILAR`, `JAIL_COMPILAR_RAIZ` | jail de compilación y su raíz | `semillero-build`, ídem |
| `JAIL_COMPILAR_IP` | IP de la jail de compilación, p. ej. `192.0.2.30` | obligatoria |
| `REPO_IP`, `REPO_PUERTO` | servidor git de la red local al que llega la jail de compilación (pf); vacía si el repositorio está en Internet | vacía, `22` |
| `DNS_IP` | DNS de la jail de compilación, p. ej. `192.0.2.1` | obligatoria |
| `RED_LAN` | red local que la jail de compilación no alcanza, p. ej. `'{ 192.0.2.0/24 198.51.100.0/24 }'` | obligatoria |
| `INTERFAZ_EXT` | interfaz del `nat` de la jail de compilación | hueco por rellenar |
| `PRECOMPILADAS_ESPERA` | minutos de espera al buzón (vacía: no se mira) | `20` |
| `TRABAJOS_COMPILACION` | trabajos de cargo a la vez (vacía: uno por núcleo) | vacía |
| `GITHUB_REPO` | repositorio de GitHub `propietario/repositorio` del que traer la versión publicada, con su atestación (vacía: no se mira GitHub); necesita `gh` y `jq` | vacía |
| `USUARIO_COMPILAR`, `UID_COMPILAR` | usuario sin privilegios que compila | `smbuild`, `3170` |
| `USUARIO_BUZON`, `UID_BUZON` | usuario del buzón | `smentrega`, `3171` |

## Preparar el host (una vez)

Como root, desde una copia del repositorio:

1. `sh deploy/freebsd/install.sh`; rellena la configuración y repítelo.
   Instala el actualizador, el cron, newsyslog, `/usr/local/semillero/releases`,
   el punto de montaje en la jail y `/var/db/semillero-buzon`, y escribe ya
   rellenados los pasos siguientes.
2. **Jail de compilación**, sin VNET, en `JAIL_COMPILAR_IP`: créala como
   cualquier jail, añade el bloque de `/etc/jail.conf` que escribe
   `install.sh`, arráncala y repite `install.sh`. Pone el DNS, instala `git`,
   `binaryen` y `rustup`, crea `USUARIO_COMPILAR`, su clave de despliegue, el
   clon y `cargo-leptos`.
3. **pf**: copia a `/etc/pf.conf` las reglas que escribe `install.sh` (la
   `nat` antes de la primera `nat`; el filtro encima de `block all`) y
   comprueba con `pfctl -nf /etc/pf.conf`. La jail de compilación sale a
   Internet, pero de `RED_LAN` solo alcanza `REPO_IP:REPO_PUERTO` (si lo hay) y
   `DNS_IP`.
4. **Clave de despliegue**: da de alta la clave pública que escribe
   `install.sh` como clave de solo lectura del repositorio y repite
   `install.sh` si aún no había clon.
5. **Montaje**: la línea de `/etc/fstab.<JAIL>` y la orden `mount -t nullfs`
   que escribe `install.sh` (solo lectura).
6. **Buzón** (solo si usas el PC): las órdenes `pw` y el `authorized_keys`
   que escribe `install.sh`. El `authorized_keys` es de root y fuerza
   `recibir-precompilada.sh`, que solo guarda el paquete y `SHA256SUMS` (300 MB
   como mucho, las 3 versiones más recientes) y no ejecuta nada. Si `sshd`
   tiene `AllowUsers` o `AllowGroups`, añade el usuario del buzón.
7. **Servicio**: el `rc.d` de semillero en la jail arranca
   `/usr/local/semillero/current/semillero` escuchando en `ESCUCHA`. Publica
   una versión y pasa el actualizador a mano:

   ```sh
   semillero-actualizar --comprobar
   semillero-actualizar
   tail /var/log/semillero-update.log
   ```

Si cambia algo de `deploy/`, repite `install.sh`.

## PC de compilación (opcional)

En el equipo de trabajo, copia `deploy/config.example` a
`deploy/config.local` y pon `PC_COMPILAR_SSH` (cómo entras al PC) y
`PRECOMPILADAS_BUZON_SSH` (el usuario del buzón como lo ve el PC, p. ej.
`smentrega@servidor.example`).

En el PC (Linux con `rustup`, `podman`, `cross`, `rsync`, `file` y `flock`):

```sh
ssh-keygen -t ed25519 -N '' -C semillero-buzon -f ~/.ssh/semillero-buzon
cat ~/.ssh/semillero-buzon.pub     # para el authorized_keys del buzón
ssh-keyscan -t ed25519 servidor.example >>~/.ssh/known_hosts   # compara la huella con la del servidor
ssh -i ~/.ssh/semillero-buzon -o IdentitiesOnly=yes smentrega@servidor.example recibir
```

(`recibir` sin argumentos responde con el uso.) Prueba sin entregar nada:
`deploy/cross/lanzar.sh --prueba`. Para el temporizador opcional, copia
`cross/semillero-cross.{service,timer}` a `~/.config/systemd/user/`, escribe
`~/.config/semillero-cross.env` con `SEMILLERO_REPO_URL` y
`SEMILLERO_BUZON_SSH`, y `systemctl --user enable --now semillero-cross.timer`.

## Publicar una versión

1. Pasa las pruebas (`cargo test --workspace --features semillero/ssr` y
   `deploy/pruebas/`).
2. Sube `version` en `crates/semillero/Cargo.toml`, haz commit, etiqueta y
   súbelo: `git tag vX.Y.Z && git push && git push origin vX.Y.Z`.
3. Con el PC: `deploy/cross/lanzar.sh vX.Y.Z`. Se niega con cambios sin
   confirmar o si la etiqueta no es la versión de `Cargo.toml`. Si el PC no
   responde, sale con 3 y el servidor compila pasada la espera.

El servidor la instala en la siguiente pasada del cron.

## Versiones de GitHub (opcional)

Con `GITHUB_REPO=propietario/repositorio` (el repositorio público donde
`release.yml` publica las versiones, p. ej. `m4rz3r0/semillero`), el
actualizador puede instalar el paquete de FreeBSD de la release `vX.Y.Z`
en vez de compilarlo. Necesita en el host `pkg install gh jq`; `install.sh`
lo comprueba y, si falta, dice qué paquete y para. No hace falta sesión de
`gh` ni token.

Para una etiqueta nueva, en este orden:

1. **El buzón**, como siempre (si `PRECOMPILADAS_ESPERA` no está vacía y ya
   ha llegado).
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

   Luego, las comprobaciones del buzón (solo ficheros normales, una sola
   carpeta, un ejecutable de FreeBSD x86-64) y la web completa. No se mira
   que `COMMIT` sea el de la etiqueta: el repositorio público es una copia
   aplanada y sus commits son otros; lo que ata el paquete a la etiqueta es
   la atestación. De lo que trae solo se instalan `semillero`, `site/` y
   `COMMIT` (no `LICENSE`, los README ni `rc.d/`).
3. **Compilar aquí**, como siempre.

La release sale unos minutos después de la etiqueta: mientras espera al
buzón, el actualizador prueba GitHub en cada pasada. Sin PC de compilación,
pon `PRECOMPILADAS_ESPERA=30` para darle tiempo antes de compilar. Si
GitHub no la tiene aún, lo dice en el registro (`no se usa GitHub para
vX.Y.Z: …`); si el sha256 o la atestación no cuadran, es un `ERROR` (`versión
de GitHub de vX.Y.Z rechazada: …`), no se instala nada y sigue como sin
`GITHUB_REPO`. Si la versión de GitHub falla al desplegarse, el segundo
intento de esa etiqueta ya no la usa: compila aquí.

`gh` y `jq` corren como root, pero solo leen lo descargado (como `tar` y
`file` con el buzón), en una carpeta propia y con una casa vacía; nada de lo
descargado se ejecuta antes de comprobarlo.

## Qué ha pasado

- `/var/log/semillero-update.log`: versión nueva, espera, instalada o
  rechazada (y por qué), compilación, despliegue, vuelta atrás y errores.
- `/var/log/semillero-build.log`: la última compilación en el servidor.
- `semillero-actualizar --comprobar`: qué haría, sin cambiar nada (con
  `GITHUB_REPO`, también el repositorio y si están `gh` y `jq`).
- En el PC, `~/semillero-cross/compilacion.log` y `~/semillero-cross/salida/`.

Solo hay una compilación a la vez en el host: se toma el cerrojo
`/var/run/compilar.lock`, que otros proyectos pueden compartir; si está
cogido, se reintenta en la pasada siguiente sin contarlo como fallo. El
binario sale de la jail de compilación como un `tar` hecho dentro de ella:
el actualizador, que corre como root, nunca abre rutas de esa jail.

## A mano

- **Una rama:** `semillero-actualizar --ref RAMA` (o un commit) la compila en
  el servidor y la despliega como `releases/<versión>-<commit>`; la siguiente
  pasada vuelve a la última etiqueta.
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
- `deploy/pruebas/precompiladas.sh`: `lib/`, el buzón y los guiones del PC.
- `deploy/pruebas/sintaxis.sh`: `sh -n`/`bash -n` (y `shellcheck`, si está).
