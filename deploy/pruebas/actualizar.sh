#!/bin/sh
# Pruebas del actualizador del servidor (deploy/freebsd/actualizar.sh) en
# Linux, sin servidor y sin tocar el sistema: todo vive en una carpeta
# temporal (fuera del repositorio) que se borra al terminar.
#
# - Las rutas fijas del actualizador cuelgan de SEMILLERO_ACT_RAIZ (la
#   carpeta temporal), y sus órdenes salen de SEMILLERO_ACT_PATH, donde van
#   delante unas de mentira: lockf, jexec (con service y fetch de la jail del
#   servicio), sockstat, ps, sha256, ssh (el servidor git: apunta con qué
#   clave y qué known_hosts se conecta y sirve git-upload-pack del «origen»
#   de aquí) y cargo, rustc y una jail semillero-build que solo apuntan que
#   alguien las ha llamado: el servidor nunca compila.
# - Un «origen» git hace de servidor git: cada versión es un commit con su
#   crates/semillero/Cargo.toml y su etiqueta vX.Y.Z (anotada; alguna,
#   ligera). El actualizador lo mira con git ls-remote por ssh.
# - El runner de Forgejo Actions del PC se simula dejando el paquete en la
#   release de un Forgejo de mentira (un curl que sirve forja.example y
#   apunta sus argumentos y la configuración que le llega por -K -), y
#   GitHub, con un fetch y un gh de mentira.
# - El semillero de mentira (service de la jail) arranca la versión de
#   «current» y su /salud responde con la versión que dice su binario; un
#   binario «roto» no arranca y uno con «migra» toca la base de datos.
#
# Casos: (a) primera instalación desde la release de Forgejo, por ssh con
# la clave de despliegue y el known_hosts del host, y el token de Forgejo
# solo por la entrada estándar de curl; (b) la misma etiqueta otra vez no
# hace nada; (c) SHA256SUMS que falta (espera, una sola línea en el
# registro y sin contar fallo) o está mal hecho (dos líneas, otro nombre, no
# hexadecimal): rechazada y espera, sin contar fallo y diciéndolo una sola
# vez en dos pasadas; (d) sha256 malo; (e)
# paquetes peligrosos (ruta absoluta, «..», enlace simbólico, enlace duro,
# carpeta raíz equivocada); (f) /salud con otra versión: vuelta atrás con la
# base de datos de antes; (g) /salud que no responde dos veces: etiqueta
# fallida, y la tercera vez ni se mira Forgejo; (h) se conservan 3
# versiones y nunca se borra la desplegada; (n) sin versión precompilada:
# espera sin límite (también con la espera de la configuración antigua ya
# pasada) y la instala cuando llega; (q) precompilada de otro commit:
# rechazada y espera; etiqueta ligera; (s) repositorio inaccesible, clave
# del servidor git que no cuadra, sin known_hosts o con la clave legible por
# otros; (j) configuración escribible por otros: no se usa; (k) --comprobar
# no cambia nada; (m) argumentos de más y --ref, que ya no existe; (t) falta
# REPO_URL o ESCUCHA, o ESCUCHA mal escrita: error claro y no se hace nada;
# (u) /salud y el puerto se miran en la ESCUCHA de la configuración (también
# IPv6); (x) GITHUB_REPO: sin él no se llama a fetch ni a gh del host; con
# él, la versión de GitHub se instala con su atestación; un sha256 malo o
# una atestación que no cuadra no instalan nada y espera; sin release,
# espera (y lo dice una vez) hasta que sale; Forgejo va primero; el 2.º
# intento de una etiqueta vuelve a GitHub; --comprobar dice la fuente; un
# GITHUB_REPO no válido para todo; (l) install.sh en una carpeta: plantilla
# y parada sin configuración, parada si falta un valor o git, lo que
# escribe (la clave de despliegue y el known_hosts en el host, sin jail de
# compilación) y una instalación anterior con jail de compilación; (v)
# install.sh con otra jail; (w) repositorio por
# https: sin clave de despliegue; (y) install.sh con GITHUB_REPO: pide gh y
# jq y rechaza un valor no válido; (z) Forgejo: sin FORGEJO_URL ni
# FORGEJO_REPO no se mira (lo dice una vez); un token que otros pueden leer
# no se usa ni se manda; install.sh con Forgejo pide curl, crea la carpeta
# del token (0700) y dice cómo ponerlo. En todos, cargo, rustc y la jail de
# compilación no se llaman nunca. Todas las direcciones son de
# documentación.
#
# Uso: deploy/pruebas/actualizar.sh (desde cualquier sitio; necesita git,
# python3, tar, file, sha256sum y jq).

set -eu

DEPLOY=$(cd "$(dirname "$0")/.." && pwd)
ACT=$DEPLOY/freebsd/actualizar.sh
OBJ=x86_64-unknown-freebsd

for orden in git python3 sha256sum tar file jq; do
	command -v "$orden" >/dev/null 2>&1 || {
		echo "FALLO: falta $orden" >&2
		exit 1
	}
done
[ -f "$ACT" ] || {
	echo "FALLO: falta deploy/freebsd/actualizar.sh" >&2
	exit 1
}
# El actualizador corre con el sh de aquí (dash en Debian y Ubuntu): no
# necesita nada que no sea POSIX.
SH_ACT='sh'

T=$(mktemp -d)
limpiar()
{
	# SEMILLERO_PRUEBA_CONSERVAR: deja la carpeta para mirarla a mano.
	if [ -n "${SEMILLERO_PRUEBA_CONSERVAR:-}" ]; then
		echo "carpeta de la prueba: $T" >&2
		return 0
	fi
	chmod -R u+w "$T" 2>/dev/null || true
	rm -rf "$T"
}
trap limpiar EXIT
trap 'exit 143' INT TERM

HOME=$T/casa
GIT_CONFIG_NOSYSTEM=1
GIT_AUTHOR_NAME=prueba
GIT_AUTHOR_EMAIL=prueba
GIT_COMMITTER_NAME=prueba
GIT_COMMITTER_EMAIL=prueba
export HOME GIT_CONFIG_NOSYSTEM GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL

R=$T/raiz                                  # SEMILLERO_ACT_RAIZ
F=$T/falsos                                # órdenes de mentira
JR=$R/usr/local/jails/containers/semillero # raíz de la jail del servicio
ORIGEN=$T/origen                           # el repositorio del servidor git
REL=$R/usr/local/semillero/releases        # versiones en el host
CUR=$JR/usr/local/semillero/current
DB=$JR/var/db/semillero
LOG=$R/var/log/semillero-update.log
ESTADO=$R/var/db/semillero-actualizar
FJ=$T/forgejo                          # releases del Forgejo de mentira
LIBEXEC=$R/usr/local/libexec/semillero-actualizar
CONF=$R/usr/local/etc/semillero-actualizar.conf
# Clave de despliegue y known_hosts del host (los valores por defecto).
# Las claves de mentira se escriben con el tipo aparte («''» o $TIPO_SSH)
# para que el escáner de datos privados no las tome por claves de verdad.
TIPO_SSH=ssh-ed25519
LLAVES=$R/usr/local/etc/semillero-actualizar
CLAVE=$LLAVES/id_ed25519
HOSTS=$LLAVES/known_hosts
# El token de lectura de Forgejo (FORGEJO_TOKEN_FICHERO por defecto).
TOKEN=$LLAVES/forgejo-token
TOKEN_VALOR=token-de-prueba-forgejo
mkdir -p "$HOME" "$F" "$T/tmpdir" "$T/fuera" "$T/build" \
	"$R/usr/local/etc" "$REL" "$JR/usr/local/semillero" "$DB" "$FJ" "$LIBEXEC" \
	"$R/var/log" "$R/var/run" "$R/var/db" "$LLAVES"
# El montaje nullfs de releases en la jail: aquí, un enlace.
ln -s "$REL" "$JR/usr/local/semillero/releases"
printf 'base de datos v0\n' >"$DB/semillero.db"
cp "$DEPLOY/lib/git.sh" "$DEPLOY/lib/precompiladas.sh" "$LIBEXEC/"
chmod 644 "$LIBEXEC"/*
chmod 700 "$LLAVES"
printf 'clave privada de mentira\n' >"$CLAVE"
chmod 600 "$CLAVE"
printf '[git.example]:2200 ssh-ed25519'' AAAAC3NzaC1lZDI1NTE5AAAAIPRUEBA\n' >"$HOSTS"
chmod 644 "$HOSTS"
printf '%s\n' "$TOKEN_VALOR" >"$TOKEN"
chmod 600 "$TOKEN"

# Dónde escucha el semillero de mentira (lo miran fetch y sockstat de
# mentira) y lo que dice la configuración (ESCUCHA).
ESCUCHA=192.0.2.10:8090
echo "$ESCUCHA" >"$T/escucha"

# El repositorio por ssh, como en el servidor (el ssh de mentira sirve
# $ORIGEN).
URL=ssh://git@git.example:2200$ORIGEN

# La configuración, con las claves que ya tenía la de una instalación con
# jail de compilación (JAIL_COMPILAR, PRECOMPILADAS_ESPERA,
# TRABAJOS_COMPILACION, REPO_IP…) o con buzón (USUARIO_BUZON, UID_BUZON):
# el actualizador ya no las usa. Con ESCUCHA y el Forgejo de mentira. Sin
# CLAVE_DESPLIEGUE, HOSTS_CONOCIDOS ni FORGEJO_TOKEN_FICHERO: los valores
# por defecto. $1: REPO_URL; $2: líneas que se añaden al final.
escribir_conf()
{
	cat >"$CONF" <<-EOF
		REPO_URL=${1:-$URL}
		REPO_HOST_CLAVE=
		ESCUCHA=$ESCUCHA
		JAIL=semillero
		JAIL_RAIZ=/usr/local/jails/containers/semillero
		JAIL_COMPILAR=semillero-build
		JAIL_COMPILAR_RAIZ=/usr/local/jails/containers/semillero-build
		JAIL_COMPILAR_IP=192.0.2.30
		PRECOMPILADAS_ESPERA=20
		TRABAJOS_COMPILACION=2
		REPO_IP=192.0.2.20
		REPO_PUERTO=2200
		DNS_IP=192.0.2.1
		RED_LAN='{ 192.0.2.0/24 198.51.100.0/24 }'
		INTERFAZ_EXT=em0
		USUARIO_BUZON=smentrega
		UID_BUZON=3171
		FORGEJO_URL=$FORGEJO_URL
		FORGEJO_REPO=$FORGEJO_REPO
		${2:-}
	EOF
	chmod 600 "$CONF"
}
FORGEJO_URL=https://forja.example
FORGEJO_REPO=ejemplo/semillero
escribir_conf

# Cabecera ELF mínima de x86-64 con el ABI $1 (9: FreeBSD, 0: Linux/SYSV);
# file la reconoce igual que un binario de verdad.
elf()
{
	printf '\177ELF\002\001\001'
	printf "\\$(printf '%03o' "$1")"
	printf '\000\000\000\000\000\000\000\000\002\000\076\000\001\000\000\000'
	head -c 40 /dev/zero
}
elf 9 >"$T/elf-freebsd"
elf 0 >"$T/elf-linux"

# --- el repositorio -----------------------------------------------------------
git init -q -b main "$ORIGEN"
mkdir -p "$ORIGEN/crates/semillero"
printf '[package]\nname = "semillero"\nversion = "0.0.0"\n' >"$ORIGEN/crates/semillero/Cargo.toml"
git -C "$ORIGEN" add -A
git -C "$ORIGEN" commit -q -m inicio

# Una versión nueva en el origen: commit con la versión $1 en Cargo.toml y
# la etiqueta anotada v$1 ($2 = ligera: etiqueta ligera).
publicar()
{
	printf '[package]\nname = "semillero"\nversion = "%s"\n\n[dependencies]\n' "$1" \
		>"$ORIGEN/crates/semillero/Cargo.toml"
	git -C "$ORIGEN" add -A
	git -C "$ORIGEN" commit -q -m "v$1"
	if [ "${2:-}" = ligera ]; then
		git -C "$ORIGEN" tag "v$1"
	else
		git -C "$ORIGEN" tag -a -m "v$1" "v$1"
	fi
}

commit_de()
{
	git -C "$ORIGEN" rev-parse "v$1^{commit}"
}

# Binario de mentira de la variante $2 para la versión $1: buena, migra
# (arranca bien y migra la base de datos), roto (no arranca), otra-version
# (su /salud dice 9.9.9 y migra) o linux (ELF de Linux).
binario()
{
	case $2 in
	linux) cat "$T/elf-linux" ;;
	*) cat "$T/elf-freebsd" ;;
	esac
	case $2 in
	buena | linux) printf '\nsalud-version=%s\n' "$1" ;;
	migra) printf '\nsalud-version=%s\nmigra\n' "$1" ;;
	roto) printf '\nroto\n' ;;
	otra-version) printf '\nsalud-version=9.9.9\nmigra\n' ;;
	esac
}

# El runner del PC publica la versión $1 como release v$1 de Forgejo (como
# deploy/cross/publicar-forgejo.sh). $2: variante del binario; $3: COMMIT
# (por defecto, el de la etiqueta).
publicar_forgejo()
{
	_v=$1
	_b=$T/build/$_v
	_r=$_b/semillero-$_v-$OBJ
	rm -rf "$_b" "${FJ:?}/$_v"
	mkdir -p "$_r/site/pkg" "$FJ/$_v"
	binario "$_v" "${2:-buena}" >"$_r/semillero"
	chmod 755 "$_r/semillero"
	printf 'js: abc\nwasm: abc\ncss: abc\n' >"$_r/site/hash.txt"
	for e in js wasm css; do printf '%s %s\n' "$e" "$_v" >"$_r/site/pkg/semillero.abc.$e"; done
	printf '%s\n' "${3:-$(commit_de "$_v")}" >"$_r/COMMIT"
	printf 'licencia\n' >"$_r/LICENSE"
	tar -C "$_b" --sort=name --owner=0 --group=0 --numeric-owner \
		-czf "$FJ/$_v/semillero-$_v-$OBJ.tar.gz" "semillero-$_v-$OBJ"
	(cd "$FJ/$_v" && sha256sum "semillero-$_v-$OBJ.tar.gz" >SHA256SUMS)
}

# Publica en Forgejo la versión $1 con un paquete peligroso (SHA256SUMS
# válido). $2: absoluta, puntos, enlace, duro o raiz.
publicar_forgejo_malo()
{
	rm -rf "${FJ:?}/$1"
	mkdir -p "$FJ/$1"
	python3 -I - "$FJ/$1/semillero-$1-$OBJ.tar.gz" "$1" "$2" "$T/fuera" "$T/elf-freebsd" <<'PY'
import io, sys, tarfile
salida, v, tipo, fuera, elf = sys.argv[1:]
raiz = "otro-" + v if tipo == "raiz" else "semillero-%s-x86_64-unknown-freebsd" % v
def carpeta(t, nombre):
    d = tarfile.TarInfo(nombre)
    d.type = tarfile.DIRTYPE
    d.mode = 0o755
    t.addfile(d)
def fichero(t, nombre, datos=b"x\n", modo=0o644):
    i = tarfile.TarInfo(nombre)
    i.size = len(datos)
    i.mode = modo
    t.addfile(i, io.BytesIO(datos))
with tarfile.open(salida, "w:gz", format=tarfile.GNU_FORMAT) as t:
    carpeta(t, raiz)
    carpeta(t, raiz + "/site")
    fichero(t, raiz + "/semillero", open(elf, "rb").read() + ("\nsalud-version=%s\n" % v).encode(), 0o755)
    fichero(t, raiz + "/COMMIT", b"0123456789abcdef0123456789abcdef01234567\n")
    if tipo == "absoluta":
        fichero(t, fuera + "/escape-absoluta")
    elif tipo == "puntos":
        fichero(t, raiz + "/../../../../../../../escape-puntos")
    elif tipo == "enlace":
        e = tarfile.TarInfo(raiz + "/lnk")
        e.type = tarfile.SYMTYPE
        e.linkname = fuera
        t.addfile(e)
        fichero(t, raiz + "/lnk/escape-enlace")
    elif tipo == "duro":
        e = tarfile.TarInfo(raiz + "/site/duro")
        e.type = tarfile.LNKTYPE
        e.linkname = raiz + "/semillero"
        t.addfile(e)
PY
	(cd "$FJ/$1" && sha256sum "semillero-$1-$OBJ.tar.gz" >SHA256SUMS)
}

# --- órdenes de mentira -----------------------------------------------------
cat >"$F/lockf" <<EOF
#!/bin/sh
# lockf [-s] [-t N] fichero orden…: aquí no bloquea nada. Con un descriptor
# (el cerrojo de compilación de antes) lo apunta: no debe pasar.
while :; do
	case \$1 in
	-s | -k | -n | -w) shift ;;
	-t) shift 2 ;;
	*) break ;;
	esac
done
if [ "\$#" -eq 1 ]; then
	echo "descriptor \$1" >>"$T/compilador.log"
	exit 2
fi
shift
# El actualizador se relanza con /bin/sh bajo el cerrojo: aquí, con el sh
# de las pruebas.
if [ "\$1" = /bin/sh ]; then
	shift
	exec $SH_ACT "\$@"
fi
exec "\$@"
EOF

cat >"$F/sha256" <<'EOF'
#!/bin/sh
# sha256 de FreeBSD: -q fichero imprime la suma.
case $1 in
-q) sha256sum <"$2" | cut -c1-64 ;;
*) exit 2 ;;
esac
EOF

# Compiladores: el servidor nunca compila; si alguien los llama, queda
# apuntado en compilador.log (y la prueba falla al final).
for c in cargo rustc rustup cargo-leptos; do
	cat >"$F/$c" <<EOF
#!/bin/sh
echo "$c \$*" >>"$T/compilador.log"
exit 1
EOF
done

# jexec [-l] [-U usuario] jail orden…: en semillero, service y fetch van a los
# de mentira y /bin/sh corre aquí con las rutas absolutas de sus argumentos
# bajo la raíz de la jail. Cualquier otra jail (la de compilación) queda
# apuntada en compilador.log y falla.
cat >"$F/jexec" <<EOF
#!/bin/sh
while :; do
	case \$1 in
	-l) shift ;;
	-U | -u) shift 2 ;;
	*) break ;;
	esac
done
jail=\$1
shift
printf '%s %s\n' "\$jail" "\$1" >>"$T/jexec.log"
orden=\$1
shift
case \$jail in
semillero) ;;
*)
	echo "jexec \$jail \$orden" >>"$T/compilador.log"
	exit 1
	;;
esac
case \$orden in
/usr/sbin/service) exec "$F/servicio-jail" "\$@" ;;
/usr/bin/fetch) exec "$F/fetch-jail" "\$@" ;;
/bin/sh)
	[ "\$1" = -c ] || exit 2
	guion=\$2
	nombre=\$3
	shift 3
	for a in "\$@"; do
		shift
		case \$a in
		/*) set -- "\$@" "$JR\$a" ;;
		*) set -- "\$@" "\$a" ;;
		esac
	done
	exec /bin/sh -c "\$guion" "\$nombre" "\$@"
	;;
*) echo "jexec de mentira: orden \$orden" >&2; exit 2 ;;
esac
EOF

# ssh: el servidor git git.example. Git lo llama con GIT_SSH_COMMAND (antes,
# con -G, para saber si es OpenSSH). Apunta los argumentos; exige la clave
# (-i) y un known_hosts (UserKnownHostsFile) con git.example, como un ssh
# con StrictHostKeyChecking=yes; con la marca clave-servidor-cambiada, la
# clave del servidor no cuadra. Luego corre aquí la orden remota
# (git-upload-pack '<ruta>').
cat >"$F/ssh" <<EOF
#!/bin/sh
for a; do
	if [ "\$a" = -G ]; then exit 0; fi
done
printf '%s\n' "\$*" >>"$T/ssh.log"
clave= hosts=
while [ "\$#" -gt 1 ]; do
	case \$1 in
	-i) clave=\$2; shift 2 ;;
	-o)
		case \$2 in UserKnownHostsFile=*) hosts=\${2#UserKnownHostsFile=} ;; esac
		shift 2
		;;
	-p | -F) shift 2 ;;
	*) shift ;;
	esac
done
[ -f "\$clave" ] || { echo "Warning: Identity file \$clave not accessible" >&2; exit 255; }
if [ ! -f "\$hosts" ] || ! grep -q git.example "\$hosts" || [ -e "$T/clave-servidor-cambiada" ]; then
	echo "Host key verification failed." >&2
	exit 255
fi
exec /bin/sh -c "\$1"
EOF

cat >"$F/servicio-jail" <<EOF
#!/bin/sh
# service semillero start|stop: el semillero de mentira.
[ "\$1" = semillero ] || exit 1
case \$2 in
stop)
	echo stop >>"$T/servicio.log"
	[ -f "$T/en-marcha" ] || exit 1
	rm -f "$T/en-marcha"
	;;
start)
	if [ ! -L "$CUR" ]; then
		echo "start sin-current" >>"$T/servicio.log"
		exit 1
	fi
	dir=\$(realpath "$CUR")
	v=\${dir##*/}
	echo "start \$v" >>"$T/servicio.log"
	bin=\$dir/semillero
	[ -f "\$bin" ] || exit 1
	if grep -qax migra "\$bin"; then
		echo "migrada por \$v" >>"$DB/semillero.db"
	fi
	# Un binario roto: daemon arranca, pero el proceso muere enseguida.
	grep -qax roto "\$bin" && exit 0
	printf '%s\n%s\n' "/usr/local/semillero/releases/\$v/semillero" \
		"\$(sed -n 's/^salud-version=//p' "\$bin")" >"$T/en-marcha"
	;;
*) exit 2 ;;
esac
EOF

cat >"$F/fetch-jail" <<EOF
#!/bin/sh
# fetch -T 2 -qo - URL desde la jail: /salud del semillero de mentira.
for u; do :; done
printf '%s\n' "\$u" >>"$T/fetch.log"
[ "\$u" = "http://\$(cat "$T/escucha")/salud" ] || exit 1
[ -f "$T/en-marcha" ] || exit 1
printf '{"ok":true,"version":"%s"}' "\$(sed -n 2p "$T/en-marcha")"
EOF

cat >"$F/sockstat" <<EOF
#!/bin/sh
# sockstat -j semillero -l -p PUERTO -q (el puerto de \$T/escucha)
printf '%s\n' "\$*" >>"$T/sockstat.log"
e=\$(cat "$T/escucha")
case " \$* " in *" -j semillero "*" -p \${e##*:} "*) ;; *) exit 1 ;; esac
[ -f "$T/en-marcha" ] || exit 0
echo "semillero semillero 4242 3 tcp4 \$e *:*"
EOF

cat >"$F/ps" <<EOF
#!/bin/sh
# ps -ww -o command= -p 4242
case " \$* " in *" -p 4242"*) ;; *) exit 1 ;; esac
[ -f "$T/en-marcha" ] || exit 1
sed -n 1p "$T/en-marcha"
EOF

# fetch del host y gh: el GitHub de mentira (casos x).
GH=$T/github
mkdir -p "$GH"
cat >"$F/fetch" <<EOF
#!/bin/sh
for u; do :; done
printf '%s\n' "\$u" >>"$GH/fetch.log"
case \$u in
https://github.com/*) f="$GH/web/\${u#https://github.com/}" ;;
https://api.github.com/*) f="$GH/api/\${u#https://api.github.com/}" ;;
*) exit 1 ;;
esac
[ -f "\$f" ] || { echo "fetch: \$u: Not Found" >&2; exit 1; }
cat "\$f"
EOF
cat >"$F/gh" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >>"$GH/gh.log"
[ "\$1 \$2" = "attestation verify" ] || exit 2
f=\$3
shift 3
b= r= ref= w=
while [ \$# -gt 0 ]; do
	case \$1 in
	--bundle) b=\$2 ;;
	--repo) r=\$2 ;;
	--source-ref) ref=\$2 ;;
	--signer-workflow) w=\$2 ;;
	*) exit 2 ;;
	esac
	shift 2
done
d=\$(sha256sum <"\$f" | cut -c1-64)
grep -qxF "{\"falsa\":\"\$d \$r \$ref \$w\"}" "\$b" && exit 0
echo "Error: verification failed" >&2
exit 1
EOF
# curl del host: el Forgejo de mentira (https://forja.example, repositorio
# ejemplo/semillero). Sirve la release vX.Y.Z si existe $FJ/X.Y.Z (la API
# da 200 o 404) y sus ficheros. Apunta los argumentos en curl.log y lo que
# le llega por «-K -» en curl-config.log; sin la cabecera del token por ahí,
# responde 401.
{
	printf '#!/bin/sh\nT=%s\nFJ=%s\nTOKEN_VALOR=%s\n' "$T" "$FJ" "$TOKEN_VALOR"
	cat <<'EOF'
printf '%s\n' "$*" >>"$T/curl.log"
cfg= salida= formato= url= falla=
while [ "$#" -gt 0 ]; do
	case $1 in
	-K)
		if [ "$2" = - ]; then cfg=$(cat); fi
		shift 2
		;;
	-o) salida=$2; shift 2 ;;
	-w) formato=$2; shift 2 ;;
	--fail | --fail-with-body | -f) falla=1; shift ;;
	--connect-timeout | --max-time | --max-filesize | --proto | -X | -H | -F | --data-binary) shift 2 ;;
	-*) shift ;;
	*) url=$1; shift ;;
	esac
done
printf '%s\n' "$cfg" >>"$T/curl-config.log"
# responder CÓDIGO [FICHERO]
responder()
{
	if [ -n "${2:-}" ]; then
		if [ -z "$salida" ] || [ "$salida" = - ]; then cat "$2"; else cat "$2" >"$salida"; fi
	fi
	if [ -n "$formato" ]; then printf '%s' "$1"; fi
	case $1 in
	2??) exit 0 ;;
	esac
	if [ -n "$falla" ]; then
		echo "curl: (22) The requested URL returned error: $1" >&2
		exit 22
	fi
	exit 0
}
printf '{"message":"no"}\n' >"$T/curl-cuerpo"
if [ "$cfg" != "header = \"Authorization: token $TOKEN_VALOR\"" ]; then
	responder 401 "$T/curl-cuerpo"
fi
case $url in
https://forja.example/api/v1/repos/ejemplo/semillero/releases/tags/v*)
	v=${url##*/tags/v}
	if [ -d "$FJ/$v" ]; then
		printf '{"id":7,"tag_name":"v%s"}\n' "$v" >"$T/curl-cuerpo"
		responder 200 "$T/curl-cuerpo"
	fi
	responder 404 "$T/curl-cuerpo"
	;;
https://forja.example/ejemplo/semillero/releases/download/v*/*)
	nombre=${url##*/}
	resto=${url%/*}
	v=${resto##*/v}
	if [ -f "$FJ/$v/$nombre" ]; then responder 200 "$FJ/$v/$nombre"; fi
	responder 404 "$T/curl-cuerpo"
	;;
esac
echo "curl: (6) Could not resolve host" >&2
exit 6
EOF
} >"$F/curl"
chmod 755 "$F"/*

# --- utilidades -------------------------------------------------------------
fallos=0
casos=0
fallo()
{
	echo "FALLO ($caso): $*" >&2
	fallos=$((fallos + 1))
}
empieza()
{
	caso=$1
	casos=$((casos + 1))
}

# Pasa el actualizador; salida en $T/salida (y acumulada en $T/salidas).
actualizar()
{
	_rc=0
	SEMILLERO_ACT_RAIZ=$R \
		SEMILLERO_ACT_PATH=$F:/usr/local/bin:/usr/bin:/bin \
		SEMILLERO_ACT_ESPERA=3 \
		TMPDIR=$T/tmpdir \
		$SH_ACT "$ACT" "$@" >"$T/salida" 2>&1 || _rc=$?
	cat "$T/salida" >>"$T/salidas"
	_restos=$(find "$T/tmpdir" -mindepth 1 -maxdepth 1 -exec basename {} \; | tr '\n' ' ')
	if [ -n "$_restos" ]; then
		fallo "el actualizador ($*) deja ficheros en TMPDIR: $_restos"
		rm -rf "${T:?}/tmpdir"/* "${T:?}/tmpdir"/.[!.]* 2>/dev/null || true
	fi
	# RESTOS_PLANTADOS: la prueba dejó a propósito un resto de otra ejecución.
	if [ -z "${RESTOS_PLANTADOS:-}" ] && [ -n "$(find "$REL" -mindepth 1 -maxdepth 1 -name '.*')" ]; then
		fallo "el actualizador ($*) deja restos ocultos en releases"
	fi
	return "$_rc"
}

actual()
{
	readlink "$CUR" 2>/dev/null || echo ninguna
}

versiones()
{
	find "$REL" -mindepth 1 -maxdepth 1 -exec basename {} \; | sort | tr '\n' ' '
}

en_marcha()
{
	sed -n 2p "$T/en-marcha" 2>/dev/null || echo parado
}

lineas()
{
	if [ -f "$1" ]; then wc -l <"$1" | tr -d ' '; else echo 0; fi
}

# Recuerda cuántas líneas tienen los registros, para ver lo que añade una
# ejecución (nuevo LOG da las líneas nuevas de LOG).
marcar()
{
	m_log=$(lineas "$LOG")
	m_serv=$(lineas "$T/servicio.log")
	m_jexec=$(lineas "$T/jexec.log")
	m_ssh=$(lineas "$T/ssh.log")
	m_curl=$(lineas "$T/curl.log")
}
nuevo()
{
	case $1 in
	log) tail -n +$((m_log + 1)) "$LOG" 2>/dev/null || true ;;
	serv) tail -n +$((m_serv + 1)) "$T/servicio.log" 2>/dev/null || true ;;
	jexec) tail -n +$((m_jexec + 1)) "$T/jexec.log" 2>/dev/null || true ;;
	ssh) tail -n +$((m_ssh + 1)) "$T/ssh.log" 2>/dev/null || true ;;
	curl) tail -n +$((m_curl + 1)) "$T/curl.log" 2>/dev/null || true ;;
	esac
}
# Nadie ha llamado a cargo, rustc ni a la jail de compilación.
sin_compilar()
{
	[ ! -e "$T/compilador.log" ] || fallo "se llama a un compilador o a la jail de compilación: $(tr '\n' ' ' <"$T/compilador.log")"
}

# Sin cambios: current, versiones, servicio en marcha y sin llamadas a service.
sin_cambios()
{
	[ "$(actual)" = "$1" ] || fallo "current ha cambiado ($(actual), se esperaba $1)"
	[ "$(versiones)" = "$2" ] || fallo "releases ha cambiado ($(versiones), se esperaba $2)"
	[ -z "$(nuevo serv)" ] || fallo "ha llamado a service: $(nuevo serv | tr '\n' ' ')"
	[ "$(en_marcha)" = "${3:-$(basename "$1")}" ] || fallo "el semillero en marcha es $(en_marcha)"
}

# --- (a) primera instalación ------------------------------------------------
empieza "a: primera instalación desde Forgejo"
publicar 1.0.0
publicar_forgejo 1.0.0
cp "$DB/semillero.db" "$T/db-v0"
marcar
if ! actualizar; then
	cat "$T/salida" >&2
	fallo "falla con una versión buena"
fi
[ "$(actual)" = releases/1.0.0 ] || fallo "current no apunta a releases/1.0.0 ($(actual))"
[ "$(versiones)" = "1.0.0 " ] || fallo "releases tiene $(versiones)"
[ "$(en_marcha)" = 1.0.0 ] || fallo "en marcha: $(en_marcha)"
[ "$(nuevo serv | tr '\n' ' ')" = "stop start 1.0.0 " ] ||
	fallo "llamadas a service: $(nuevo serv | tr '\n' ' ')"
cmp -s "$T/db-v0" "$DB/antes-de-actualizar/semillero.db" ||
	fallo "no copia la base de datos a antes-de-actualizar"
d=$REL/1.0.0
[ -f "$d/semillero" ] && [ -x "$d/semillero" ] || fallo "falta el binario o no es ejecutable"
[ -f "$d/site/pkg/semillero.abc.css" ] && [ -f "$d/site/hash.txt" ] || fallo "falta site/"
[ "$(cat "$d/COMMIT" 2>/dev/null)" = "$(commit_de 1.0.0)" ] || fallo "COMMIT no es el de la etiqueta"
[ "$(find "$d" -mindepth 1 -maxdepth 1 -printf '%f\n' | sort | tr '\n' ' ')" = "COMMIT semillero site " ] ||
	fallo "la versión no tiene solo semillero, site/ y COMMIT"
[ "$(stat -c %a "$d" "$d/site" "$d/semillero" "$d/COMMIT" "$d/site/hash.txt" 2>/dev/null | tr '\n' ' ')" = "755 755 755 644 644 " ] ||
	fallo "permisos de la versión: $(stat -c '%n %a' "$d" "$d/semillero" "$d/COMMIT" | tr '\n' ' ')"
nuevo log | grep -q 'instalada la versión de Forgejo de v1.0.0 (ejemplo/semillero' || fallo "el registro no dice que instala la de Forgejo: $(nuevo log)"
nuevo log | grep -q 'desplegada 1.0.0' || fallo "el registro no dice «desplegada 1.0.0»"
[ ! -e "$ESTADO/intentos" ] && [ ! -e "$ESTADO/fallida" ] || fallo "deja intentos o fallida"
# Las etiquetas, con git ls-remote por ssh desde el host: la clave de
# despliegue, el known_hosts fijado y sin preguntar nada.
[ -n "$(nuevo ssh)" ] || fallo "no mira el repositorio por ssh"
for o in "-i $CLAVE" "-o IdentitiesOnly=yes" "-o StrictHostKeyChecking=yes" \
	"-o UserKnownHostsFile=$HOSTS" "-o BatchMode=yes"; do
	nuevo ssh | grep -qF -- "$o" || fallo "ssh sin «$o»: $(nuevo ssh)"
done
nuevo ssh | grep -q "git-upload-pack '$ORIGEN'" || fallo "no pide la lista al repositorio: $(nuevo ssh)"
[ -z "$(nuevo jexec | grep -v '^semillero ')" ] || fallo "entra en otra jail: $(nuevo jexec)"
# Forgejo: la release de la etiqueta por la API y sus ficheros con los
# nombres de siempre; el token, solo por la entrada estándar de curl.
nuevo curl | grep -q ' https://forja.example/api/v1/repos/ejemplo/semillero/releases/tags/v1.0.0$' ||
	fallo "no pregunta por la release de v1.0.0: $(nuevo curl)"
for f in SHA256SUMS "semillero-1.0.0-$OBJ.tar.gz"; do
	nuevo curl | grep -q " https://forja.example/ejemplo/semillero/releases/download/v1.0.0/$f\$" ||
		fallo "no descarga $f de la release: $(nuevo curl)"
done
grep -qF "$TOKEN_VALOR" "$T/curl.log" && fallo "el token va en los argumentos de curl"
grep -qxF "header = \"Authorization: token $TOKEN_VALOR\"" "$T/curl-config.log" ||
	fallo "el token no llega por la entrada estándar de curl: $(sort -u "$T/curl-config.log")"
grep -q 'max-time' "$T/curl.log" || fallo "curl sin tiempo máximo"
sin_compilar

# --- (b) la misma etiqueta otra vez -----------------------------------------
empieza "b: misma etiqueta"
marcar
if ! actualizar; then
	cat "$T/salida" >&2
	fallo "falla sin nada nuevo"
fi
sin_cambios releases/1.0.0 "1.0.0 "
[ -z "$(nuevo log)" ] || fallo "escribe en el registro sin nada nuevo: $(nuevo log)"
[ ! -s "$T/salida" ] || fallo "escribe algo sin nada nuevo: $(cat "$T/salida")"

# --- (c) SHA256SUMS que falta o está mal hecho -----------------------------
n=1
for mal in falta dos-lineas otro-nombre no-hex; do
	empieza "c: SHA256SUMS $mal"
	v=1.0.$n
	n=$((n + 1))
	publicar "$v"
	publicar_forgejo "$v"
	s=$FJ/$v/SHA256SUMS
	a=semillero-$v-$OBJ.tar.gz
	case $mal in
	falta) rm -f "$s" ;;
	dos-lineas) printf '%s  otro.tar.gz\n' "$(sed -n 's/ .*//p' "$s")" >>"$s" ;;
	otro-nombre) sed -i "s/  $a\$/  semillero-9.9.9-$OBJ.tar.gz/" "$s" ;;
	no-hex) sed -i 's/^[0-9a-f]\{4\}/ZZZZ/' "$s" ;;
	esac
	marcar
	if [ "$mal" = falta ]; then
		# Sin SHA256SUMS no ha llegado: espera, sin contar nada.
		actualizar || fallo "falla mientras espera: $(cat "$T/salida")"
		sin_cambios releases/1.0.0 "1.0.0 "
		[ "$(nuevo log | grep -c "esperando la versión precompilada de $v")" -eq 1 ] ||
			fallo "el registro no dice una vez que espera: $(nuevo log)"
		[ ! -e "$ESTADO/intentos" ] || fallo "esperar cuenta como fallo de la etiqueta"
		# La pasada siguiente sigue esperando, sin repetirlo en el registro.
		marcar
		actualizar || fallo "falla mientras espera (2.ª pasada): $(cat "$T/salida")"
		[ -z "$(nuevo log)" ] || fallo "repite en el registro que espera: $(nuevo log)"
		sin_compilar
		continue
	fi
	if actualizar; then fallo "acepta un SHA256SUMS que $mal"; fi
	sin_cambios releases/1.0.0 "1.0.0 "
	nuevo log | grep -q 'rechazada: SHA256SUMS' || fallo "el registro no habla de SHA256SUMS: $(nuevo log)"
	nuevo log | grep -q "ERROR: versión de Forgejo de v$v rechazada: SHA256SUMS" || fallo "no dice que rechaza la de Forgejo: $(nuevo log)"
	nuevo log | grep -q "esperando la versión precompilada de $v" || fallo "no dice que sigue esperando: $(nuevo log)"
	[ ! -e "$ESTADO/intentos" ] || fallo "una precompilada rechazada cuenta como fallo de la etiqueta"
	# La misma release mala en la pasada siguiente: se vuelve a mirar, pero
	# el rechazo no se repite en el registro ni cuenta como fallo.
	marcar
	if actualizar; then fallo "acepta un SHA256SUMS que $mal (2.ª pasada)"; fi
	[ -n "$(nuevo curl)" ] || fallo "no vuelve a mirar Forgejo en la 2.ª pasada"
	[ -z "$(nuevo log)" ] || fallo "repite el rechazo en la 2.ª pasada: $(nuevo log)"
	[ ! -e "$ESTADO/intentos" ] || fallo "el rechazo cuenta como fallo en la 2.ª pasada"
	sin_cambios releases/1.0.0 "1.0.0 "
	sin_compilar
done

# --- (d) sha256 malo --------------------------------------------------------
empieza "d: sha256 malo"
publicar 1.1.1
publicar_forgejo 1.1.1
printf 'basura\n' >>"$FJ/1.1.1/semillero-1.1.1-$OBJ.tar.gz"
marcar
if actualizar; then fallo "acepta un sha256 que no cuadra"; fi
sin_cambios releases/1.0.0 "1.0.0 "
nuevo log | grep -q 'rechazada: el sha256' || fallo "el registro no habla del sha256: $(nuevo log)"
[ ! -e "$ESTADO/intentos" ] || fallo "un sha256 malo cuenta como fallo de la etiqueta"

# --- (e) paquetes peligrosos ------------------------------------------------
n=2
for tipo in absoluta puntos enlace duro raiz; do
	empieza "e: paquete con $tipo"
	v=1.1.$n
	n=$((n + 1))
	publicar "$v"
	publicar_forgejo_malo "$v" "$tipo"
	marcar
	if actualizar; then fallo "acepta un paquete con $tipo"; fi
	sin_cambios releases/1.0.0 "1.0.0 "
	nuevo log | grep -q 'rechazada' || fallo "el registro no da el rechazo: $(nuevo log)"
done
if [ -n "$(find "$T" -name 'escape-*' -print)" ]; then
	caso="e"
	fallo "un paquete peligroso ha escrito fuera: $(find "$T" -name 'escape-*')"
fi
sin_compilar

# --- (f) /salud con otra versión: vuelta atrás ------------------------------
empieza "f: /salud con otra versión"
publicar 1.2.0
publicar_forgejo 1.2.0 otra-version
cp "$DB/semillero.db" "$T/db-antes"
marcar
if actualizar; then fallo "acepta una versión cuya /salud dice otra"; fi
[ "$(actual)" = releases/1.0.0 ] || fallo "no vuelve a 1.0.0 ($(actual))"
[ "$(versiones)" = "1.0.0 " ] || fallo "no borra la versión fallida ($(versiones))"
[ "$(en_marcha)" = 1.0.0 ] || fallo "en marcha: $(en_marcha)"
[ "$(nuevo serv | tr '\n' ' ')" = "stop start 1.2.0 stop start 1.0.0 " ] ||
	fallo "llamadas a service: $(nuevo serv | tr '\n' ' ')"
grep -q 'migrada por 1.2.0' "$DB/antes-de-actualizar/semillero.db" &&
	fallo "la copia de la base de datos se hizo tarde"
cmp -s "$T/db-antes" "$DB/semillero.db" || fallo "no restaura la base de datos de antes"
nuevo log | grep -q 'vuelta atrás a 1.0.0' || fallo "el registro no dice la vuelta atrás"
nuevo log | grep -q 'borrada la versión fallida 1.2.0' || fallo "el registro no dice que borra 1.2.0"
[ "$(cat "$ESTADO/intentos" 2>/dev/null | tr '\n' ' ')" = "v1.2.0 1 " ] ||
	fallo "intentos no cuenta el fallo de v1.2.0"

# --- (g) /salud que no responde, dos veces ----------------------------------
empieza "g: etiqueta fallida"
publicar 1.3.0
publicar_forgejo 1.3.0 roto
marcar
if actualizar; then fallo "acepta una versión que no arranca"; fi
[ "$(actual)" = releases/1.0.0 ] && [ "$(en_marcha)" = 1.0.0 ] || fallo "no vuelve a 1.0.0 (1.ª vez)"
[ "$(cat "$ESTADO/intentos" 2>/dev/null | tr '\n' ' ')" = "v1.3.0 1 " ] || fallo "intentos tras el 1.er fallo"
[ ! -e "$ESTADO/fallida" ] || fallo "fallida tras un solo fallo"
# La release sigue en Forgejo: el 2.º intento la vuelve a traer.
marcar
if actualizar; then fallo "acepta una versión que no arranca (2.ª vez)"; fi
nuevo curl | grep -q 'releases/download/v1.3.0/' || fallo "no usa Forgejo en el 2.º intento: $(nuevo curl)"
[ "$(actual)" = releases/1.0.0 ] && [ "$(en_marcha)" = 1.0.0 ] || fallo "no vuelve a 1.0.0 (2.ª vez)"
[ "$(cat "$ESTADO/fallida" 2>/dev/null)" = v1.3.0 ] || fallo "fallida no es v1.3.0"
[ ! -e "$ESTADO/intentos" ] || fallo "deja intentos con la etiqueta ya fallida"
nuevo log | grep -q 'v1.3.0 ha fallado 2 veces' || fallo "el registro no dice que v1.3.0 queda fallida"
marcar
if ! actualizar; then fallo "con la etiqueta fallida no sale bien"; fi
sin_cambios releases/1.0.0 "1.0.0 "
[ -z "$(nuevo curl)" ] || fallo "mira Forgejo con una etiqueta fallida: $(nuevo curl)"
[ -z "$(nuevo log)" ] || fallo "escribe en el registro con la etiqueta fallida: $(nuevo log)"

# --- (h) se conservan 3 y nunca la desplegada -------------------------------
empieza "h: se conservan 3"
for v in 1.4.0 1.5.0 1.6.0; do
	publicar "$v"
	publicar_forgejo "$v" migra
	actualizar || fallo "no despliega $v: $(cat "$T/salida")"
	sleep 0.05
done
[ "$(actual)" = releases/1.6.0 ] && [ "$(en_marcha)" = 1.6.0 ] || fallo "no queda 1.6.0 desplegada"
[ "$(versiones)" = "1.4.0 1.5.0 1.6.0 " ] || fallo "no quedan las 3 últimas ($(versiones))"
[ ! -e "$ESTADO/fallida" ] && [ ! -e "$ESTADO/intentos" ] || fallo "un despliegue bueno no limpia fallida/intentos"
grep -q 'borrada la versión antigua 1.0.0' "$LOG" || fallo "el registro no dice que borra 1.0.0"
# Tres versiones más nuevas que la que se va a desplegar (por fecha): la
# desplegada queda la cuarta, pero no se borra.
for v in 9.0.0 9.0.1 9.0.2; do
	mkdir -p "$REL/$v"
	touch -t 209901010000 "$REL/$v"
done
publicar 1.7.0
publicar_forgejo 1.7.0
actualizar || fallo "no despliega 1.7.0: $(cat "$T/salida")"
[ "$(actual)" = releases/1.7.0 ] || fallo "current no es 1.7.0"
[ "$(versiones)" = "1.7.0 9.0.0 9.0.1 9.0.2 " ] || fallo "borra la desplegada o no poda ($(versiones))"
rm -rf "${REL:?}"/9.0.*

# --- (n) sin precompilada: espera sin límite ---------------------------------
empieza "n: sin precompilada se espera, sin compilar"
publicar 1.7.1
marcar
actualizar || fallo "falla mientras espera: $(cat "$T/salida")"
sin_cambios releases/1.7.0 "1.7.0 "
[ "$(nuevo log | grep -c 'esperando la versión precompilada de 1.7.1')" -eq 1 ] || fallo "no dice una vez que espera: $(nuevo log)"
# La espera de la configuración de antes (PRECOMPILADAS_ESPERA=20), ya
# pasada: sigue esperando, sin compilar y sin repetirlo.
echo "v1.7.1 1" >"$ESTADO/precompilada"
for pasada in 2 3; do
	marcar
	actualizar || fallo "falla en la pasada $pasada de la espera: $(cat "$T/salida")"
	sin_cambios releases/1.7.0 "1.7.0 "
	[ -z "$(nuevo log)" ] || fallo "la pasada $pasada de la espera escribe en el registro: $(nuevo log)"
done
[ ! -e "$ESTADO/intentos" ] && [ ! -e "$ESTADO/fallida" ] || fallo "esperar cuenta como fallo"
sin_compilar
# Llega: se instala en la pasada siguiente.
publicar_forgejo 1.7.1
marcar
actualizar || fallo "no despliega 1.7.1 cuando llega: $(cat "$T/salida")"
[ "$(actual)" = releases/1.7.1 ] && [ "$(en_marcha)" = 1.7.1 ] || fallo "no despliega 1.7.1 ($(actual))"
[ ! -e "$ESTADO/precompilada" ] || fallo "un despliegue bueno no limpia el estado de la espera antigua"
# Otra etiqueta: vuelve a decir (una vez) que espera.
sleep 0.05
publicar 1.7.2
marcar
actualizar || fallo "falla mientras espera 1.7.2: $(cat "$T/salida")"
[ "$(nuevo log | grep -c 'esperando la versión precompilada de 1.7.2')" -eq 1 ] || fallo "no dice que espera a 1.7.2: $(nuevo log)"
publicar_forgejo 1.7.2
actualizar || fallo "no despliega 1.7.2: $(cat "$T/salida")"
[ "$(actual)" = releases/1.7.2 ] || fallo "no despliega 1.7.2 ($(actual))"
sin_compilar

# --- (q) precompilada de otro commit; etiqueta ligera --------------------------
empieza "q: precompilada de otro commit"
sleep 0.05
publicar 1.7.5
publicar_forgejo 1.7.5 buena "$(commit_de 1.7.2)"
marcar
if actualizar; then fallo "acepta una precompilada de otro commit"; fi
nuevo log | grep -q 'rechazada: el paquete es de otro commit' || fallo "no dice por qué la rechaza: $(nuevo log)"
nuevo log | grep -q 'esperando la versión precompilada de 1.7.5' || fallo "no dice que espera otra: $(nuevo log)"
sin_cambios releases/1.7.2 "$(versiones)"
[ ! -e "$ESTADO/intentos" ] || fallo "una precompilada de otro commit cuenta como fallo"
publicar_forgejo 1.7.5
actualizar || fallo "no despliega la precompilada buena: $(cat "$T/salida")"
[ "$(actual)" = releases/1.7.5 ] || fallo "no despliega 1.7.5"
[ "$(cat "$REL/1.7.5/COMMIT")" = "$(commit_de 1.7.5)" ] || fallo "COMMIT de 1.7.5"
sleep 0.05
empieza "q: etiqueta ligera"
publicar 1.7.6 ligera
publicar_forgejo 1.7.6
actualizar || fallo "no despliega una etiqueta ligera: $(cat "$T/salida")"
[ "$(actual)" = releases/1.7.6 ] || fallo "no despliega 1.7.6 ($(actual))"
sin_compilar

# --- (s) repositorio inaccesible ------------------------------------------------
empieza "s: sin repositorio"
v_antes=$(versiones)
escribir_conf "ssh://git@git.example:2200$T/no-existe"
marcar
actualizar || fallo "sin repositorio no sale bien: $(cat "$T/salida")"
sin_cambios releases/1.7.6 "$v_antes"
nuevo log | grep -q 'sin acceso al repositorio' || fallo "no dice que no llega al repositorio: $(nuevo log)"
escribir_conf
empieza "s: la clave del servidor git no cuadra"
touch "$T/clave-servidor-cambiada"
marcar
actualizar || fallo "con otra clave del servidor git no sale bien: $(cat "$T/salida")"
sin_cambios releases/1.7.6 "$v_antes"
nuevo log | grep -q 'sin acceso al repositorio: Host key verification failed' || fallo "no dice que la clave no cuadra: $(nuevo log)"
rm -f "$T/clave-servidor-cambiada"
empieza "s: sin known_hosts"
mv "$HOSTS" "$T/hosts-aparte"
marcar
if actualizar; then fallo "sale bien sin known_hosts"; fi
nuevo log | grep -q 'HOSTS_CONOCIDOS' || fallo "no dice que falta HOSTS_CONOCIDOS: $(nuevo log)"
[ -z "$(nuevo ssh)" ] || fallo "llama a ssh sin known_hosts"
mv "$T/hosts-aparte" "$HOSTS"
empieza "s: clave de despliegue legible por otros"
chmod 644 "$CLAVE"
marcar
if actualizar; then fallo "usa una clave de despliegue legible por otros"; fi
nuevo log | grep -q 'CLAVE_DESPLIEGUE' || fallo "no dice qué pasa con la clave: $(nuevo log)"
[ -z "$(nuevo ssh)" ] || fallo "llama a ssh con la clave legible por otros"
chmod 600 "$CLAVE"
empieza "s: CLAVE_DESPLIEGUE con espacios"
escribir_conf "" "CLAVE_DESPLIEGUE='/usr/local/etc/una clave'"
marcar
if actualizar; then fallo "acepta CLAVE_DESPLIEGUE con espacios"; fi
nuevo log | grep -q 'CLAVE_DESPLIEGUE' || fallo "no dice qué pasa con CLAVE_DESPLIEGUE: $(nuevo log)"
[ -z "$(nuevo ssh)" ] || fallo "llama a ssh con CLAVE_DESPLIEGUE mala"
escribir_conf
sin_cambios releases/1.7.6 "$v_antes"

# --- (j) configuración escribible por otros ---------------------------------
empieza "j: configuración insegura"
publicar 1.8.0
publicar_forgejo 1.8.0
chmod 620 "$CONF"
v_antes=$(versiones)
marcar
if actualizar; then fallo "usa una configuración escribible por el grupo"; fi
sin_cambios releases/1.7.6 "$v_antes"
[ -z "$(nuevo jexec)" ] || fallo "entra en alguna jail con una configuración insegura"
[ -z "$(nuevo ssh)" ] || fallo "mira el repositorio con una configuración insegura"
nuevo log | grep -q 'semillero-actualizar.conf' || fallo "el registro no dice qué fichero es inseguro"
chmod 600 "$CONF"

# --- (k) --comprobar --------------------------------------------------------
empieza "k: --comprobar"
marcar
# Restos de una ejecución cortada: --comprobar no los toca.
mkdir -p "$REL/.9.9.9.tmp"
RESTOS_PLANTADOS=1
if ! actualizar --comprobar; then
	cat "$T/salida" >&2
	fallo "--comprobar falla"
fi
RESTOS_PLANTADOS=
grep -q 'hay una versión nueva: 1.8.0' "$T/salida" || fallo "--comprobar no dice la versión nueva: $(cat "$T/salida")"
grep -q 'versiones de Forgejo: https://forja.example ejemplo/semillero (curl y token listos)' "$T/salida" ||
	fallo "--comprobar no dice la fuente de Forgejo: $(cat "$T/salida")"
[ -d "$REL/.9.9.9.tmp" ] || fallo "--comprobar borra restos de otra ejecución"
rm -rf "$REL/.9.9.9.tmp"
sin_cambios releases/1.7.6 "$v_antes"
[ -z "$(nuevo curl)" ] || fallo "--comprobar descarga de Forgejo: $(nuevo curl)"
[ -n "$(nuevo ssh)" ] || fallo "--comprobar no mira el repositorio"
[ ! -e "$ESTADO/esperando" ] || fallo "--comprobar apunta que espera"
[ -z "$(nuevo log)" ] || fallo "--comprobar escribe en el registro"

# --- (m) argumentos de más y --ref ------------------------------------------
empieza "m: argumentos de más y --ref"
marcar
for a in "--comprobar --otro" "--comprobar extra" "-- --comprobar" "--ref" "--ref -x" \
	"--ref a b" "--comprobar --ref x" "--ref ../x" "--ref main" "--ref v1.8.0"; do
	# shellcheck disable=SC2086
	if actualizar $a; then
		fallo "acepta «$a»"
	fi
	grep -q 'uso:' "$T/salida" || fallo "«$a» no dice el uso: $(cat "$T/salida")"
	grep -q -- '--ref' "$T/salida" && fallo "el uso todavía habla de --ref: $(cat "$T/salida")"
done
if actualizar "" --comprobar; then
	fallo "acepta «'' --comprobar»"
fi
sin_cambios releases/1.7.6 "$v_antes"
[ -z "$(nuevo jexec)" ] || fallo "con argumentos de más entra en alguna jail"
[ -z "$(nuevo ssh)" ] || fallo "con argumentos de más mira el repositorio"
sin_compilar

# --- (t) falta un valor obligatorio -----------------------------------------
empieza "t: falta un valor obligatorio"
for clave in REPO_URL ESCUCHA; do
	# La configuración sin esa línea (vacía, como si nunca se hubiera puesto).
	escribir_conf
	grep -v "^$clave=" "$CONF" >"$T/conf" && cat "$T/conf" >"$CONF"
	marcar
	if actualizar; then fallo "sale bien sin $clave"; fi
	sin_cambios releases/1.7.6 "$v_antes"
	[ -z "$(nuevo jexec)" ] || fallo "sin $clave entra en alguna jail"
	nuevo log | grep -q "ERROR: falta $clave en /usr/local/etc/semillero-actualizar.conf" ||
		fallo "no dice que falta $clave: $(nuevo log)"
	marcar
	if actualizar --comprobar; then fallo "--comprobar sale bien sin $clave"; fi
	grep -q "falta $clave" "$T/salida" || fallo "--comprobar no dice que falta $clave"
	[ -z "$(nuevo jexec)" ] || fallo "--comprobar sin $clave entra en alguna jail"
done
for mala in 192.0.2.10 :8090 192.0.2.10: 192.0.2.10:80x '192.0.2.10 :8090'; do
	escribir_conf "" "ESCUCHA='$mala'"
	marcar
	if actualizar; then fallo "acepta ESCUCHA=«$mala»"; fi
	[ -z "$(nuevo jexec)" ] || fallo "con ESCUCHA=«$mala» entra en alguna jail"
	nuevo log | grep -q 'ESCUCHA tiene que ser' || fallo "no dice qué pasa con ESCUCHA=«$mala»"
done
escribir_conf

# --- (u) /salud se pregunta en la ESCUCHA de la configuración ---------------
empieza "u: ESCUCHA de la configuración"
# El semillero de mentira escucha ahora en otra dirección; con la ESCUCHA de
# antes en la configuración, /salud no responde y vuelve atrás.
echo "198.51.100.7:9000" >"$T/escucha"
publicar 1.8.1
publicar_forgejo 1.8.1
marcar
if actualizar; then fallo "despliega preguntando /salud en otra dirección"; fi
[ "$(actual)" = releases/1.7.6 ] || fallo "no vuelve a 1.7.6 ($(actual))"
# Con la de la configuración, sí.
ESCUCHA=198.51.100.7:9000
escribir_conf
rm -f "$ESTADO/intentos"
publicar_forgejo 1.8.1
: >"$T/fetch.log"
: >"$T/sockstat.log"
marcar
actualizar || fallo "no despliega con ESCUCHA=$ESCUCHA: $(cat "$T/salida")"
[ "$(actual)" = releases/1.8.1 ] && [ "$(en_marcha)" = 1.8.1 ] || fallo "no queda 1.8.1 desplegada ($(actual))"
grep -qx 'http://198.51.100.7:9000/salud' "$T/fetch.log" || fallo "no pregunta /salud en ESCUCHA: $(cat "$T/fetch.log")"
grep -v 'http://198.51.100.7:9000/salud' "$T/fetch.log" | grep -q . && fallo "pregunta /salud en otra dirección"
grep -qE -- '-p 9000( |$)' "$T/sockstat.log" ||
	fallo "no mira quién escucha en el puerto de ESCUCHA: $(cat "$T/sockstat.log")"
# IPv6 entre corchetes.
ESCUCHA='[2001:db8::7]:9000'
echo "$ESCUCHA" >"$T/escucha"
escribir_conf
publicar 1.8.2
publicar_forgejo 1.8.2
: >"$T/fetch.log"
actualizar || fallo "no despliega con ESCUCHA=$ESCUCHA: $(cat "$T/salida")"
grep -qxF 'http://[2001:db8::7]:9000/salud' "$T/fetch.log" || fallo "no pregunta /salud en $ESCUCHA: $(cat "$T/fetch.log")"
v_antes=$(versiones)

# --- (x) versiones de GitHub ------------------------------------------------------
# fetch del host y gh de mentira (como en deploy/pruebas/precompiladas.sh):
# fetch sirve $GH/web (github.com) y $GH/api (api.github.com); gh da por
# buena una atestación {"falsa":"<sha256> <repo> <ref> <flujo>"} que case
# con el paquete. jq es el de verdad.
empieza "x: sin GITHUB_REPO no se mira GitHub"
# Hasta aquí ninguna configuración tenía GITHUB_REPO.
[ ! -e "$GH/fetch.log" ] && [ ! -e "$GH/gh.log" ] || fallo "sin GITHUB_REPO llama a fetch o gh en el host"

RG=ejemplo/semillero
# Publica en el GitHub de mentira la versión $1 como release.yml. $2:
# variante del binario (binario); $3: buena, sha-malo u otra-etiqueta.
publicar_github()
{
	_v=$1
	_a=semillero-$_v-$OBJ
	_rel=$GH/web/$RG/releases/download/v$_v
	_b=$T/build-github/$_v
	rm -rf "$_b" "$_rel" "$GH/api"
	mkdir -p "$_b/$_a/site/pkg" "$_b/$_a/rc.d" "$_rel" "$GH/api/repos/$RG/attestations"
	binario "$_v" "${2:-buena}" >"$_b/$_a/semillero"
	printf 'js: abc\nwasm: abc\ncss: abc\n' >"$_b/$_a/site/hash.txt"
	for e in js wasm css; do printf '%s %s\n' "$e" "$_v" >"$_b/$_a/site/pkg/semillero.abc.$e"; done
	printf '%040d\n' 0 | tr 0 c >"$_b/$_a/COMMIT"
	for f in LICENSE README.md README.en.md rc.d/semillero; do echo "$f" >"$_b/$_a/$f"; done
	tar -C "$_b" --sort=name --owner=0 --group=0 --numeric-owner -czf "$_rel/$_a.tar.gz" "$_a"
	_d=$(sha256sum <"$_rel/$_a.tar.gz" | cut -c1-64)
	_s=$_d
	if [ "${3:-}" = sha-malo ]; then _s=$(printf '%064d' 2); fi
	printf '%s  %s\n%s  %s\n' "$_s" "$_a.tar.gz" "$(printf '%064d' 1)" "semillero-$_v-x86_64-unknown-linux-gnu.tar.gz" >"$_rel/SHA256SUMS"
	_ref=refs/tags/v$_v
	if [ "${3:-}" = otra-etiqueta ]; then _ref=refs/tags/v9.9.9; fi
	printf '{"attestations":[{"bundle":{"falsa":"%s %s %s %s/.github/workflows/release.yml"}}]}\n' \
		"$_d" "$RG" "$_ref" "$RG" >"$GH/api/repos/$RG/attestations/sha256:$_d"
}
nuevo_fetch()
{
	tail -n +$((m_fetch + 1)) "$GH/fetch.log" 2>/dev/null || true
}
marcar_fetch()
{
	m_fetch=$(lineas "$GH/fetch.log")
}
escribir_conf "" "GITHUB_REPO=$RG"

empieza "x: versión de GitHub (Forgejo sin release)"
publicar 1.9.0
publicar_github 1.9.0
marcar
marcar_fetch
actualizar || fallo "no despliega la versión de GitHub: $(cat "$T/salida")"
nuevo curl | grep -q 'releases/tags/v1.9.0$' || fallo "no mira Forgejo antes que GitHub: $(nuevo curl)"
[ "$(actual)" = releases/1.9.0 ] && [ "$(en_marcha)" = 1.9.0 ] || fallo "no despliega 1.9.0 de GitHub ($(actual))"
[ "$(find "$REL/1.9.0" -mindepth 1 -maxdepth 1 -printf '%f\n' | sort | tr '\n' ' ')" = "COMMIT semillero site " ] ||
	fallo "instala de GitHub algo más que semillero, site/ y COMMIT"
[ "$(stat -c %a "$REL/1.9.0/semillero" "$REL/1.9.0/COMMIT" | tr '\n' ' ')" = "755 644 " ] || fallo "permisos de lo de GitHub"
nuevo log | grep -q "instalada la versión de GitHub de v1.9.0 ($RG" || fallo "el registro no dice que viene de GitHub: $(nuevo log)"
grep -q -- "--repo $RG --source-ref refs/tags/v1.9.0 --signer-workflow $RG/.github/workflows/release.yml" "$GH/gh.log" ||
	fallo "gh no comprueba repositorio, etiqueta y flujo: $(cat "$GH/gh.log")"
sin_compilar

empieza "x: GitHub con sha256 malo"
publicar 1.9.1
publicar_github 1.9.1 buena sha-malo
echo "v1.9.1 1" >"$ESTADO/precompilada"
marcar
if actualizar; then fallo "sale bien rechazando la versión de GitHub"; fi
nuevo log | grep -q 'ERROR: versión de GitHub de v1.9.1 rechazada: el sha256' || fallo "no dice por qué rechaza GitHub: $(nuevo log)"
nuevo log | grep -q 'esperando la versión precompilada de 1.9.1' || fallo "no espera tras rechazar GitHub: $(nuevo log)"
sin_cambios releases/1.9.0 "$(versiones)"
[ ! -e "$REL/1.9.1" ] || fallo "instala la versión de GitHub con sha256 malo"
[ ! -e "$ESTADO/intentos" ] || fallo "un sha256 malo de GitHub cuenta como fallo de la etiqueta"
# La misma release mala en la pasada siguiente: no se repite el error.
marcar
actualizar >/dev/null 2>&1 || true
[ -z "$(nuevo log)" ] || fallo "repite el mismo rechazo de GitHub en cada pasada: $(nuevo log)"
# Sale en Forgejo: se instala.
publicar_forgejo 1.9.1
actualizar || fallo "no despliega de Forgejo tras rechazar GitHub: $(cat "$T/salida")"
[ "$(actual)" = releases/1.9.1 ] || fallo "no despliega 1.9.1 de Forgejo"
sin_compilar

empieza "x: atestación que no cuadra"
publicar 1.9.2
publicar_github 1.9.2 buena otra-etiqueta
marcar
if actualizar; then fallo "sale bien con una atestación que no cuadra"; fi
sin_cambios releases/1.9.1 "$(versiones)"
[ ! -e "$REL/1.9.2" ] || fallo "instala una versión cuya atestación no cuadra"
nuevo log | grep -q 'ERROR: versión de GitHub de v1.9.2 rechazada: la atestación' || fallo "no dice que la atestación no cuadra: $(nuevo log)"
nuevo log | grep -q 'esperando la versión precompilada de 1.9.2' || fallo "no sigue esperando: $(nuevo log)"
[ ! -e "$ESTADO/intentos" ] || fallo "una atestación que no cuadra cuenta como fallo de la etiqueta"
publicar_forgejo 1.9.2
actualizar || fallo "no despliega de Forgejo tras la atestación mala: $(cat "$T/salida")"
[ "$(actual)" = releases/1.9.2 ] || fallo "no despliega 1.9.2 de Forgejo"
sin_compilar

empieza "x: sin release en GitHub"
publicar 1.9.3
rm -rf "$GH/web/$RG/releases/download/v1.9.3"
echo "v1.9.3 1" >"$ESTADO/precompilada"
marcar
actualizar || fallo "falla sin release en GitHub: $(cat "$T/salida")"
nuevo log | grep -q 'esperando la versión precompilada de 1.9.3.*GitHub: no se puede descargar' || fallo "no dice que espera y que no hay release: $(nuevo log)"
nuevo log | grep -q 'ERROR' && fallo "una release que aún no está se da como error: $(nuevo log)"
sin_cambios releases/1.9.2 "$(versiones)"
marcar
marcar_fetch
actualizar || fallo "falla sin release en GitHub (2.ª pasada): $(cat "$T/salida")"
[ -n "$(nuevo_fetch)" ] || fallo "no vuelve a mirar GitHub en la pasada siguiente"
[ -z "$(nuevo log)" ] || fallo "repite en el registro que espera: $(nuevo log)"
# Sale la release: se instala.
publicar_github 1.9.3
actualizar || fallo "no despliega la release cuando sale: $(cat "$T/salida")"
[ "$(actual)" = releases/1.9.3 ] || fallo "no despliega 1.9.3 de GitHub ($(actual))"
sin_compilar

empieza "x: Forgejo va primero"
publicar 1.9.4
publicar_forgejo 1.9.4
publicar_github 1.9.4
marcar
marcar_fetch
actualizar || fallo "no despliega de Forgejo: $(cat "$T/salida")"
nuevo log | grep -q 'instalada la versión de Forgejo de v1.9.4' || fallo "no usa Forgejo: $(nuevo log)"
[ -z "$(nuevo_fetch)" ] || fallo "descarga de GitHub teniendo la de Forgejo: $(nuevo_fetch)"
[ "$(actual)" = releases/1.9.4 ] || fallo "no despliega 1.9.4"

empieza "x: el 2.º intento vuelve a GitHub"
publicar 1.9.5
publicar_github 1.9.5 roto
marcar
if actualizar; then fallo "acepta una versión de GitHub que no arranca"; fi
[ "$(cat "$ESTADO/intentos" 2>/dev/null | tr '\n' ' ')" = "v1.9.5 1 " ] || fallo "no cuenta el fallo de la versión de GitHub"
marcar
marcar_fetch
if actualizar; then fallo "acepta una versión de GitHub que no arranca (2.ª vez)"; fi
[ -n "$(nuevo_fetch)" ] || fallo "no vuelve a GitHub en el 2.º intento"
[ "$(cat "$ESTADO/fallida" 2>/dev/null)" = v1.9.5 ] || fallo "tras 2 fallos de GitHub no queda fallida"
[ "$(actual)" = releases/1.9.4 ] || fallo "no queda 1.9.4 ($(actual))"
sin_compilar

empieza "x: --comprobar con GITHUB_REPO"
marcar
actualizar --comprobar || fallo "--comprobar falla con GITHUB_REPO: $(cat "$T/salida")"
grep -q "GitHub: $RG (gh y jq instalados)" "$T/salida" || fallo "--comprobar no dice la fuente de GitHub: $(cat "$T/salida")"
[ -z "$(nuevo log)" ] || fallo "--comprobar escribe en el registro"

empieza "x: GITHUB_REPO no válido"
for mala in ejemplo 'ejemplo/semillero/x' '-ejemplo/semillero' 'ejemplo/semillero;id' 'ejemplo/..'; do
	escribir_conf "" "GITHUB_REPO='$mala'"
	marcar
	if actualizar; then fallo "acepta GITHUB_REPO=«$mala»"; fi
	[ -z "$(nuevo jexec)" ] || fallo "con GITHUB_REPO=«$mala» entra en alguna jail"
	nuevo log | grep -q 'GITHUB_REPO tiene que ser' || fallo "no dice qué pasa con GITHUB_REPO=«$mala»: $(nuevo log)"
done
escribir_conf
v_antes=$(versiones)

# --- (z) Forgejo ------------------------------------------------------------------
# La etiqueta nueva, publicada en Forgejo, que no se mira mientras falte
# FORGEJO_URL o FORGEJO_REPO.
publicar 1.9.6
publicar_forgejo 1.9.6
for falta in FORGEJO_URL FORGEJO_REPO; do
	empieza "z: sin $falta no se mira Forgejo"
	escribir_conf "" "$falta="
	rm -f "$ESTADO/esperando" "$ESTADO/forgejo-aviso"
	marcar
	actualizar || fallo "falla sin $falta: $(cat "$T/salida")"
	[ -z "$(nuevo curl)" ] || fallo "llama a curl sin $falta: $(nuevo curl)"
	[ "$(nuevo log | grep -c 'Forgejo desactivado')" -eq 1 ] || fallo "no dice una vez que Forgejo está desactivado: $(nuevo log)"
	nuevo log | grep -q 'esperando la versión precompilada de 1.9.6' || fallo "no espera con Forgejo desactivado: $(nuevo log)"
	sin_cambios releases/1.9.4 "$v_antes"
	marcar
	actualizar || fallo "falla sin $falta (2.ª pasada): $(cat "$T/salida")"
	[ -z "$(nuevo log)" ] || fallo "repite que Forgejo está desactivado: $(nuevo log)"
	[ ! -e "$ESTADO/intentos" ] || fallo "Forgejo desactivado cuenta como fallo"
done

empieza "z: token de Forgejo legible por otros"
escribir_conf
chmod 644 "$TOKEN"
marcar
actualizar || fallo "falla con el token legible por otros: $(cat "$T/salida")"
[ -z "$(nuevo curl)" ] || fallo "llama a curl con el token legible por otros: $(nuevo curl)"
nuevo log | grep -q 'FORGEJO_TOKEN_FICHERO' || fallo "no dice qué pasa con el token: $(nuevo log)"
sin_cambios releases/1.9.4 "$v_antes"
[ ! -e "$ESTADO/intentos" ] || fallo "el token legible por otros cuenta como fallo"
marcar
actualizar || fallo "falla con el token legible por otros (2.ª pasada): $(cat "$T/salida")"
[ -z "$(nuevo log)" ] || fallo "repite lo del token en cada pasada: $(nuevo log)"
chmod 600 "$TOKEN"

empieza "z: sin token de Forgejo"
mv "$TOKEN" "$T/token-aparte"
marcar
actualizar || fallo "falla sin token: $(cat "$T/salida")"
[ -z "$(nuevo curl)" ] || fallo "llama a curl sin token: $(nuevo curl)"
sin_cambios releases/1.9.4 "$v_antes"
mv "$T/token-aparte" "$TOKEN"

empieza "z: con el token bien, se instala de Forgejo"
marcar
actualizar || fallo "no despliega 1.9.6 de Forgejo: $(cat "$T/salida")"
[ "$(actual)" = releases/1.9.6 ] || fallo "no despliega 1.9.6 ($(actual))"
v_antes=$(versiones)

empieza "z: FORGEJO_URL o FORGEJO_REPO no válidas"
for mala in "FORGEJO_URL=forja.example" "FORGEJO_URL='https://forja.example/a b'" \
	"FORGEJO_URL='https://forja.example;id'" "FORGEJO_REPO=ejemplo" "FORGEJO_REPO='ejemplo/semillero/x'" \
	"FORGEJO_REPO='-ejemplo/semillero'" "FORGEJO_TOKEN_FICHERO='token relativo'"; do
	escribir_conf "" "$mala"
	marcar
	if actualizar; then fallo "acepta $mala"; fi
	[ -z "$(nuevo jexec)" ] || fallo "con $mala entra en alguna jail"
	[ -z "$(nuevo curl)" ] || fallo "con $mala llama a curl"
	nuevo log | grep -q "${mala%%=*} tiene que ser" || fallo "no dice qué pasa con $mala: $(nuevo log)"
done
escribir_conf

# --- (l) install.sh en una carpeta -----------------------------------------
empieza "l: install.sh"

# ssh-keygen de mentira para install.sh (apunta cada llamada) y el git del
# host que necesita el actualizador.
FI=$T/falsos-install
mkdir -p "$FI"
cat >"$FI/ssh-keygen" <<EOF
#!/bin/sh
echo "ssh-keygen \$*" >>"$T/ssh-keygen.log"
f= c=
while [ "\$#" -gt 0 ]; do
	case \$1 in
	-f) f=\$2; shift 2 ;;
	-C) c=\$2; shift 2 ;;
	-t | -N) shift 2 ;;
	*) shift ;;
	esac
done
[ ! -e "\$f" ] || exit 1
printf 'privada\n' >"\$f"
chmod 600 "\$f"
printf '${TIPO_SSH} AAAAC3NzaC1lZDI1NTE5AAAAIPUBLICA %s\n' "\$c" >"\$f.pub"
EOF
chmod 755 "$FI/ssh-keygen"
instalar_en()
{
	PATH=$FI:$PATH sh "$DEPLOY/freebsd/install.sh" --prefijo "$1" >"$T/salida" 2>&1
}
# Un ejecutable vacío en $1/usr/local/bin/$2 (lo que dejaría pkg install).
paquete()
{
	mkdir -p "$1/usr/local/bin"
	printf '#!/bin/sh\n' >"$1/usr/local/bin/$2"
	chmod 755 "$1/usr/local/bin/$2"
}

# Configuración de install.sh $1; $3: líneas de más ($2 no se usa).
conf_install()
{
	mkdir -p "$1/usr/local/etc"
	cat >"$1/usr/local/etc/semillero-actualizar.conf" <<-EOF
		REPO_URL=ssh://git@192.0.2.20:2200/usuario/semillero.git
		REPO_HOST_CLAVE='[192.0.2.20]:2200 ${TIPO_SSH} AAAAC3NzaC1lZDI1NTE5AAAAISERVIDOR'
		ESCUCHA=192.0.2.10:8090
		${3:-}
	EOF
	chmod 600 "$1/usr/local/etc/semillero-actualizar.conf"
}

# Sin configuración: escribe la plantilla y para sin instalar nada más.
I=$T/inst
mkdir -p "$I/usr/local/jails/containers/semillero"
paquete "$I" git
if instalar_en "$I"; then
	fallo "install.sh sale bien sin configuración"
fi
grep -q 'ERROR: faltan en /usr/local/etc/semillero-actualizar.conf: REPO_URL ESCUCHA;' "$T/salida" ||
	fallo "no dice qué falta en la configuración: $(cat "$T/salida")"
[ "$(stat -c %a "$I/usr/local/etc/semillero-actualizar.conf" 2>&1)" = 600 ] || fallo "no escribe la plantilla con 600"
for clave in REPO_URL= REPO_HOST_CLAVE= ESCUCHA= JAIL=semillero JAIL_RAIZ= GITHUB_REPO= \
	CLAVE_DESPLIEGUE=/usr/local/etc/semillero-actualizar/id_ed25519 \
	HOSTS_CONOCIDOS=/usr/local/etc/semillero-actualizar/known_hosts FORGEJO_URL= FORGEJO_REPO= \
	FORGEJO_TOKEN_FICHERO=/usr/local/etc/semillero-actualizar/forgejo-token; do
	grep -qx "$clave" "$I/usr/local/etc/semillero-actualizar.conf" || fallo "la plantilla no tiene $clave"
done
for clave in JAIL_COMPILAR JAIL_COMPILAR_IP PRECOMPILADAS_ESPERA TRABAJOS_COMPILACION USUARIO_COMPILAR \
	REPO_IP DNS_IP RED_LAN INTERFAZ_EXT USUARIO_BUZON UID_BUZON; do
	grep -q "^$clave=" "$I/usr/local/etc/semillero-actualizar.conf" && fallo "la plantilla todavía tiene $clave"
done
[ ! -e "$I/usr/local/sbin" ] && [ ! -e "$I/usr/local/etc/cron.d" ] && [ ! -e "$I/var" ] ||
	fallo "sin configuración instala algo más que la plantilla: $(cd "$I" && find . -mindepth 1 | tr '\n' ' ')"

# Con una configuración a la que le falta un valor: dice cuál y no instala.
I=$T/inst-falta
mkdir -p "$I/usr/local/jails/containers/semillero"
paquete "$I" git
conf_install "$I" nuevas
grep -v '^ESCUCHA=' "$I/usr/local/etc/semillero-actualizar.conf" >"$T/conf"
cat "$T/conf" >"$I/usr/local/etc/semillero-actualizar.conf"
if instalar_en "$I"; then
	fallo "install.sh sale bien sin ESCUCHA"
fi
grep -q 'ERROR: faltan en /usr/local/etc/semillero-actualizar.conf: ESCUCHA;' "$T/salida" ||
	fallo "no dice que falta ESCUCHA: $(cat "$T/salida")"
[ ! -e "$I/usr/local/sbin" ] || fallo "sin ESCUCHA instala el actualizador"

# Sin git en el host: dice qué paquete falta y no instala.
I=$T/inst-sin-git
mkdir -p "$I/usr/local/jails/containers/semillero"
conf_install "$I" nuevas
if instalar_en "$I"; then fallo "install.sh sale bien sin git en el host"; fi
grep -q 'pkg install git' "$T/salida" || fallo "no dice que falta git: $(cat "$T/salida")"
[ ! -e "$I/usr/local/sbin" ] || fallo "sin git instala el actualizador"

# Una ruta de la clave que no vale: para antes de instalar nada.
I=$T/inst-clave-mala
mkdir -p "$I/usr/local/jails/containers/semillero"
paquete "$I" git
conf_install "$I" nuevas "CLAVE_DESPLIEGUE='clave relativa'"
if instalar_en "$I"; then fallo "install.sh acepta CLAVE_DESPLIEGUE relativa y con espacios"; fi
grep -q 'CLAVE_DESPLIEGUE' "$T/salida" || fallo "no dice qué pasa con CLAVE_DESPLIEGUE: $(cat "$T/salida")"
[ ! -e "$I/usr/local/sbin" ] || fallo "con CLAVE_DESPLIEGUE mala instala el actualizador"

I=$T/inst
conf_install "$I" nuevas
: >"$T/ssh-keygen.log"
if ! instalar_en "$I"; then
	cat "$T/salida" >&2
	fallo "install.sh falla"
fi
cmp -s "$ACT" "$I/usr/local/sbin/semillero-actualizar" || fallo "no instala el actualizador"
L=usr/local/libexec/semillero-actualizar
cmp -s "$DEPLOY/lib/git.sh" "$I/$L/git.sh" && cmp -s "$DEPLOY/lib/precompiladas.sh" "$I/$L/precompiladas.sh" ||
	fallo "no instala lo de libexec"
[ ! -e "$I/$L/recibir-precompilada.sh" ] && [ ! -e "$I/var/db/semillero-buzon" ] ||
	fallo "todavía instala el buzón"
K=usr/local/etc/semillero-actualizar
modos=$(cd "$I" && stat -c '%a' usr/local/sbin/semillero-actualizar \
	usr/local/etc/semillero-actualizar.conf usr/local/etc/cron.d/semillero \
	usr/local/etc/newsyslog.conf.d/semillero.conf usr/local/semillero/releases \
	var/db/semillero-actualizar \
	usr/local/jails/containers/semillero/usr/local/semillero/releases \
	$L/git.sh $L/precompiladas.sh \
	$K $K/id_ed25519 $K/known_hosts 2>&1 | tr '\n' ' ')
[ "$modos" = "755 600 644 644 755 700 755 644 644 700 600 644 " ] || fallo "permisos de lo instalado: $modos"
# La clave de despliegue, en el host; el known_hosts, el de REPO_HOST_CLAVE.
[ "$(grep -c . "$T/ssh-keygen.log")" -eq 1 ] && grep -q -- "-t ed25519" "$T/ssh-keygen.log" &&
	grep -q -- "-f $I/$K/id_ed25519" "$T/ssh-keygen.log" || fallo "no genera la clave de despliegue: $(cat "$T/ssh-keygen.log")"
grep -qxF '[192.0.2.20]:2200 ssh-ed25519'' AAAAC3NzaC1lZDI1NTE5AAAAISERVIDOR' "$I/$K/known_hosts" ||
	fallo "el known_hosts no es REPO_HOST_CLAVE: $(cat "$I/$K/known_hosts")"
grep -qF 'ssh-ed25519'' AAAAC3NzaC1lZDI1NTE5AAAAIPUBLICA' "$T/salida" || fallo "no escribe la clave pública para darla de alta"
grep -qx '\*/15	\*	\*	\*	\*	root	/usr/local/sbin/semillero-actualizar' \
	"$I/usr/local/etc/cron.d/semillero" || fallo "la línea de cron no es la esperada"
grep -q '^/var/log/semillero-update.log	' "$I/usr/local/etc/newsyslog.conf.d/semillero.conf" ||
	fallo "falta la línea de newsyslog"
grep -qxF '     /usr/local/semillero/releases	/usr/local/jails/containers/semillero/usr/local/semillero/releases	nullfs	ro	0	0' "$T/salida" ||
	fallo "no escribe la línea de /etc/fstab.semillero"
grep -qF 'mount -t nullfs -o ro /usr/local/semillero/releases /usr/local/jails/containers/semillero/usr/local/semillero/releases' "$T/salida" ||
	fallo "no escribe la orden de montaje"
grep -qiE 'buz[oó]n|smentrega|authorized_keys|pw useradd' "$T/salida" &&
	fallo "todavía habla del buzón: $(grep -iE 'buz[oó]n|smentrega|authorized_keys|pw useradd' "$T/salida")"
grep -qF 'escuchando en 192.0.2.10:8090' "$T/salida" || fallo "no dice dónde tiene que escuchar el servicio"
# Nada de la jail de compilación.
grep -qiE 'jail\.conf|pf\.conf|pfctl|semillero-build|cargo|jail de compilaci' "$T/salida" &&
	fallo "todavía habla de la jail de compilación: $(grep -iE 'jail\.conf|pf\.conf|pfctl|semillero-build|cargo|jail de compilaci' "$T/salida")"
[ ! -e "$DEPLOY/freebsd/jail.conf.plantilla" ] && [ ! -e "$DEPLOY/freebsd/pf.conf.plantilla" ] ||
	fallo "quedan las plantillas de jail.conf y pf.conf"
grep -q '@[A-Z_]*@' "$T/salida" && fallo "deja huecos de plantilla sin rellenar"
# Solo direcciones de documentación (y 127.0.0.1) en lo que escribe.
ips=$(grep -ohE '[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+' "$T/salida" "$I/usr/local/etc/semillero-actualizar.conf" \
	"$DEPLOY/freebsd/install.sh" | grep -vE '^(192\.0\.2|198\.51\.100|203\.0\.113)\.|^127\.0\.0\.1$' | sort -u | tr '\n' ' ')
[ -z "$ips" ] || fallo "escribe direcciones que no son de documentación: $ips"
# La 2.ª vez: ni la configuración, ni la clave, ni el known_hosts cambian.
echo '# cambiada' >>"$I/usr/local/etc/semillero-actualizar.conf"
echo '# cambiado' >>"$I/$K/known_hosts"
cp "$I/$K/id_ed25519" "$T/clave-antes"
instalar_en "$I" || fallo "install.sh falla la 2.ª vez: $(cat "$T/salida")"
grep -qx '# cambiada' "$I/usr/local/etc/semillero-actualizar.conf" || fallo "install.sh pisa la configuración"
grep -qx '# cambiado' "$I/$K/known_hosts" || fallo "install.sh pisa el known_hosts"
cmp -s "$T/clave-antes" "$I/$K/id_ed25519" && [ "$(grep -c . "$T/ssh-keygen.log")" -eq 1 ] ||
	fallo "install.sh rehace la clave de despliegue"

empieza "l: instalación anterior, con jail de compilación"
# La configuración de una instalación con jail de compilación, con la clave
# de despliegue ya pasada al host y sin REPO_HOST_CLAVE (el known_hosts, ya
# copiado): se instala sin tocar nada de eso y dice que la jail sobra.
I=$T/inst-anterior
mkdir -p "$I/usr/local/jails/containers/semillero" "$I/$K"
paquete "$I" git
cat >"$I/usr/local/etc/semillero-actualizar.conf" <<-'EOF'
	REPO_URL=ssh://git@192.0.2.20:2200/usuario/semillero.git
	REPO_HOST_CLAVE=
	ESCUCHA=192.0.2.10:8090
	JAIL=semillero
	JAIL_RAIZ=
	JAIL_COMPILAR=semillero-build
	JAIL_COMPILAR_RAIZ=
	JAIL_COMPILAR_IP=192.0.2.30
	PRECOMPILADAS_ESPERA=20
	TRABAJOS_COMPILACION=
	GITHUB_REPO=
	REPO_IP=192.0.2.20
	REPO_PUERTO=2200
	DNS_IP=192.0.2.1
	RED_LAN='{ 192.0.2.0/24 198.51.100.0/24 }'
	INTERFAZ_EXT=em0
	USUARIO_COMPILAR=smbuild
	UID_COMPILAR=3170
	USUARIO_BUZON=smentrega
	UID_BUZON=3171
EOF
chmod 600 "$I/usr/local/etc/semillero-actualizar.conf"
cp "$I/usr/local/etc/semillero-actualizar.conf" "$T/conf-anterior"
printf 'privada pasada de la jail\n' >"$I/$K/id_ed25519"
printf 'ssh-ed25519'' AAAAPASADA smbuild\n' >"$I/$K/id_ed25519.pub"
printf '[192.0.2.20]:2200 ssh-ed25519'' AAAAPASADO\n' >"$I/$K/known_hosts"
chmod 600 "$I/$K/id_ed25519"
: >"$T/ssh-keygen.log"
if ! instalar_en "$I"; then
	cat "$T/salida" >&2
	fallo "install.sh falla con la configuración de una instalación anterior"
fi
cmp -s "$T/conf-anterior" "$I/usr/local/etc/semillero-actualizar.conf" || fallo "toca la configuración anterior"
[ ! -s "$T/ssh-keygen.log" ] || fallo "rehace la clave de despliegue pasada de la jail"
grep -qx 'privada pasada de la jail' "$I/$K/id_ed25519" || fallo "pisa la clave de despliegue"
grep -qx '\[192.0.2.20\]:2200 ssh-ed25519'' AAAAPASADO' "$I/$K/known_hosts" || fallo "pisa el known_hosts"
[ "$(stat -c %a "$I/$K/id_ed25519")" = 600 ] || fallo "permisos de la clave pasada"
grep -q 'semillero-build ya no hace falta' "$T/salida" || fallo "no dice que la jail de compilación sobra: $(cat "$T/salida")"
grep -qF 'Migrar una instalación existente' "$T/salida" || fallo "no remite a «Migrar una instalación existente»"
grep -q 'smentrega ya no hace falta' "$T/salida" || fallo "no dice que el usuario del buzón sobra: $(cat "$T/salida")"
cmp -s "$ACT" "$I/usr/local/sbin/semillero-actualizar" || fallo "no instala el actualizador nuevo"

empieza "l: sin known_hosts ni REPO_HOST_CLAVE"
I=$T/inst-sin-hosts
mkdir -p "$I/usr/local/jails/containers/semillero"
paquete "$I" git
conf_install "$I" nuevas "REPO_HOST_CLAVE="
instalar_en "$I" || fallo "install.sh falla sin REPO_HOST_CLAVE: $(cat "$T/salida")"
[ ! -e "$I/$K/known_hosts" ] || fallo "escribe un known_hosts sin saber la clave del servidor git"
grep -q 'ssh-keyscan' "$T/salida" && grep -q 'HOSTS_CONOCIDOS' "$T/salida" ||
	fallo "no dice cómo fijar la clave del servidor git: $(cat "$T/salida")"

# --- (v) otra jail ------------------------------------------------------------
empieza "v: otra jail"
I=$T/inst-otra
mkdir -p "$I/usr/local/jails/containers/torre"
paquete "$I" git
conf_install "$I" nuevas 'JAIL=torre'
if ! instalar_en "$I"; then
	cat "$T/salida" >&2
	fallo "install.sh falla con otra jail"
fi
grep -qF '/usr/local/jails/containers/torre/usr/local/semillero/releases' "$T/salida" ||
	fallo "no toma la raíz de la jail de JAIL"

# --- (w) repositorio por https: sin clave de despliegue -------------------------
empieza "w: repositorio por https"
I=$T/inst-https
mkdir -p "$I/usr/local/jails/containers/semillero"
paquete "$I" git
conf_install "$I" nuevas 'REPO_URL=https://git.example/usuario/semillero.git'
: >"$T/ssh-keygen.log"
if ! instalar_en "$I"; then
	cat "$T/salida" >&2
	fallo "install.sh falla con el repositorio por https"
fi
[ ! -s "$T/ssh-keygen.log" ] || fallo "genera una clave de despliegue para un repositorio por https"
[ ! -e "$I/$K/id_ed25519" ] && [ ! -e "$I/$K/known_hosts" ] || fallo "escribe clave o known_hosts para https"

# --- (y) install.sh con GITHUB_REPO ---------------------------------------------
empieza "y: install.sh con GITHUB_REPO"
I=$T/inst-github
mkdir -p "$I/usr/local/jails/containers/semillero"
paquete "$I" git
conf_install "$I" nuevas 'GITHUB_REPO=ejemplo/semillero'
if instalar_en "$I"; then
	fallo "install.sh sale bien con GITHUB_REPO y sin gh ni jq"
fi
grep -q 'pkg install gh jq' "$T/salida" || fallo "no dice qué paquetes faltan: $(cat "$T/salida")"
[ ! -e "$I/usr/local/sbin" ] || fallo "sin gh instala el actualizador"
paquete "$I" gh
instalar_en "$I" && fallo "install.sh sale bien sin jq"
grep -q 'pkg install jq' "$T/salida" || fallo "no dice que falta jq: $(cat "$T/salida")"
paquete "$I" jq
if ! instalar_en "$I"; then
	cat "$T/salida" >&2
	fallo "install.sh falla con GITHUB_REPO, gh y jq"
fi
conf_install "$I" nuevas 'GITHUB_REPO=ejemplo'
if instalar_en "$I"; then
	fallo "install.sh acepta GITHUB_REPO=ejemplo"
fi
grep -q 'GITHUB_REPO tiene que ser' "$T/salida" || fallo "install.sh no dice qué pasa con GITHUB_REPO: $(cat "$T/salida")"

# --- (z) install.sh con Forgejo ---------------------------------------------------
empieza "z: install.sh con Forgejo"
I=$T/inst-forgejo
mkdir -p "$I/usr/local/jails/containers/semillero"
paquete "$I" git
conf_install "$I" nuevas 'REPO_URL=https://git.example/usuario/semillero.git
FORGEJO_URL=https://forja.example
FORGEJO_REPO=ejemplo/semillero'
if instalar_en "$I"; then fallo "install.sh sale bien con Forgejo y sin curl"; fi
grep -q 'pkg install curl' "$T/salida" || fallo "no dice que falta curl: $(cat "$T/salida")"
[ ! -e "$I/usr/local/sbin" ] || fallo "sin curl instala el actualizador"
paquete "$I" curl
if ! instalar_en "$I"; then
	cat "$T/salida" >&2
	fallo "install.sh falla con Forgejo y curl"
fi
[ "$(stat -c %a "$I/$K" 2>&1)" = 700 ] || fallo "no crea la carpeta del token con 0700: $(stat -c %a "$I/$K" 2>&1)"
[ ! -e "$I/$K/forgejo-token" ] || fallo "escribe un token"
grep -qF "$K/forgejo-token" "$T/salida" && grep -q 'solo lectura' "$T/salida" ||
	fallo "no dice dónde poner el token de solo lectura: $(cat "$T/salida")"
for mala in 'FORGEJO_URL=forja.example' 'FORGEJO_REPO=ejemplo'; do
	conf_install "$I" nuevas "REPO_URL=https://git.example/usuario/semillero.git
FORGEJO_URL=https://forja.example
FORGEJO_REPO=ejemplo/semillero
$mala"
	if instalar_en "$I"; then fallo "install.sh acepta $mala"; fi
	grep -q "${mala%%=*} tiene que ser" "$T/salida" || fallo "install.sh no dice qué pasa con $mala: $(cat "$T/salida")"
done

# --- en todas: el servidor no compila ---------------------------------------------
empieza "nadie compila"
sin_compilar
[ ! -e "$R/var/run/compilar.lock" ] || fallo "crea el cerrojo de compilación"
[ ! -e "$R/var/log/semillero-build.log" ] || fallo "escribe un registro de compilación"
grep -v '^semillero ' "$T/jexec.log" | grep -q . && fallo "entra en otra jail: $(grep -v '^semillero ' "$T/jexec.log" | sort -u | tr '\n' ' ')"

if [ "$fallos" -ne 0 ]; then
	echo "$fallos fallos en $casos casos" >&2
	exit 1
fi
echo "OK ($casos casos)"
