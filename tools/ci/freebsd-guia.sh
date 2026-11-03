#!/bin/sh
# Sigue la guía de FreeBSD (docs/instalar/freebsd.md) paso a paso, como la
# seguiría alguien en su host, con el paquete de FreeBSD que hace la CI igual
# que release.yml: la jail, el usuario semillero, el paquete en
# releases/X.Y.Z con «current», el rc.d del paquete, «semillero configurar»
# con las respuestas por la entrada estándar, sysrc y «service semillero
# start». Luego comprueba que /salud contesta en 127.0.0.1 con la versión,
# que escucha el usuario semillero y que «service semillero status»,
# «restart» y «stop» funcionan. Lo usa la CI (.github/workflows/freebsd.yml)
# dentro de una máquina virtual de FreeBSD; tira lo que toca del sistema
# (/etc/jail.conf, una jail), así que no es para un host de verdad.
#
#   sh tools/ci/freebsd-guia.sh CARPETA
#
# CARPETA  donde están semillero-X.Y.Z-x86_64-unknown-freebsd.tar.gz y su
#          SHA256SUMS (los de packaging/empaquetar.sh).
#
# Lo que cambia respecto a la guía:
# - La jail no se crea con «bsdinstall jail», que pregunta en pantalla, sino
#   como lo hace bsdinstall eligiendo solo «base»: base.txz de la misma
#   versión que el host, desempaquetado en la carpeta de la jail.
# - Sin red de verdad: la jail tiene 127.0.0.1 (sin «interface»: la dirección
#   ya está en lo0) y semillero escucha en 127.0.0.1:8090. No hay
#   qBittorrent: /salud no le pregunta, y «semillero configurar» se guarda
#   «de todos modos».
# - El paquete no se descarga de GitHub (no es una versión publicada) ni se
#   comprueba su atestación: se copia con su SHA256SUMS y se comprueba el
#   sha256 como dice la guía.
#
# Sale con 0 si todo va bien; si no, dice qué falla, con el registro de
# semillero, y sale con 1.

set -eu

error()
{
	echo "freebsd-guia: $*" >&2
	if [ -n "${J:-}" ] && [ -f "$J/var/log/semillero-daemon.log" ]; then
		echo "--- $J/var/log/semillero-daemon.log" >&2
		tail -n 40 "$J/var/log/semillero-daemon.log" >&2
	fi
	exit 1
}

paso()
{
	echo "== $*"
}

# Espera hasta 60 s a que /salud conteste con "ok":true y la versión $V.
salud_responde()
{
	_n=0
	while [ "$_n" -lt 60 ]; do
		if _s=$(fetch -q -T 2 -o - "http://$ESCUCHA/salud" 2>/dev/null) &&
			printf '%s' "$_s" | grep -q '"ok":true' &&
			printf '%s' "$_s" | grep -q "\"version\":\"$V\""; then
			echo "/salud: $_s"
			return 0
		fi
		sleep 1
		_n=$((_n + 1))
	done
	return 1
}

[ "$#" -eq 1 ] || {
	sed -n '/^#   sh tools/s/^#   //p' "$0" >&2
	exit 2
}
[ "$(uname -s)" = FreeBSD ] || error "esto es para FreeBSD"
[ "$(id -u)" -eq 0 ] || error "hace falta root (como en la guía)"
CARPETA=$(cd "$1" && pwd) || error "no existe la carpeta $1"
set -- "$CARPETA"/semillero-*-x86_64-unknown-freebsd.tar.gz
if [ "$#" -ne 1 ] || [ ! -f "$1" ]; then
	error "en $CARPETA tiene que haber un solo paquete de FreeBSD"
fi
[ -f "$CARPETA/SHA256SUMS" ] || error "falta $CARPETA/SHA256SUMS"
V=${1##*/semillero-}
V=${V%-x86_64-unknown-freebsd.tar.gz}
ESCUCHA=127.0.0.1:8090
echo "semillero $V en FreeBSD $(freebsd-version -u)"

# --- 1. La jail ----------------------------------------------------------------
paso "1. La jail"
mkdir -p /usr/local/jails/containers
J=/usr/local/jails/containers/semillero
mkdir -p "$J"
version_base=$(freebsd-version -u | sed 's/-p[0-9]*$//')
fetch -q -o /tmp/base.txz "https://download.freebsd.org/releases/$(uname -m)/$version_base/base.txz" ||
	error "no se puede descargar base.txz de $version_base"
tar -xf /tmp/base.txz -C "$J"
rm -f /tmp/base.txz
cat >>/etc/jail.conf <<EOF
semillero {
	path = "$J";
	host.hostname = "semillero";
	ip4.addr = 127.0.0.1;
	exec.start = "/bin/sh /etc/rc";
	exec.stop = "/bin/sh /etc/rc.shutdown";
	exec.clean;
	mount.devfs;
}
EOF
sysrc jail_enable=YES
service jail start semillero
jexec semillero pw useradd semillero -c semillero -d /nonexistent -s /usr/sbin/nologin

# --- 2. Descargar y comprobar --------------------------------------------------
paso "2. Comprobar el paquete"
P=semillero-$V-x86_64-unknown-freebsd
cd "$(mktemp -d)"
cp "$CARPETA/$P.tar.gz" "$CARPETA/SHA256SUMS" .
sha256 -c "$(awk -v f="$P.tar.gz" '$2 == f { print $1 }' SHA256SUMS)" "$P.tar.gz" ||
	error "el sha256 de $P.tar.gz no cuadra con SHA256SUMS"

# --- 3. Instalar en la jail ----------------------------------------------------
paso "3. Instalar en la jail"
mkdir -p "$J/usr/local/semillero/releases"
tar -xzf "$P.tar.gz" -C "$J/usr/local/semillero/releases"
mv "$J/usr/local/semillero/releases/$P" "$J/usr/local/semillero/releases/$V"
ln -sfh "releases/$V" "$J/usr/local/semillero/current"

# --- 4. El arranque (rc.d) -----------------------------------------------------
paso "4. El arranque (rc.d)"
mkdir -p "$J/usr/local/etc/rc.d"
install -m 555 "$J/usr/local/semillero/releases/$V/rc.d/semillero" \
	"$J/usr/local/etc/rc.d/semillero"

# --- 5. Configurar y arrancar --------------------------------------------------
paso "5. Configurar y arrancar"
jexec semillero install -d -o semillero -m 700 /var/db/semillero
# Orden de las preguntas (crates/semillero/src/configurar.rs): dónde
# escucha, dirección en el navegador, qBittorrent, modo (1: con contraseña),
# usuario, contraseña, proxy, Jellyfin, fichero de estado, base de datos y,
# como qBittorrent no responde, si se guarda de todos modos.
printf '%s\n' "$ESCUCHA" '' '' 1 '' prueba-de-la-ci '' '' '' '' s |
	jexec semillero /usr/local/semillero/current/semillero configurar \
		--env /usr/local/etc/semillero/semillero.env --secretos /var/db/semillero ||
	error "semillero configurar falla"
envf=$J/usr/local/etc/semillero/semillero.env
[ -f "$envf" ] || error "configurar no escribe $envf"
grep -qx "SEMILLERO_LISTEN=$ESCUCHA" "$envf" || error "$envf no dice SEMILLERO_LISTEN=$ESCUCHA: $(cat "$envf")"
grep -qx 'SEMILLERO_QBT_CLAVE=/var/db/semillero/qbt.clave' "$envf" || error "$envf no apunta a qbt.clave: $(cat "$envf")"
[ "$(stat -f '%u %Lp' "$envf")" = "0 600" ] || error "$envf no es de root con permisos 600: $(stat -f '%Su %Lp' "$envf")"
# Dentro de la jail: el usuario semillero solo existe en su /etc/passwd.
[ "$(jexec semillero stat -f '%Su %Lp' /var/db/semillero/qbt.clave)" = "semillero 600" ] ||
	error "qbt.clave no es solo del usuario semillero"

jexec semillero sysrc semillero_enable=YES
jexec semillero service semillero start || error "service semillero start falla"
salud_responde || error "/salud no contesta con la versión $V en $ESCUCHA"
tail "$J/var/log/semillero-daemon.log"
grep -q "semillero escuchando en http://$ESCUCHA" "$J/var/log/semillero-daemon.log" ||
	error "el registro no dice «semillero escuchando en http://$ESCUCHA»"
# Escucha el usuario sin privilegios, no root.
usuario=$(jexec semillero sockstat -4 -l -p 8090 | awk 'NR > 1 { print $1 }' | sort -u)
[ "$usuario" = semillero ] || error "en el puerto 8090 escucha «$usuario», no semillero: $(jexec semillero sockstat -4 -l -p 8090)"
[ -f "$J/var/db/semillero/semillero.db" ] || error "no hay base de datos en /var/db/semillero de la jail"

# --- status, restart y stop ----------------------------------------------------
paso "status, restart y stop"
jexec semillero service semillero status || error "service semillero status no lo da en marcha"
jexec semillero service semillero restart || error "service semillero restart falla"
salud_responde || error "/salud no contesta tras restart"
jexec semillero service semillero stop || error "service semillero stop falla"
if jexec semillero service semillero status; then
	error "service semillero status lo da en marcha tras stop"
fi
n=0
while fetch -q -T 2 -o /dev/null "http://$ESCUCHA/salud" 2>/dev/null; do
	n=$((n + 1))
	[ "$n" -lt 10 ] || error "/salud sigue contestando 10 s después de stop"
	sleep 1
done
echo "freebsd-guia: todo bien (semillero $V, FreeBSD $(freebsd-version -u))"
