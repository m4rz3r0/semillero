#!/bin/sh
# shellcheck disable=SC2016 # guiones que se expanden en la jail de compilación
#
# Instala en el host (FreeBSD, como root) el actualizador de semillero. Se
# pasa desde una copia del repositorio en el servidor; es idempotente: se
# puede repetir y solo hace lo que falte.
#
#   sh deploy/freebsd/install.sh [--prefijo DIR]
#
# Hace:
# - /usr/local/sbin/semillero-actualizar (deploy/freebsd/actualizar.sh) y, en
#   /usr/local/libexec/semillero-actualizar, git.sh y precompiladas.sh
#   (deploy/lib/) y recibir-precompilada.sh (la orden forzada del buzón);
#   todo root:wheel;
# - /usr/local/etc/semillero-actualizar.conf (root, 0600) con los valores por
#   defecto, solo si no existe (si existe, se usan los suyos);
# - /usr/local/etc/cron.d/semillero (cada 15 minutos) y
#   /usr/local/etc/newsyslog.conf.d/semillero.conf (rota
#   /var/log/semillero-update.log);
# - /usr/local/semillero/releases en el host (root:wheel 0755),
#   /var/db/semillero-actualizar (0700), el punto de montaje
#   <jail>/usr/local/semillero/releases dentro de la jail del servicio
#   (JAIL) y /var/db/semillero-buzon (root) con su entrada/ (del usuario del
#   buzón, si ya existe);
# - si la jail de compilación (JAIL_COMPILAR) está en marcha: su
#   resolv.conf, git, binaryen y rustup, el usuario USUARIO_COMPILAR, su
#   clave de despliegue, el clon del repositorio y cargo-leptos (la Rust la
#   fija rust-toolchain.toml del repositorio, y rustup la instala la primera
#   vez que se compila).
# No toca /etc/jail.conf, /etc/pf.conf ni /etc/fstab.<jail>, no crea la
# jail ni el usuario del buzón: lo que queda por hacer a mano lo escribe al
# final, ya rellenado con la configuración.
#
# Si la configuración no existe, escribe la plantilla y para; si le falta un
# valor obligatorio, dice cuál y para, antes de instalar nada más.
#
# --prefijo DIR (o SEMILLERO_ACT_RAIZ): lo instala todo bajo DIR, sin pedir
# root, sin cambiar dueños y sin tocar ninguna jail; es para las pruebas
# (deploy/pruebas/actualizar.sh).

set -eu

DIR=$(cd "$(dirname "$0")" && pwd)
DEPLOY_DIR=$(dirname "$DIR")
P=${SEMILLERO_ACT_RAIZ:-}
case "$#:${1:-}" in
2:--prefijo)
	P=${2:?falta la carpeta tras --prefijo}
	;;
0:) ;;
*)
	echo "uso: $0 [--prefijo DIR]" >&2
	exit 2
	;;
esac

if [ -z "$P" ]; then
	if [ "$(id -u)" -ne 0 ]; then
		echo "ERROR: hay que pasarlo como root" >&2
		exit 1
	fi
	if [ "$(uname -s)" != FreeBSD ]; then
		echo "ERROR: es para el host FreeBSD (para probarlo aquí, --prefijo DIR)" >&2
		exit 1
	fi
fi

CONF=/usr/local/etc/semillero-actualizar.conf
LIBEXEC=/usr/local/libexec/semillero-actualizar
BUZON_CASA=/var/db/semillero-buzon
# La que usa también deploy/cross/compilar-freebsd.sh en el PC.
CARGO_LEPTOS_VERSION=0.3.11
# Valores por defecto (los mismos que actualizar.sh); si ya hay
# configuración, mandan los suyos. Los vacíos de OBLIGATORIAS no tienen
# valor por defecto: son de cada instalación.
OBLIGATORIAS="REPO_URL ESCUCHA JAIL_COMPILAR_IP DNS_IP RED_LAN"
REPO_URL=
REPO_HOST_CLAVE=
ESCUCHA=
JAIL=semillero
JAIL_RAIZ=
JAIL_COMPILAR=semillero-build
JAIL_COMPILAR_RAIZ=
JAIL_COMPILAR_IP=
REPO_IP=
REPO_PUERTO=
DNS_IP=
RED_LAN=
INTERFAZ_EXT=
USUARIO_COMPILAR=smbuild
UID_COMPILAR=3170
USUARIO_BUZON=smentrega
UID_BUZON=3171

say()
{
	echo "==> $*"
}

# install con dueño root:wheel salvo en las pruebas (sin root).
instalar()
{
	if [ "$(id -u)" -eq 0 ]; then
		install -o root -g wheel "$@"
	else
		install "$@"
	fi
}

for f in "$DIR/actualizar.sh" "$DIR/recibir-precompilada.sh" "$DEPLOY_DIR/lib/git.sh" \
	"$DEPLOY_DIR/lib/precompiladas.sh"; do
	sh -n "$f" || {
		echo "ERROR: error de sintaxis en $f" >&2
		exit 1
	}
done

# La configuración va antes que nada: si le falta algo obligatorio, no se
# instala ni se cambia nada más (el actualizador que ya hubiera sigue).
instalar -d -m 0755 "$P/usr/local/etc"
if [ -e "$P$CONF" ]; then
	say "$CONF ya existe; no se toca"
	# shellcheck source=/dev/null
	. "$P$CONF"
else
	say "$CONF (plantilla)"
	_t=$(mktemp)
	cat >"$_t" <<-'EOF'
		# Configuración del actualizador de semillero (semillero-actualizar).
		# Shell: VARIABLE=valor. Tiene que ser de root y que nadie más pueda
		# escribirla, o el actualizador no la lee. Obligatorias: REPO_URL,
		# ESCUCHA, JAIL_COMPILAR_IP, DNS_IP y RED_LAN. Los ejemplos son
		# direcciones de documentación: pon las tuyas.

		# Repositorio del que se traen las etiquetas vX.Y.Z (en la jail de
		# compilación, con la clave de despliegue de USUARIO_COMPILAR). En la
		# red local, mejor por IP: pf solo deja llegar a la jail de compilación
		# a REPO_IP. Ejemplos: ssh://git@192.0.2.20:2200/usuario/semillero.git
		# o, en Internet, https://git.example/usuario/semillero.git
		REPO_URL=
		# Línea de known_hosts del servidor git («[equipo]:puerto ssh-ed25519 …»).
		# Vacía: se acepta y se fija la primera clave que presente.
		REPO_HOST_CLAVE=

		# Dirección:puerto donde escucha semillero dentro de su jail (la de su
		# rc.d); ahí pregunta el actualizador por /salud. Ejemplo: 192.0.2.10:8090
		ESCUCHA=

		# Jail donde corre semillero y su raíz en el host
		# (vacía: /usr/local/jails/containers/<JAIL>).
		JAIL=semillero
		JAIL_RAIZ=

		# Jail de compilación (sin VNET), su raíz en el host (vacía: como la
		# de arriba) y su IP. Ejemplo: 192.0.2.30
		JAIL_COMPILAR=semillero-build
		JAIL_COMPILAR_RAIZ=
		JAIL_COMPILAR_IP=

		# Minutos que se espera a que llegue al buzón la versión que compila
		# el PC antes de compilarla aquí. 0: usa la del buzón si ya está, sin
		# esperar. Vacía: no se mira el buzón.
		PRECOMPILADAS_ESPERA=20
		# Trabajos de cargo a la vez al compilar aquí. Vacía: los que decida
		# cargo (uno por núcleo); menos trabajos, menos memoria.
		TRABAJOS_COMPILACION=

		# Solo para las reglas de pf que escribe install.sh: el servidor git de
		# la red local (REPO_PUERTO vacío: 22; REPO_IP vacía si el repositorio
		# está en Internet, que la jail ya alcanza) y el DNS a los que llega la
		# jail de compilación, la red local que no alcanza (una red o una lista
		# de pf entre llaves) y la interfaz externa del nat (vacía: se escribe
		# un hueco). Ejemplo:
		#   REPO_IP=192.0.2.20 REPO_PUERTO=2200 DNS_IP=192.0.2.1
		#   RED_LAN='{ 192.0.2.0/24 198.51.100.0/24 }' INTERFAZ_EXT=em0
		REPO_IP=
		REPO_PUERTO=
		DNS_IP=
		RED_LAN=
		INTERFAZ_EXT=

		# Usuarios sin privilegios (y su uid) de la jail de compilación y del
		# buzón de versiones.
		USUARIO_COMPILAR=smbuild
		UID_COMPILAR=3170
		USUARIO_BUZON=smentrega
		UID_BUZON=3171
	EOF
	instalar -m 0600 "$_t" "$P$CONF"
	rm -f "$_t"
fi
REPO_PUERTO=${REPO_PUERTO:-22}
JAIL_RAIZ=${JAIL_RAIZ:-/usr/local/jails/containers/$JAIL}
JAIL_COMPILAR_RAIZ=${JAIL_COMPILAR_RAIZ:-/usr/local/jails/containers/$JAIL_COMPILAR}
CLON=/home/$USUARIO_COMPILAR/semillero
_faltan=
for _k in $OBLIGATORIAS; do
	eval "_v=\${$_k}"
	[ -n "$_v" ] || _faltan="$_faltan $_k"
done
if [ -n "$_faltan" ]; then
	echo "ERROR: faltan en $CONF:$_faltan; rellénalas y repite install.sh (no se ha instalado nada más)" >&2
	exit 1
fi

say "actualizador en /usr/local/sbin/semillero-actualizar y $LIBEXEC"
instalar -d -m 0755 "$P/usr/local/sbin" "$P/usr/local/libexec" "$P$LIBEXEC"
instalar -m 0755 "$DIR/actualizar.sh" "$P/usr/local/sbin/semillero-actualizar"
instalar -m 0644 "$DEPLOY_DIR/lib/git.sh" "$P$LIBEXEC/git.sh"
instalar -m 0644 "$DEPLOY_DIR/lib/precompiladas.sh" "$P$LIBEXEC/precompiladas.sh"
instalar -m 0755 "$DIR/recibir-precompilada.sh" "$P$LIBEXEC/recibir-precompilada.sh"

say "cron y newsyslog"
instalar -d -m 0755 "$P/usr/local/etc/cron.d" "$P/usr/local/etc/newsyslog.conf.d"
_t=$(mktemp)
cat >"$_t" <<-'EOF'
	# Actualizador de semillero (repositorio semillero,
	# deploy/freebsd/actualizar.sh). Registro en /var/log/semillero-update.log.
	*/15	*	*	*	*	root	/usr/local/sbin/semillero-actualizar
EOF
instalar -m 0644 "$_t" "$P/usr/local/etc/cron.d/semillero"
cat >"$_t" <<-'EOF'
	/var/log/semillero-update.log	root:wheel	640	5	1000	*	JC
EOF
instalar -m 0644 "$_t" "$P/usr/local/etc/newsyslog.conf.d/semillero.conf"
rm -f "$_t"

say "carpetas"
instalar -d -m 0755 "$P/usr/local/semillero" "$P/usr/local/semillero/releases"
instalar -d -m 0755 "$P/var/db"
instalar -d -m 0700 "$P/var/db/semillero-actualizar"
if [ ! -d "$P$JAIL_RAIZ" ]; then
	echo "ERROR: no existe la jail en $JAIL_RAIZ (JAIL_RAIZ en $CONF)" >&2
	exit 1
fi
# Punto de montaje de las versiones: una vez montado (nullfs, solo lectura)
# no se toca, o install fallaría al fijar sus permisos.
if [ ! -d "$P$JAIL_RAIZ/usr/local/semillero/releases" ]; then
	instalar -d -m 0755 "$P$JAIL_RAIZ/usr/local/semillero" "$P$JAIL_RAIZ/usr/local/semillero/releases"
fi
# El buzón: su casa y .ssh de root (su usuario no puede quitarse las
# restricciones de su authorized_keys); entrada/ suya, si ya existe.
instalar -d -m 0755 "$P$BUZON_CASA" "$P$BUZON_CASA/.ssh"
BUZON_LISTO=
if [ -n "$P" ]; then
	install -d -m 0750 "$P$BUZON_CASA/entrada"
	BUZON_LISTO=1
elif pw usershow "$USUARIO_BUZON" >/dev/null 2>&1; then
	install -d -m 0750 -o "$USUARIO_BUZON" -g "$USUARIO_BUZON" "$BUZON_CASA/entrada"
	BUZON_LISTO=1
fi

# --- jail de compilación -------------------------------------------------------

BJ=$JAIL_COMPILAR_RAIZ

# Ejecuta $1 como USUARIO_COMPILAR en la jail de compilación, con su toolchain.
como_compilar()
{
	_g=$1
	shift
	jexec -l -U "$USUARIO_COMPILAR" "$JAIL_COMPILAR" /bin/sh -c "
		PATH=\$HOME/.cargo/bin:/usr/local/bin:/usr/bin:/bin
		export PATH
		$_g" sh "$@"
}

preparar_jail_compilar()
{
	say "$JAIL_COMPILAR: resolv.conf, paquetes y usuario $USUARIO_COMPILAR"
	echo "nameserver $DNS_IP" >"$BJ/etc/resolv.conf"
	if ! pkg -j "$JAIL_COMPILAR" info -e pkg 2>/dev/null; then
		pkg --rootdir "$BJ" install -y -r FreeBSD-ports pkg
	fi
	for p in git binaryen rustup-init; do
		if ! pkg -j "$JAIL_COMPILAR" info -e "$p" 2>/dev/null; then
			say "$JAIL_COMPILAR: instalando $p"
			pkg -j "$JAIL_COMPILAR" install -y "$p"
		fi
	done
	if ! pw -R "$BJ" usershow "$USUARIO_COMPILAR" >/dev/null 2>&1; then
		pw -R "$BJ" groupadd "$USUARIO_COMPILAR" -g "$UID_COMPILAR"
		pw -R "$BJ" useradd "$USUARIO_COMPILAR" -u "$UID_COMPILAR" -g "$USUARIO_COMPILAR" \
			-c "Compilación de semillero" -m -s /bin/sh
	fi

	# Sin toolchain por defecto: la versión de Rust la fija rust-toolchain.toml
	# del repositorio, y rustup la instala la primera vez que se compila.
	if ! como_compilar 'command -v rustup' >/dev/null 2>&1; then
		say "$JAIL_COMPILAR: rustup"
		como_compilar 'rustup-init -y --no-modify-path --profile minimal --default-toolchain none'
	fi

	# Clave de despliegue (de solo lectura en el servidor git) y la del
	# servidor, fijada. Todo lo de ~$USUARIO_COMPILAR se escribe como ese
	# usuario desde dentro de la jail: el código que compila podría haber dejado enlaces simbólicos que
	# una escritura de root desde el host seguiría.
	como_compilar 'mkdir -p "$HOME/.ssh" && chmod 700 "$HOME/.ssh"'
	if ! como_compilar 'test -f "$HOME/.ssh/id_ed25519"'; then
		say "$JAIL_COMPILAR: clave de despliegue"
		como_compilar 'ssh-keygen -q -t ed25519 -N "" -C "$1" -f "$HOME/.ssh/id_ed25519"' "$JAIL_COMPILAR"
	fi
	if [ -n "$REPO_HOST_CLAVE" ]; then
		_estricto=yes
		printf '%s\n' "$REPO_HOST_CLAVE" |
			como_compilar 'umask 077; rm -f "$HOME/.ssh/known_hosts" && cat >"$HOME/.ssh/known_hosts"'
	else
		_estricto=accept-new
	fi
	printf 'Host *\n    IdentityFile ~/.ssh/id_ed25519\n    IdentitiesOnly yes\n    StrictHostKeyChecking %s\n    UserKnownHostsFile ~/.ssh/known_hosts\n' "$_estricto" |
		como_compilar 'umask 077; rm -f "$HOME/.ssh/config" && cat >"$HOME/.ssh/config"'

	if como_compilar 'test -d "$1/.git"' "$CLON"; then
		como_compilar 'git -C "$1" remote set-url origin "$2"' "$CLON" "$REPO_URL"
	else
		say "$JAIL_COMPILAR: clonando el repositorio"
		if ! como_compilar 'GIT_SSH_COMMAND="ssh -o BatchMode=yes" git clone --quiet "$1" "$2"' \
			"$REPO_URL" "$CLON" 2>/dev/null; then
			echo "    sin acceso al repositorio todavía: da de alta la clave de despliegue (abajo) y repite install.sh"
			return 0
		fi
	fi
	# Dentro del clon, para que cargo use la Rust de rust-toolchain.toml.
	if [ "$(como_compilar 'cd "$1" && cargo leptos --version 2>/dev/null' "$CLON")" != "cargo-leptos $CARGO_LEPTOS_VERSION" ]; then
		say "$JAIL_COMPILAR: cargo-leptos $CARGO_LEPTOS_VERSION (tarda)"
		como_compilar 'cd "$1" && cargo install --locked cargo-leptos --version "$2"' "$CLON" "$CARGO_LEPTOS_VERSION"
	fi
}

JAIL_COMPILAR_LISTA=
if [ -z "$P" ] && jls -j "$JAIL_COMPILAR" jid >/dev/null 2>&1; then
	preparar_jail_compilar
	JAIL_COMPILAR_LISTA=1
fi

# --- lo que queda a mano -----------------------------------------------------

# Rellena con la configuración la plantilla que llega por la entrada
# estándar, sin sus comentarios, y la sangra para el texto de abajo.
rellenar()
{
	# Sin REPO_IP (repositorio en Internet) no hay regla para el servidor git.
	if [ -n "$REPO_IP" ]; then
		grep -v '^#'
	else
		grep -v -e '^#' -e '@REPO_IP@'
	fi | sed -e "s|@JAIL_COMPILAR@|$JAIL_COMPILAR|g" \
		-e "s|@JAIL_COMPILAR_IP@|$JAIL_COMPILAR_IP|g" \
		-e "s|@INTERFAZ_EXT@|${INTERFAZ_EXT:-<interfaz externa>}|g" \
		-e "s|@REPO_IP@|$REPO_IP|g" \
		-e "s|@REPO_PUERTO@|$REPO_PUERTO|g" \
		-e "s|@DNS_IP@|$DNS_IP|g" \
		-e "s|@RED_LAN@|$RED_LAN|g" \
		-e 's/^/     /'
}

cat <<EOF

Queda por hacer a mano (ver deploy/README.md):

1. Jail de compilación $JAIL_COMPILAR (IP $JAIL_COMPILAR_IP: comprueba que está
   libre y fuera del DHCP). Créala como cualquier jail (p. ej. un dataset
   ZFS en $BJ con la base de FreeBSD), añade a /etc/jail.conf:
$(rellenar <"$DIR/jail.conf.plantilla")
   arráncala (service jail start $JAIL_COMPILAR) y repite install.sh.
EOF
if [ -n "$JAIL_COMPILAR_LISTA" ]; then
	echo "   (ya está en marcha y preparada)"
fi
cat <<EOF

2. Reglas de pf de la jail de compilación (cópialas a /etc/pf.conf y
   comprueba con pfctl -nf /etc/pf.conf antes de cargarlas). La regla nat,
   antes de la primera nat del host:
$(sed -n '/^# @nat/,/^# @filtro/p' "$DIR/pf.conf.plantilla" | rellenar)
   y el filtro, encima de «block all»:
$(sed -n '/^# @filtro/,$p' "$DIR/pf.conf.plantilla" | rellenar)

3. Montar las versiones del host en la jail, solo lectura. Añade a
   /etc/fstab.$JAIL esta línea:
     /usr/local/semillero/releases	$JAIL_RAIZ/usr/local/semillero/releases	nullfs	ro	0	0
   y móntala ya:
     mount -t nullfs -o ro /usr/local/semillero/releases $JAIL_RAIZ/usr/local/semillero/releases

4. Usuario del buzón ($USUARIO_BUZON), para que el PC de compilación entregue
   las versiones (IP_DEL_PC y la clave, las del PC: ~/.ssh/semillero-buzon.pub):
     pw groupadd $USUARIO_BUZON -g $UID_BUZON
     pw useradd $USUARIO_BUZON -u $UID_BUZON -g $USUARIO_BUZON -d $BUZON_CASA -s /bin/sh -w no -c "Buzón de versiones de semillero"
     install -d -m 750 -o $USUARIO_BUZON -g $USUARIO_BUZON $BUZON_CASA/entrada
     printf '%s\n' 'restrict,from="IP_DEL_PC",command="$LIBEXEC/recibir-precompilada.sh" <clave pública del PC>' >$BUZON_CASA/.ssh/authorized_keys
     chown root:wheel $BUZON_CASA/.ssh/authorized_keys; chmod 644 $BUZON_CASA/.ssh/authorized_keys
   (si sshd tiene AllowUsers o AllowGroups, añade $USUARIO_BUZON).
EOF
if [ -n "$BUZON_LISTO" ]; then
	echo "   ($BUZON_CASA/entrada ya existe)"
fi
cat <<EOF

5. Clave de despliegue de $USUARIO_COMPILAR: dala de alta en el servidor git
   como clave de solo lectura del repositorio y repite install.sh si aún no
   había clon.
EOF
if [ -n "$JAIL_COMPILAR_LISTA" ]; then
	como_compilar 'cat "$HOME/.ssh/id_ed25519.pub"' | sed 's/^/     /'
fi
cat <<EOF

6. Servicio semillero en la jail $JAIL: su rc.d arranca
   /usr/local/semillero/current/semillero escuchando en $ESCUCHA. Publica
   la primera versión y pasa una vez el actualizador mirando el registro:
     /usr/local/sbin/semillero-actualizar --comprobar
     /usr/local/sbin/semillero-actualizar; tail /var/log/semillero-update.log
EOF
