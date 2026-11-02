#!/bin/sh
# Pruebas del actualizador del servidor (deploy/freebsd/actualizar.sh) en
# Linux, sin servidor y sin tocar el sistema: todo vive en una carpeta
# temporal (fuera del repositorio) que se borra al terminar.
#
# - Las rutas fijas del actualizador cuelgan de SEMILLERO_ACT_RAIZ (la
#   carpeta temporal), y sus órdenes salen de SEMILLERO_ACT_PATH, donde van
#   delante unas de mentira: lockf (también con descriptor: el cerrojo
#   neutro de compilación, «ocupado» si existe un fichero de marca), jexec
#   (con service y fetch de la jail del servicio y la jail de compilación, que
#   corre aquí con su clon y sus rutas cambiadas), sockstat, ps y sha256.
# - Un «origen» git hace de servidor git: cada versión es un commit con su
#   crates/semillero/Cargo.toml y su etiqueta vX.Y.Z. El clon de la jail de
#   compilación lo trae.
# - El PC de compilación se simula dejando el paquete en el buzón; la
#   compilación del servidor, con un cargo de mentira que deja un binario
#   (una cabecera ELF de FreeBSD) y la web.
# - El semillero de mentira (service de la jail) arranca la versión de
#   «current» y su /salud responde con la versión que dice su binario; un
#   binario «roto» no arranca y uno con «migra» toca la base de datos.
#
# Casos: (a) primera instalación desde el buzón; (b) la misma etiqueta otra
# vez no hace nada; (c) SHA256SUMS que falta (espera) o está mal hecho (dos
# líneas, otro nombre, no hexadecimal): rechazada, y si la compilación de
# respaldo falla, cuenta un fallo; (d) sha256 malo; (e) paquetes peligrosos
# (ruta absoluta, «..», enlace simbólico, enlace duro, carpeta raíz
# equivocada); (f) /salud con otra versión: vuelta atrás con la base de
# datos de antes; (g) /salud que no responde dos veces: etiqueta fallida, y
# la tercera vez ni se mira el buzón; (h) se conservan 3 versiones y nunca se
# borra la desplegada; (n) sin precompilada: espera y, pasada la espera,
# compila aquí con el cerrojo neutro (que la jail no hereda) y despliega;
# (o) cerrojo ocupado: lo deja para otra pasada sin contar fallo; (p) versión
# de Cargo.toml distinta de la etiqueta; (q) precompilada de otro commit:
# rechazada, compila aquí y despliega; (r) --ref; (s) repositorio
# inaccesible; (j) configuración escribible por otros: no se usa; (k)
# --comprobar no cambia nada; (m) argumentos de más; (t) falta REPO_URL o
# ESCUCHA, o ESCUCHA mal escrita: error claro y no se hace nada; (u) /salud
# y el puerto se miran en la ESCUCHA de la configuración (también IPv6); (l)
# install.sh en una carpeta: plantilla y parada sin configuración, parada si
# falta un valor, y lo que escribe; (v) install.sh con otra jail, otro usuario
# del buzón y el puerto del servidor git por defecto. Todas las
# direcciones son de documentación.
#
# Uso: deploy/pruebas/actualizar.sh (desde cualquier sitio; necesita git,
# python3, tar, file y sha256sum).

set -eu

DEPLOY=$(cd "$(dirname "$0")/.." && pwd)
ACT=$DEPLOY/freebsd/actualizar.sh
OBJ=x86_64-unknown-freebsd

for orden in git python3 sha256sum tar file; do
	command -v "$orden" >/dev/null 2>&1 || {
		echo "FALLO: falta $orden" >&2
		exit 1
	}
done
[ -f "$ACT" ] || {
	echo "FALLO: falta deploy/freebsd/actualizar.sh" >&2
	exit 1
}
# El actualizador usa «set -o pipefail», que tiene el sh de FreeBSD pero no
# el dash de Debian y Ubuntu: ahí se prueba con bash en modo POSIX.
SH_ACT='sh'
if ! sh -c 'set -o pipefail' 2>/dev/null; then
	command -v bash >/dev/null 2>&1 || {
		echo "FALLO: este sh no tiene pipefail y falta bash" >&2
		exit 1
	}
	SH_ACT="bash --posix"
fi

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
BJH=$T/jail-compilar                       # «raíz» de la jail de compilación
CASA_SM=$BJH/home/smbuild
CLON=$CASA_SM/semillero
ORIGEN=$T/origen                           # el repositorio del servidor git
REL=$R/usr/local/semillero/releases        # versiones en el host
CUR=$JR/usr/local/semillero/current
DB=$JR/var/db/semillero
LOG=$R/var/log/semillero-update.log
ESTADO=$R/var/db/semillero-actualizar
BUZ=$R/var/db/semillero-buzon/entrada
LIBEXEC=$R/usr/local/libexec/semillero-actualizar
CONF=$R/usr/local/etc/semillero-actualizar.conf
mkdir -p "$HOME" "$F" "$T/tmpdir" "$T/fuera" "$T/build" "$CASA_SM/.cargo/bin" \
	"$R/usr/local/etc" "$REL" "$JR/usr/local/semillero" "$DB" "$BUZ" "$LIBEXEC" \
	"$R/var/log" "$R/var/run" "$R/var/db"
# El montaje nullfs de releases en la jail: aquí, un enlace.
ln -s "$REL" "$JR/usr/local/semillero/releases"
printf 'base de datos v0\n' >"$DB/semillero.db"
cp "$DEPLOY/lib/git.sh" "$DEPLOY/lib/precompiladas.sh" "$LIBEXEC/"
chmod 644 "$LIBEXEC"/*

# Dónde escucha el semillero de mentira (lo miran fetch y sockstat de
# mentira) y lo que dice la configuración (ESCUCHA).
ESCUCHA=192.0.2.10:8090
echo "$ESCUCHA" >"$T/escucha"

# La configuración, con las claves que ya tenía la de una instalación
# anterior (también REPO_IP y REPO_PUERTO, que el actualizador no usa) y
# ESCUCHA. $1: REPO_URL; $2: líneas que se añaden al final.
escribir_conf()
{
	cat >"$CONF" <<-EOF
		REPO_URL=${1:-$ORIGEN}
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
		${2:-}
	EOF
	chmod 600 "$CONF"
}
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
printf '[[package]]\nname = "wasm-bindgen"\nversion = "0.2.100"\n' >"$ORIGEN/Cargo.lock"
printf '[package]\nname = "semillero"\nversion = "0.0.0"\n' >"$ORIGEN/crates/semillero/Cargo.toml"
git -C "$ORIGEN" add -A
git -C "$ORIGEN" commit -q -m inicio
git clone -q "$ORIGEN" "$CLON"

# Una versión nueva en el origen: commit con la versión $2 (por defecto $1)
# en Cargo.toml y la variante $3 del binario que compilaría el servidor, y
# la etiqueta v$1.
publicar()
{
	printf '[package]\nname = "semillero"\nversion = "%s"\n\n[dependencies]\n' "${2:-$1}" \
		>"$ORIGEN/crates/semillero/Cargo.toml"
	printf '%s\n' "${3:-buena}" >"$ORIGEN/variante"
	git -C "$ORIGEN" add -A
	git -C "$ORIGEN" commit -q -m "v$1"
	git -C "$ORIGEN" tag -a -m "v$1" "v$1"
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

# El PC de compilación deja la versión $1 en el buzón (como
# compilar-freebsd.sh + entregar.sh). $2: variante del binario; $3: COMMIT
# (por defecto, el de la etiqueta).
entregar()
{
	_v=$1
	_b=$T/build/$_v
	_r=$_b/semillero-$_v-$OBJ
	rm -rf "$_b" "${BUZ:?}/$_v"
	mkdir -p "$_r/site/pkg" "$BUZ/$_v"
	binario "$_v" "${2:-buena}" >"$_r/semillero"
	chmod 755 "$_r/semillero"
	printf 'js: abc\nwasm: abc\ncss: abc\n' >"$_r/site/hash.txt"
	for e in js wasm css; do printf '%s %s\n' "$e" "$_v" >"$_r/site/pkg/semillero.abc.$e"; done
	printf '%s\n' "${3:-$(commit_de "$_v")}" >"$_r/COMMIT"
	printf 'licencia\n' >"$_r/LICENSE"
	tar -C "$_b" --sort=name --owner=0 --group=0 --numeric-owner \
		-czf "$BUZ/$_v/semillero-$_v-$OBJ.tar.gz" "semillero-$_v-$OBJ"
	(cd "$BUZ/$_v" && sha256sum "semillero-$_v-$OBJ.tar.gz" >SHA256SUMS)
}

# Deja en el buzón la versión $1 con un paquete peligroso (SHA256SUMS
# válido). $2: absoluta, puntos, enlace, duro o raiz.
entregar_malo()
{
	rm -rf "${BUZ:?}/$1"
	mkdir -p "$BUZ/$1"
	python3 -I - "$BUZ/$1/semillero-$1-$OBJ.tar.gz" "$1" "$2" "$T/fuera" "$T/elf-freebsd" <<'PY'
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
	(cd "$BUZ/$1" && sha256sum "semillero-$1-$OBJ.tar.gz" >SHA256SUMS)
}

# --- órdenes de mentira -----------------------------------------------------
GIT_REAL=$(command -v git)

cat >"$F/lockf" <<EOF
#!/bin/sh
# lockf [-s] [-t N] fichero orden…: aquí no bloquea nada.
# lockf [-s] [-t N] descriptor: el cerrojo neutro de compilación; falla si
# existe la marca compilar-ocupado (75, como el de FreeBSD) o compilar-roto
# (64, un lockf que no sabe de descriptores).
while :; do
	case \$1 in
	-s | -k | -n | -w) shift ;;
	-t) shift 2 ;;
	*) break ;;
	esac
done
if [ "\$#" -eq 1 ]; then
	case \$1 in
	*[!0-9]*) exit 2 ;;
	esac
	echo "descriptor \$1" >>"$T/lockf.log"
	[ ! -e "$T/compilar-roto" ] || exit 64
	[ ! -e "$T/compilar-ocupado" ] || exit 75
	exit 0
fi
shift
# El actualizador se relanza con /bin/sh bajo el cerrojo: aquí, con el sh
# que tiene pipefail.
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

# jexec [-l] [-U usuario] jail orden…: en semillero, service y fetch van a los
# de mentira y /bin/sh corre aquí con las rutas absolutas de sus argumentos
# bajo la raíz de la jail; en semillero-build, /bin/sh -c corre aquí con
# /home/smbuild y el git de la jail cambiados, y apunta si le llega abierto
# el descriptor 9 (el cerrojo del host no debe llegar a la jail).
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
semillero-build)
	[ "\$orden" = /bin/sh ] && [ "\$1" = -c ] || exit 2
	if [ -e /proc/\$\$/fd/9 ]; then echo "descriptor 9 en la jail" >>"$T/fd9.log"; fi
	guion=\$(printf '%s' "\$2" | sed -e "s|/home/smbuild|$CASA_SM|g" -e "s|/usr/local/bin/git|$GIT_REAL|g")
	nombre=\$3
	shift 3
	HOME=$CASA_SM exec /bin/sh -c "\$guion" "\$nombre" "\$@"
	;;
semillero) ;;
*) exit 1 ;;
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

# cargo de la jail de compilación: «compila» la versión de Cargo.toml con la
# variante del fichero «variante» del repositorio. Falla si existe la marca
# cargo-falla.
cat >"$CASA_SM/.cargo/bin/cargo" <<EOF
#!/bin/sh
echo "cargo \$* (CARGO_BUILD_JOBS=\${CARGO_BUILD_JOBS:-})" >>"$T/cargo.log"
[ ! -e "$T/cargo-falla" ] || exit 1
[ "\$*" = "leptos build --release --precompress" ] || exit 0
v=\$(sed -n 's/^version = "\(.*\)"/\1/p' crates/semillero/Cargo.toml | head -n 1)
echo "   Compiling semillero v\$v"
mkdir -p target/release target/site/pkg
{
	cat "$T/elf-freebsd"
	printf '\nsalud-version=%s\ncompilado-aqui\n' "\$v"
	if [ "\$(cat variante 2>/dev/null)" = migra ]; then echo migra; fi
} >target/release/semillero
printf 'js: abc\nwasm: abc\ncss: abc\n' >target/site/hash.txt
for e in js wasm css; do echo "\$e" >target/site/pkg/semillero.abc.\$e; done
EOF
printf '#!/bin/sh\necho "wasm-bindgen 0.2.100"\n' >"$CASA_SM/.cargo/bin/wasm-bindgen"
chmod 755 "$F"/* "$CASA_SM/.cargo/bin"/*

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
	m_cargo=$(lineas "$T/cargo.log")
	m_jexec=$(lineas "$T/jexec.log")
	m_lockf=$(lineas "$T/lockf.log")
}
nuevo()
{
	case $1 in
	log) tail -n +$((m_log + 1)) "$LOG" 2>/dev/null || true ;;
	serv) tail -n +$((m_serv + 1)) "$T/servicio.log" 2>/dev/null || true ;;
	cargo) tail -n +$((m_cargo + 1)) "$T/cargo.log" 2>/dev/null || true ;;
	jexec) tail -n +$((m_jexec + 1)) "$T/jexec.log" 2>/dev/null || true ;;
	lockf) tail -n +$((m_lockf + 1)) "$T/lockf.log" 2>/dev/null || true ;;
	esac
}
compilaciones_nuevas()
{
	nuevo cargo | grep -c 'leptos build' || true
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
empieza "a: primera instalación desde el buzón"
publicar 1.0.0
entregar 1.0.0
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
[ "$(compilaciones_nuevas)" -eq 0 ] || fallo "compila teniendo la precompilada"
[ ! -e "$BUZ/1.0.0" ] || fallo "no vacía el buzón"
nuevo log | grep -q 'instalada la versión precompilada de v1.0.0' || fallo "el registro no dice que instala la precompilada"
nuevo log | grep -q 'desplegada 1.0.0' || fallo "el registro no dice «desplegada 1.0.0»"
[ ! -e "$ESTADO/intentos" ] && [ ! -e "$ESTADO/fallida" ] || fallo "deja intentos o fallida"

# --- (b) la misma etiqueta otra vez -----------------------------------------
empieza "b: misma etiqueta"
marcar
if ! actualizar; then
	cat "$T/salida" >&2
	fallo "falla sin nada nuevo"
fi
sin_cambios releases/1.0.0 "1.0.0 "
[ "$(compilaciones_nuevas)" -eq 0 ] || fallo "compila sin versión nueva"
[ -z "$(nuevo log)" ] || fallo "escribe en el registro sin nada nuevo: $(nuevo log)"
[ ! -s "$T/salida" ] || fallo "escribe algo sin nada nuevo: $(cat "$T/salida")"

# --- (c) SHA256SUMS que falta o está mal hecho -----------------------------
# Con la compilación de respaldo fallando, para ver solo el rechazo.
touch "$T/cargo-falla"
n=1
for mal in falta dos-lineas otro-nombre no-hex; do
	empieza "c: SHA256SUMS $mal"
	v=1.0.$n
	n=$((n + 1))
	publicar "$v"
	entregar "$v"
	s=$BUZ/$v/SHA256SUMS
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
		nuevo log | grep -q "esperando la versión precompilada de v$v" ||
			fallo "el registro no dice que espera: $(nuevo log)"
		[ ! -e "$ESTADO/intentos" ] || fallo "esperar cuenta como fallo de la etiqueta"
		[ "$(compilaciones_nuevas)" -eq 0 ] || fallo "compila mientras espera"
		[ -f "$BUZ/$v/$a" ] || fallo "toca el buzón mientras espera"
		continue
	fi
	if actualizar; then fallo "acepta un SHA256SUMS que $mal"; fi
	sin_cambios releases/1.0.0 "1.0.0 "
	nuevo log | grep -q 'rechazada: SHA256SUMS' || fallo "el registro no habla de SHA256SUMS: $(nuevo log)"
	[ "$(compilaciones_nuevas)" -eq 1 ] || fallo "no intenta compilar aquí tras rechazarla"
	[ ! -e "$BUZ/$v" ] || fallo "deja en el buzón la versión rechazada"
	[ "$(cat "$ESTADO/intentos" 2>/dev/null | tr '\n' ' ')" = "v$v 1 " ] ||
		fallo "intentos no cuenta el fallo de v$v"
done

# --- (d) sha256 malo --------------------------------------------------------
empieza "d: sha256 malo"
publicar 1.1.1
entregar 1.1.1
printf 'basura\n' >>"$BUZ/1.1.1/semillero-1.1.1-$OBJ.tar.gz"
marcar
if actualizar; then fallo "acepta un sha256 que no cuadra"; fi
sin_cambios releases/1.0.0 "1.0.0 "
nuevo log | grep -q 'rechazada: el sha256' || fallo "el registro no habla del sha256: $(nuevo log)"

# --- (e) paquetes peligrosos ------------------------------------------------
n=2
for tipo in absoluta puntos enlace duro raiz; do
	empieza "e: paquete con $tipo"
	v=1.1.$n
	n=$((n + 1))
	publicar "$v"
	entregar_malo "$v" "$tipo"
	marcar
	if actualizar; then fallo "acepta un paquete con $tipo"; fi
	sin_cambios releases/1.0.0 "1.0.0 "
	nuevo log | grep -q 'rechazada' || fallo "el registro no da el rechazo: $(nuevo log)"
done
if [ -n "$(find "$T" -name 'escape-*' -print)" ]; then
	caso="e"
	fallo "un paquete peligroso ha escrito fuera: $(find "$T" -name 'escape-*')"
fi
rm -f "$T/cargo-falla"

# --- (f) /salud con otra versión: vuelta atrás ------------------------------
empieza "f: /salud con otra versión"
publicar 1.2.0
entregar 1.2.0 otra-version
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
entregar 1.3.0 roto
marcar
if actualizar; then fallo "acepta una versión que no arranca"; fi
[ "$(actual)" = releases/1.0.0 ] && [ "$(en_marcha)" = 1.0.0 ] || fallo "no vuelve a 1.0.0 (1.ª vez)"
[ "$(cat "$ESTADO/intentos" 2>/dev/null | tr '\n' ' ')" = "v1.3.0 1 " ] || fallo "intentos tras el 1.er fallo"
[ ! -e "$ESTADO/fallida" ] || fallo "fallida tras un solo fallo"
# El PC la vuelve a entregar.
entregar 1.3.0 roto
marcar
if actualizar; then fallo "acepta una versión que no arranca (2.ª vez)"; fi
[ ! -e "$BUZ/1.3.0" ] || fallo "no usa el buzón en el 2.º intento"
[ "$(actual)" = releases/1.0.0 ] && [ "$(en_marcha)" = 1.0.0 ] || fallo "no vuelve a 1.0.0 (2.ª vez)"
[ "$(cat "$ESTADO/fallida" 2>/dev/null)" = v1.3.0 ] || fallo "fallida no es v1.3.0"
[ ! -e "$ESTADO/intentos" ] || fallo "deja intentos con la etiqueta ya fallida"
nuevo log | grep -q 'v1.3.0 ha fallado 2 veces' || fallo "el registro no dice que v1.3.0 queda fallida"
entregar 1.3.0 roto
marcar
if ! actualizar; then fallo "con la etiqueta fallida no sale bien"; fi
sin_cambios releases/1.0.0 "1.0.0 "
[ -f "$BUZ/1.3.0/SHA256SUMS" ] || fallo "mira el buzón de una etiqueta fallida"
[ "$(compilaciones_nuevas)" -eq 0 ] || fallo "compila una etiqueta fallida"
[ -z "$(nuevo log)" ] || fallo "escribe en el registro con la etiqueta fallida: $(nuevo log)"

# --- (h) se conservan 3 y nunca la desplegada -------------------------------
empieza "h: se conservan 3"
for v in 1.4.0 1.5.0 1.6.0; do
	publicar "$v"
	entregar "$v" migra
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
entregar 1.7.0
actualizar || fallo "no despliega 1.7.0: $(cat "$T/salida")"
[ "$(actual)" = releases/1.7.0 ] || fallo "current no es 1.7.0"
[ "$(versiones)" = "1.7.0 9.0.0 9.0.1 9.0.2 " ] || fallo "borra la desplegada o no poda ($(versiones))"
rm -rf "${REL:?}"/9.0.*

# --- (n) sin precompilada: espera y luego compila aquí -----------------------
empieza "n: compilar en el servidor"
publicar 1.7.1
marcar
actualizar || fallo "falla mientras espera: $(cat "$T/salida")"
sin_cambios releases/1.7.0 "1.7.0 "
[ "$(compilaciones_nuevas)" -eq 0 ] || fallo "compila sin esperar"
[ "$(cut -d' ' -f1 "$ESTADO/precompilada" 2>/dev/null)" = v1.7.1 ] || fallo "no apunta cuándo vio la etiqueta"
nuevo log | grep -q 'esperando la versión precompilada de v1.7.1 (hasta 20 min' || fallo "no dice que espera: $(nuevo log)"
# Pasada la espera.
echo "v1.7.1 1" >"$ESTADO/precompilada"
marcar
actualizar || fallo "no compila ni despliega pasada la espera: $(cat "$T/salida")"
[ "$(actual)" = releases/1.7.1 ] && [ "$(en_marcha)" = 1.7.1 ] || fallo "no despliega 1.7.1 ($(actual))"
grep -qax compilado-aqui "$REL/1.7.1/semillero" || fallo "lo desplegado no es lo compilado aquí"
[ "$(cat "$REL/1.7.1/COMMIT" 2>/dev/null)" = "$(commit_de 1.7.1)" ] || fallo "COMMIT no es el de la etiqueta"
[ "$(stat -c %a "$REL/1.7.1/semillero" "$REL/1.7.1/COMMIT" 2>/dev/null | tr '\n' ' ')" = "755 644 " ] ||
	fallo "permisos de lo compilado"
nuevo log | grep -q 'no ha llegado la versión precompilada de v1.7.1; se compila aquí' || fallo "no dice por qué compila"
[ "$(compilaciones_nuevas)" -eq 1 ] || fallo "compila $(compilaciones_nuevas) veces"
nuevo cargo | grep -q 'CARGO_BUILD_JOBS=2' || fallo "no pasa TRABAJOS_COMPILACION a cargo"
[ "$(nuevo lockf)" = "descriptor 9" ] || fallo "no toma el cerrojo neutro con el descriptor 9: $(nuevo lockf)"
[ -f "$R/var/run/compilar.lock" ] || fallo "no crea /var/run/compilar.lock"
[ ! -e "$T/fd9.log" ] || fallo "la jail de compilación hereda el cerrojo del host"
[ -s "$R/var/log/semillero-build.log" ] || fallo "no escribe el registro de compilación"
[ ! -e "$ESTADO/precompilada" ] || fallo "un despliegue bueno no limpia precompilada"

# --- (o) cerrojo neutro ocupado ----------------------------------------------
empieza "o: cerrojo ocupado"
publicar 1.7.2 1.7.2 migra
echo "v1.7.2 1" >"$ESTADO/precompilada"
touch "$T/compilar-ocupado"
marcar
actualizar || fallo "con el cerrojo ocupado no sale bien: $(cat "$T/salida")"
sin_cambios releases/1.7.1 "1.7.0 1.7.1 "
[ "$(compilaciones_nuevas)" -eq 0 ] || fallo "compila con el cerrojo ocupado"
nuevo log | grep -q 'hay otra compilación en marcha en el servidor; se reintenta en la próxima pasada' ||
	fallo "no dice que hay otra compilación: $(nuevo log)"
[ ! -e "$ESTADO/intentos" ] && [ ! -e "$ESTADO/fallida" ] || fallo "el cerrojo ocupado cuenta como fallo"
[ "$(cat "$ESTADO/precompilada" 2>/dev/null)" = "v1.7.2 1" ] || fallo "cambia el estado de la espera"
rm -f "$T/compilar-ocupado"
# Un lockf que falla por otra cosa no pasa por «ocupado»: error a la vista.
touch "$T/compilar-roto"
marcar
if actualizar; then fallo "con lockf roto sale bien"; fi
[ "$(compilaciones_nuevas)" -eq 0 ] || fallo "compila con lockf roto"
nuevo log | grep -q 'lockf no puede tomar el cerrojo de compilación' || fallo "no dice que lockf falla: $(nuevo log)"
if nuevo log | grep -q 'hay otra compilación en marcha'; then fallo "lockf roto pasa por cerrojo ocupado"; fi
[ ! -e "$ESTADO/intentos" ] && [ ! -e "$ESTADO/fallida" ] || fallo "lockf roto cuenta como fallo de la etiqueta"
rm -f "$T/compilar-roto"
marcar
actualizar || fallo "no compila con el cerrojo libre: $(cat "$T/salida")"
[ "$(actual)" = releases/1.7.2 ] && [ "$(en_marcha)" = 1.7.2 ] || fallo "no despliega 1.7.2"
grep -q 'migrada por 1.7.2' "$DB/semillero.db" || fallo "la variante del repositorio no llega al binario compilado"

# --- (p) Cargo.toml con otra versión ------------------------------------------
empieza "p: Cargo.toml con otra versión"
publicar 1.7.3 1.7.4
entregar 1.7.3
marcar
if actualizar; then fallo "acepta una etiqueta cuyo Cargo.toml dice otra versión"; fi
sin_cambios releases/1.7.2 "1.7.0 1.7.1 1.7.2 "
nuevo log | grep -q 'crates/semillero/Cargo.toml en v1.7.3 es «1.7.4»' || fallo "no dice qué versión trae: $(nuevo log)"
[ "$(cat "$ESTADO/intentos" 2>/dev/null | tr '\n' ' ')" = "v1.7.3 1 " ] || fallo "no cuenta el fallo"
[ "$(compilaciones_nuevas)" -eq 0 ] || fallo "compila una etiqueta con otra versión"

# --- (q) precompilada de otro commit: compila aquí -----------------------------
empieza "q: precompilada de otro commit"
publicar 1.7.5
entregar 1.7.5 buena "$(commit_de 1.7.3)"
marcar
actualizar || fallo "no despliega lo compilado aquí: $(cat "$T/salida")"
nuevo log | grep -q 'rechazada: el paquete es de otro commit' || fallo "no dice por qué la rechaza: $(nuevo log)"
[ "$(compilaciones_nuevas)" -eq 1 ] || fallo "no compila aquí"
[ "$(actual)" = releases/1.7.5 ] || fallo "no despliega 1.7.5"
grep -qax compilado-aqui "$REL/1.7.5/semillero" || fallo "despliega la precompilada mala"
[ ! -e "$BUZ/1.7.5" ] || fallo "deja la precompilada mala en el buzón"
[ ! -e "$ESTADO/intentos" ] || fallo "deja intentos tras desplegar"

# --- (r) --ref ------------------------------------------------------------------
empieza "r: --ref"
git -C "$ORIGEN" checkout -q -b prueba
printf '[package]\nname = "semillero"\nversion = "1.8.0"\n' >"$ORIGEN/crates/semillero/Cargo.toml"
git -C "$ORIGEN" commit -q -am prueba
git -C "$ORIGEN" checkout -q main
sha=$(git -C "$ORIGEN" rev-parse prueba | cut -c1-12)
echo "v9.9.9" >"$ESTADO/fallida"
entregar 1.8.0 buena "$(git -C "$ORIGEN" rev-parse prueba)"
marcar
actualizar --ref prueba || fallo "--ref falla: $(cat "$T/salida")"
[ "$(actual)" = "releases/1.8.0-$sha" ] && [ "$(en_marcha)" = 1.8.0 ] || fallo "--ref no despliega 1.8.0-$sha ($(actual))"
[ "$(compilaciones_nuevas)" -eq 1 ] || fallo "--ref no compila aquí"
[ -f "$BUZ/1.8.0/SHA256SUMS" ] || fallo "--ref mira el buzón"
[ "$(cat "$ESTADO/fallida")" = v9.9.9 ] || fallo "--ref toca fallida"
rm -rf "${BUZ:?}/1.8.0" "$ESTADO/fallida"
# La siguiente pasada vuelve a la última etiqueta.
marcar
actualizar || fallo "no vuelve a la última etiqueta: $(cat "$T/salida")"
[ "$(actual)" = releases/1.7.5 ] || fallo "tras --ref no vuelve a 1.7.5 ($(actual))"
[ "$(compilaciones_nuevas)" -eq 0 ] || fallo "recompila una versión que ya estaba en releases"

# --- (s) repositorio inaccesible ------------------------------------------------
empieza "s: sin repositorio"
escribir_conf "$T/no-existe"
marcar
actualizar || fallo "sin repositorio no sale bien: $(cat "$T/salida")"
sin_cambios releases/1.7.5 "$(versiones)"
nuevo log | grep -q 'sin acceso al repositorio' || fallo "no dice que no llega al repositorio"
escribir_conf

# --- (j) configuración escribible por otros ---------------------------------
empieza "j: configuración insegura"
publicar 1.8.0
entregar 1.8.0
chmod 620 "$CONF"
v_antes=$(versiones)
marcar
if actualizar; then fallo "usa una configuración escribible por el grupo"; fi
sin_cambios releases/1.7.5 "$v_antes"
[ -z "$(nuevo jexec)" ] || fallo "entra en alguna jail con una configuración insegura"
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
grep -q '1.8.0.*en el buzón: sí' "$T/salida" || fallo "--comprobar no dice la versión nueva: $(cat "$T/salida")"
[ -d "$REL/.9.9.9.tmp" ] || fallo "--comprobar borra restos de otra ejecución"
rm -rf "$REL/.9.9.9.tmp"
sin_cambios releases/1.7.5 "$v_antes"
[ "$(compilaciones_nuevas)" -eq 0 ] || fallo "--comprobar compila"
[ -f "$BUZ/1.8.0/SHA256SUMS" ] || fallo "--comprobar toca el buzón"
git -C "$CLON" rev-parse -q --verify refs/tags/v1.8.0 >/dev/null && fallo "--comprobar trae las etiquetas al clon"
[ -z "$(nuevo log)" ] || fallo "--comprobar escribe en el registro"

# --- (m) argumentos de más ----------------------------------------------------
empieza "m: argumentos de más"
marcar
for a in "--comprobar --otro" "--comprobar extra" "-- --comprobar" "--ref" "--ref -x" \
	"--ref a b" "--comprobar --ref x" "--ref ../x"; do
	# shellcheck disable=SC2086
	if actualizar $a; then
		fallo "acepta «$a»"
	fi
	grep -q 'uso:' "$T/salida" || fallo "«$a» no dice el uso: $(cat "$T/salida")"
done
if actualizar "" --comprobar; then
	fallo "acepta «'' --comprobar»"
fi
sin_cambios releases/1.7.5 "$v_antes"
[ -z "$(nuevo jexec)" ] || fallo "con argumentos de más entra en alguna jail"

# --- (t) falta un valor obligatorio -----------------------------------------
empieza "t: falta un valor obligatorio"
for clave in REPO_URL ESCUCHA; do
	# La configuración sin esa línea (vacía, como si nunca se hubiera puesto).
	escribir_conf
	grep -v "^$clave=" "$CONF" >"$T/conf" && cat "$T/conf" >"$CONF"
	marcar
	if actualizar; then fallo "sale bien sin $clave"; fi
	sin_cambios releases/1.7.5 "$v_antes"
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
entregar 1.8.1
marcar
if actualizar; then fallo "despliega preguntando /salud en otra dirección"; fi
[ "$(actual)" = releases/1.7.5 ] || fallo "no vuelve a 1.7.5 ($(actual))"
# Con la de la configuración, sí.
ESCUCHA=198.51.100.7:9000
escribir_conf
rm -f "$ESTADO/intentos"
entregar 1.8.1
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
entregar 1.8.2
: >"$T/fetch.log"
actualizar || fallo "no despliega con ESCUCHA=$ESCUCHA: $(cat "$T/salida")"
grep -qxF 'http://[2001:db8::7]:9000/salud' "$T/fetch.log" || fallo "no pregunta /salud en $ESCUCHA: $(cat "$T/fetch.log")"
v_antes=$(versiones)

# --- (l) install.sh en una carpeta -----------------------------------------
empieza "l: install.sh"

# Configuración de install.sh $1; $3: líneas de más ($2 no se usa).
conf_install()
{
	mkdir -p "$1/usr/local/etc"
	_git='REPO_IP=192.0.2.20
REPO_PUERTO=2200'
	cat >"$1/usr/local/etc/semillero-actualizar.conf" <<-EOF
		REPO_URL=ssh://git@192.0.2.20:2200/usuario/semillero.git
		ESCUCHA=192.0.2.10:8090
		JAIL_COMPILAR_IP=192.0.2.30
		$_git
		DNS_IP=192.0.2.1
		RED_LAN='{ 192.0.2.0/24 198.51.100.0/24 }'
		INTERFAZ_EXT=em0
		${3:-}
	EOF
	chmod 600 "$1/usr/local/etc/semillero-actualizar.conf"
}

# Sin configuración: escribe la plantilla y para sin instalar nada más.
I=$T/inst
mkdir -p "$I/usr/local/jails/containers/semillero"
if sh "$DEPLOY/freebsd/install.sh" --prefijo "$I" >"$T/salida" 2>&1; then
	fallo "install.sh sale bien sin configuración"
fi
grep -q 'ERROR: faltan en /usr/local/etc/semillero-actualizar.conf: REPO_URL ESCUCHA JAIL_COMPILAR_IP DNS_IP RED_LAN;' "$T/salida" ||
	fallo "no dice qué falta en la configuración: $(cat "$T/salida")"
[ "$(stat -c %a "$I/usr/local/etc/semillero-actualizar.conf" 2>&1)" = 600 ] || fallo "no escribe la plantilla con 600"
for clave in REPO_URL= ESCUCHA= JAIL=semillero JAIL_RAIZ= JAIL_COMPILAR=semillero-build \
	JAIL_COMPILAR_IP= REPO_IP= REPO_PUERTO= DNS_IP= RED_LAN= INTERFAZ_EXT= \
	PRECOMPILADAS_ESPERA=20 TRABAJOS_COMPILACION= USUARIO_COMPILAR=smbuild; do
	grep -qx "$clave" "$I/usr/local/etc/semillero-actualizar.conf" || fallo "la plantilla no tiene $clave"
done
[ ! -e "$I/usr/local/sbin" ] && [ ! -e "$I/usr/local/etc/cron.d" ] && [ ! -e "$I/var" ] ||
	fallo "sin configuración instala algo más que la plantilla: $(cd "$I" && find . -mindepth 1 | tr '\n' ' ')"

# Con una configuración a la que le falta un valor: dice cuál y no instala.
I=$T/inst-falta
mkdir -p "$I/usr/local/jails/containers/semillero"
conf_install "$I" nuevas
grep -v '^ESCUCHA=' "$I/usr/local/etc/semillero-actualizar.conf" >"$T/conf"
cat "$T/conf" >"$I/usr/local/etc/semillero-actualizar.conf"
if sh "$DEPLOY/freebsd/install.sh" --prefijo "$I" >"$T/salida" 2>&1; then
	fallo "install.sh sale bien sin ESCUCHA"
fi
grep -q 'ERROR: faltan en /usr/local/etc/semillero-actualizar.conf: ESCUCHA;' "$T/salida" ||
	fallo "no dice que falta ESCUCHA: $(cat "$T/salida")"
[ ! -e "$I/usr/local/sbin" ] || fallo "sin ESCUCHA instala el actualizador"

I=$T/inst
conf_install "$I" nuevas
if ! sh "$DEPLOY/freebsd/install.sh" --prefijo "$I" >"$T/salida" 2>&1; then
	cat "$T/salida" >&2
	fallo "install.sh falla"
fi
cmp -s "$ACT" "$I/usr/local/sbin/semillero-actualizar" || fallo "no instala el actualizador"
L=usr/local/libexec/semillero-actualizar
cmp -s "$DEPLOY/lib/git.sh" "$I/$L/git.sh" && cmp -s "$DEPLOY/lib/precompiladas.sh" "$I/$L/precompiladas.sh" &&
	cmp -s "$DEPLOY/freebsd/recibir-precompilada.sh" "$I/$L/recibir-precompilada.sh" ||
	fallo "no instala lo de libexec"
modos=$(cd "$I" && stat -c '%a' usr/local/sbin/semillero-actualizar \
	usr/local/etc/semillero-actualizar.conf usr/local/etc/cron.d/semillero \
	usr/local/etc/newsyslog.conf.d/semillero.conf usr/local/semillero/releases \
	var/db/semillero-actualizar \
	usr/local/jails/containers/semillero/usr/local/semillero/releases \
	$L/git.sh $L/precompiladas.sh $L/recibir-precompilada.sh \
	var/db/semillero-buzon var/db/semillero-buzon/entrada 2>&1 | tr '\n' ' ')
[ "$modos" = "755 600 644 644 755 700 755 644 644 755 755 750 " ] || fallo "permisos de lo instalado: $modos"
grep -qx '\*/15	\*	\*	\*	\*	root	/usr/local/sbin/semillero-actualizar' \
	"$I/usr/local/etc/cron.d/semillero" || fallo "la línea de cron no es la esperada"
grep -q '^/var/log/semillero-update.log	' "$I/usr/local/etc/newsyslog.conf.d/semillero.conf" ||
	fallo "falta la línea de newsyslog"
grep -qxF '     /usr/local/semillero/releases	/usr/local/jails/containers/semillero/usr/local/semillero/releases	nullfs	ro	0	0' "$T/salida" ||
	fallo "no escribe la línea de /etc/fstab.semillero"
grep -qF 'mount -t nullfs -o ro /usr/local/semillero/releases /usr/local/jails/containers/semillero/usr/local/semillero/releases' "$T/salida" ||
	fallo "no escribe la orden de montaje"
grep -qF 'ip4.addr = 192.0.2.30;' "$T/salida" || fallo "no escribe el bloque de jail.conf"
grep -qF 'pass quick proto tcp from 192.0.2.30 to 192.0.2.20 port 2200' "$T/salida" ||
	fallo "no escribe la regla de pf del servidor git"
grep -qF 'from 192.0.2.30 to 192.0.2.1 port 53' "$T/salida" || fallo "no escribe la regla de pf del DNS"
grep -qF 'block quick from 192.0.2.30 to { 192.0.2.0/24 198.51.100.0/24 }' "$T/salida" || fallo "no escribe el bloqueo de la red local"
grep -qF 'nat on em0 from 192.0.2.30 to any tag SEMILLERO_BUILD -> (em0)' "$T/salida" || fallo "no escribe el nat por em0"
grep -qF 'restrict,from="IP_DEL_PC",command="/usr/local/libexec/semillero-actualizar/recibir-precompilada.sh"' "$T/salida" ||
	fallo "no escribe la línea de authorized_keys del buzón"
grep -qF 'pw useradd smentrega -u 3171' "$T/salida" || fallo "no escribe cómo crear el usuario del buzón"
grep -qF 'escuchando en 192.0.2.10:8090' "$T/salida" || fallo "no dice dónde tiene que escuchar el servicio"
grep -q '@[A-Z_]*@' "$T/salida" && fallo "deja huecos de plantilla sin rellenar"
# Solo direcciones de documentación (y 127.0.0.1) en lo que escribe.
ips=$(grep -ohE '[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+' "$T/salida" "$I/usr/local/etc/semillero-actualizar.conf" \
	"$DEPLOY/freebsd/install.sh" | grep -vE '^(192\.0\.2|198\.51\.100|203\.0\.113)\.|^127\.0\.0\.1$' | sort -u | tr '\n' ' ')
[ -z "$ips" ] || fallo "escribe direcciones que no son de documentación: $ips"
echo '# cambiada' >>"$I/usr/local/etc/semillero-actualizar.conf"
sh "$DEPLOY/freebsd/install.sh" --prefijo "$I" >"$T/salida" 2>&1 || fallo "install.sh falla la 2.ª vez"
grep -qx '# cambiada' "$I/usr/local/etc/semillero-actualizar.conf" || fallo "install.sh pisa la configuración"

# --- (v) otra jail, otro buzón y el puerto por defecto --------------------------------
empieza "v: otra jail, otro buzón y puerto por defecto"
I=$T/inst-otra
mkdir -p "$I/usr/local/jails/containers/torre"
conf_install "$I" nuevas 'JAIL=torre
USUARIO_BUZON=buzon
UID_BUZON=4001'
if ! sh "$DEPLOY/freebsd/install.sh" --prefijo "$I" >"$T/salida" 2>&1; then
	cat "$T/salida" >&2
	fallo "install.sh falla con otra jail y otro buzón"
fi
grep -qF 'pass quick proto tcp from 192.0.2.30 to 192.0.2.20 port 2200' "$T/salida" ||
	fallo "no toma REPO_IP y REPO_PUERTO"
grep -qF '/usr/local/jails/containers/torre/usr/local/semillero/releases' "$T/salida" ||
	fallo "no toma la raíz de la jail de JAIL"
grep -qF 'pw useradd buzon -u 4001' "$T/salida" || fallo "no toma USUARIO_BUZON y UID_BUZON"
# Sin puerto: el de ssh.
conf_install "$I" nuevas 'JAIL=torre
REPO_PUERTO='
sh "$DEPLOY/freebsd/install.sh" --prefijo "$I" >"$T/salida" 2>&1 || fallo "install.sh falla sin REPO_PUERTO"
grep -qE 'to 192\.0\.2\.20 port 22$' "$T/salida" || fallo "sin REPO_PUERTO no usa el 22"

# --- (w) repositorio en Internet: sin REPO_IP ---------------------------------
empieza "w: repositorio en Internet, sin REPO_IP"
I=$T/inst-internet
mkdir -p "$I/usr/local/jails/containers/semillero"
conf_install "$I" nuevas 'REPO_URL=https://git.example/usuario/semillero.git
REPO_IP=
REPO_PUERTO='
if ! sh "$DEPLOY/freebsd/install.sh" --prefijo "$I" >"$T/salida" 2>&1; then
	cat "$T/salida" >&2
	fallo "install.sh pide REPO_IP con el repositorio en Internet"
fi
grep -q 'pass quick proto tcp from 192.0.2.30 to ' "$T/salida" &&
	fallo "sin REPO_IP escribe una regla de pf hacia el servidor git: $(grep 'pass quick proto tcp' "$T/salida")"
grep -qF 'from 192.0.2.30 to 192.0.2.1 port 53' "$T/salida" || fallo "sin REPO_IP no escribe la regla del DNS"
grep -qF 'block quick from 192.0.2.30 to { 192.0.2.0/24 198.51.100.0/24 }' "$T/salida" ||
	fallo "sin REPO_IP no bloquea la red local"
grep -q '@[A-Z_]*@' "$T/salida" && fallo "sin REPO_IP deja huecos de plantilla"

if [ "$fallos" -ne 0 ]; then
	echo "$fallos fallos en $casos casos" >&2
	exit 1
fi
echo "OK ($casos casos)"
