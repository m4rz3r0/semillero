#!/bin/sh
#
# Instala en el host (FreeBSD, como root) el actualizador de semillero. Se
# pasa desde una copia del repositorio en el servidor; es idempotente: se
# puede repetir y solo hace lo que falte (también sobre una instalación
# anterior, la que tenía jail de compilación).
#
#   sh deploy/freebsd/install.sh [--prefijo DIR]
#
# Hace:
# - /usr/local/sbin/semillero-actualizar (deploy/freebsd/actualizar.sh) y, en
#   /usr/local/libexec/semillero-actualizar, git.sh y precompiladas.sh
#   (deploy/lib/); todo root:wheel;
# - /usr/local/etc/semillero-actualizar.conf (root, 0600) con los valores por
#   defecto, solo si no existe (si existe, se usan los suyos);
# - con REPO_URL por ssh: la clave de despliegue del host (CLAVE_DESPLIEGUE,
#   ed25519, root 0600) si no existe, y el known_hosts (HOSTS_CONOCIDOS,
#   0644) con la clave del servidor git de REPO_HOST_CLAVE si no existe; la
#   carpeta de las dos, 0700. Nunca rehace ni pisa ninguna de las dos;
# - con FORGEJO_URL y FORGEJO_REPO: la carpeta del token de lectura de
#   Forgejo (la de FORGEJO_TOKEN_FICHERO, 0700) si no existe. El token no lo
#   escribe: dice cómo ponerlo;
# - /usr/local/etc/cron.d/semillero (cada 15 minutos) y
#   /usr/local/etc/newsyslog.conf.d/semillero.conf (rota
#   /var/log/semillero-update.log);
# - /usr/local/semillero/releases en el host (root:wheel 0755),
#   /var/db/semillero-actualizar (0700) y el punto de montaje
#   <jail>/usr/local/semillero/releases dentro de la jail del servicio
#   (JAIL).
# No crea jails ni usuarios, no toca /etc/fstab.<jail> ni /etc/pf.conf y no
# compila nada: lo que queda por hacer a mano lo escribe al final, ya
# rellenado con la configuración.
#
# Si la configuración no existe, escribe la plantilla y para; si le falta un
# valor obligatorio (o no vale), dice cuál y para, antes de instalar nada
# más. Lo mismo si falta git en el host o, con Forgejo, curl o, con
# GITHUB_REPO, gh o jq (dice qué pkg install falta; no instala paquetes del
# host).
#
# --prefijo DIR (o SEMILLERO_ACT_RAIZ): lo instala todo bajo DIR, sin pedir
# root y sin cambiar dueños; es para las pruebas
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
# Valores por defecto (los mismos que actualizar.sh); si ya hay
# configuración, mandan los suyos. Los vacíos de OBLIGATORIAS no tienen
# valor por defecto: son de cada instalación.
OBLIGATORIAS="REPO_URL ESCUCHA"
REPO_URL=
REPO_HOST_CLAVE=
CLAVE_DESPLIEGUE=/usr/local/etc/semillero-actualizar/id_ed25519
HOSTS_CONOCIDOS=/usr/local/etc/semillero-actualizar/known_hosts
ESCUCHA=
JAIL=semillero
JAIL_RAIZ=
FORGEJO_URL=
FORGEJO_REPO=
FORGEJO_TOKEN_FICHERO=/usr/local/etc/semillero-actualizar/forgejo-token
GITHUB_REPO=
# Solo las tiene la configuración de una instalación anterior (con jail de
# compilación o con buzón): para decir que ya sobran.
JAIL_COMPILAR=
USUARIO_BUZON=

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

# Para antes de instalar nada más.
parar()
{
	echo "ERROR: $*; corrígelo y repite install.sh (no se ha instalado nada más)" >&2
	exit 1
}

for f in "$DIR/actualizar.sh" "$DEPLOY_DIR/lib/git.sh" "$DEPLOY_DIR/lib/precompiladas.sh"; do
	sh -n "$f" || {
		echo "ERROR: error de sintaxis en $f" >&2
		exit 1
	}
done
# shellcheck source=../lib/git.sh
. "$DEPLOY_DIR/lib/git.sh"

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
		# escribirla, o el actualizador no la lee. Obligatorias: REPO_URL y
		# ESCUCHA. Los ejemplos son direcciones de documentación: pon las tuyas.
		# El servidor no compila: instala la versión que publica el runner de
		# Forgejo Actions del PC en la release de Forgejo o la de la release de
		# GitHub (GITHUB_REPO).

		# Repositorio del que se leen las etiquetas vX.Y.Z (git ls-remote desde
		# el host, sin clonar). Por ssh, con la clave de despliegue de abajo.
		# Ejemplos: ssh://git@192.0.2.20:2200/usuario/semillero.git o
		# https://git.example/usuario/semillero.git (público, sin clave).
		REPO_URL=
		# Línea de known_hosts del servidor git («[equipo]:puerto ssh-ed25519 …»);
		# install.sh la escribe en HOSTS_CONOCIDOS si ese fichero no existe.
		REPO_HOST_CLAVE=
		# Clave de despliegue (de solo lectura en el servidor git; install.sh la
		# crea si no existe) y known_hosts con la clave del servidor git.
		CLAVE_DESPLIEGUE=/usr/local/etc/semillero-actualizar/id_ed25519
		HOSTS_CONOCIDOS=/usr/local/etc/semillero-actualizar/known_hosts

		# Dirección:puerto donde escucha semillero dentro de su jail (la de su
		# rc.d); ahí pregunta el actualizador por /salud. Ejemplo: 192.0.2.10:8090
		ESCUCHA=

		# Jail donde corre semillero y su raíz en el host
		# (vacía: /usr/local/jails/containers/<JAIL>).
		JAIL=semillero
		JAIL_RAIZ=

		# Forgejo del que traer la versión que publica el runner del PC: su URL
		# (sin barra al final, p. ej. https://forja.example) y el repositorio
		# (propietario/nombre). Se descarga la release vX.Y.Z con el token de
		# solo lectura de FORGEJO_TOKEN_FICHERO (de root, 0600). Necesita curl
		# en el host: pkg install curl. Vacías: no se mira Forgejo.
		FORGEJO_URL=
		FORGEJO_REPO=
		FORGEJO_TOKEN_FICHERO=/usr/local/etc/semillero-actualizar/forgejo-token

		# Repositorio de GitHub (propietario/repositorio, p. ej.
		# propietario/semillero) que publica las versiones con su atestación de
		# origen. Si se pone, una versión nueva que no está en Forgejo se trae
		# de su release, comprobando SHA256SUMS y la atestación con gh.
		# Necesita gh y jq en el host: pkg install gh jq. Vacía: no se mira
		# GitHub.
		GITHUB_REPO=
	EOF
	instalar -m 0600 "$_t" "$P$CONF"
	rm -f "$_t"
fi
JAIL_RAIZ=${JAIL_RAIZ:-/usr/local/jails/containers/$JAIL}
_faltan=
for _k in $OBLIGATORIAS; do
	eval "_v=\${$_k}"
	[ -n "$_v" ] || _faltan="$_faltan $_k"
done
if [ -n "$_faltan" ]; then
	echo "ERROR: faltan en $CONF:$_faltan; rellénalas y repite install.sh (no se ha instalado nada más)" >&2
	exit 1
fi
POR_SSH=
if repo_por_ssh "$REPO_URL"; then
	POR_SSH=1
	for _k in CLAVE_DESPLIEGUE HOSTS_CONOCIDOS; do
		eval "_v=\${$_k}"
		case $_v in
		/*[!A-Za-z0-9._/-]* | *..*) parar "$_k en $CONF tiene que ser una ruta absoluta sin espacios ni caracteres raros" ;;
		/*) ;;
		*) parar "$_k en $CONF tiene que ser una ruta absoluta sin espacios ni caracteres raros" ;;
		esac
	done
fi
# shellcheck source=../lib/precompiladas.sh
. "$DEPLOY_DIR/lib/precompiladas.sh"
if [ -n "$GITHUB_REPO" ] && ! repo_github_valido "$GITHUB_REPO"; then
	parar "GITHUB_REPO tiene que ser propietario/repositorio de GitHub (p. ej. propietario/semillero) o vacía en $CONF"
fi
FORGEJO_URL=${FORGEJO_URL%/}
if [ -n "$FORGEJO_URL" ] && ! url_forgejo_valida "$FORGEJO_URL"; then
	parar "FORGEJO_URL tiene que ser la URL de Forgejo (p. ej. https://forja.example) o vacía en $CONF"
fi
if [ -n "$FORGEJO_REPO" ] && ! repo_forgejo_valido "$FORGEJO_REPO"; then
	parar "FORGEJO_REPO tiene que ser propietario/nombre del repositorio en Forgejo o vacía en $CONF"
fi
case $FORGEJO_TOKEN_FICHERO in
/*[!A-Za-z0-9._/-]* | *..*) parar "FORGEJO_TOKEN_FICHERO en $CONF tiene que ser una ruta absoluta sin espacios ni caracteres raros" ;;
/*) ;;
*) parar "FORGEJO_TOKEN_FICHERO en $CONF tiene que ser una ruta absoluta sin espacios ni caracteres raros" ;;
esac
CON_FORGEJO=
if [ -n "$FORGEJO_URL" ] && [ -n "$FORGEJO_REPO" ]; then CON_FORGEJO=1; fi
# Paquetes del host: no se instalan desde aquí.
_paquetes=
_necesarios=git
if [ -n "$CON_FORGEJO" ]; then _necesarios="$_necesarios curl"; fi
if [ -n "$GITHUB_REPO" ]; then _necesarios="$_necesarios gh jq"; fi
for _o in $_necesarios; do
	[ -x "$P/usr/local/bin/$_o" ] || _paquetes="$_paquetes $_o"
done
if [ -n "$_paquetes" ]; then
	parar "el actualizador necesita en el host: pkg install$_paquetes"
fi

say "actualizador en /usr/local/sbin/semillero-actualizar y $LIBEXEC"
instalar -d -m 0755 "$P/usr/local/sbin" "$P/usr/local/libexec" "$P$LIBEXEC"
instalar -m 0755 "$DIR/actualizar.sh" "$P/usr/local/sbin/semillero-actualizar"
instalar -m 0644 "$DEPLOY_DIR/lib/git.sh" "$P$LIBEXEC/git.sh"
instalar -m 0644 "$DEPLOY_DIR/lib/precompiladas.sh" "$P$LIBEXEC/precompiladas.sh"

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
# La carpeta del token de Forgejo, solo si no existe (puede ser la de la
# clave de despliegue, que ya tiene sus permisos).
if [ -n "$CON_FORGEJO" ] && [ ! -d "$P$(dirname "$FORGEJO_TOKEN_FICHERO")" ]; then
	instalar -d -m 0700 "$P$(dirname "$FORGEJO_TOKEN_FICHERO")"
fi

# --- clave de despliegue y known_hosts, en el host ------------------------------
if [ -n "$POR_SSH" ]; then
	# La carpeta, solo si no existe (puede ser una que ya tiene sus permisos).
	for _d in "$(dirname "$CLAVE_DESPLIEGUE")" "$(dirname "$HOSTS_CONOCIDOS")"; do
		if [ ! -d "$P$_d" ]; then
			instalar -d -m 0700 "$P$_d"
		fi
	done
	if [ ! -e "$P$CLAVE_DESPLIEGUE" ]; then
		say "clave de despliegue $CLAVE_DESPLIEGUE"
		ssh-keygen -q -t ed25519 -N '' -C semillero-actualizar \
			-f "$P$CLAVE_DESPLIEGUE"
	fi
	# ssh no usa una clave que otros puedan leer; el actualizador tampoco.
	chmod 600 "$P$CLAVE_DESPLIEGUE"
	if [ ! -e "$P$HOSTS_CONOCIDOS" ] && [ -n "$REPO_HOST_CLAVE" ]; then
		say "known_hosts $HOSTS_CONOCIDOS (REPO_HOST_CLAVE)"
		_t=$(mktemp)
		printf '%s\n' "$REPO_HOST_CLAVE" >"$_t"
		instalar -m 0644 "$_t" "$P$HOSTS_CONOCIDOS"
		rm -f "$_t"
	fi
fi

# --- lo que queda a mano -----------------------------------------------------

cat <<EOF

Queda por hacer a mano (ver deploy/README.md):

1. Montar las versiones del host en la jail, solo lectura. Añade a
   /etc/fstab.$JAIL esta línea:
     /usr/local/semillero/releases	$JAIL_RAIZ/usr/local/semillero/releases	nullfs	ro	0	0
   y móntala ya:
     mount -t nullfs -o ro /usr/local/semillero/releases $JAIL_RAIZ/usr/local/semillero/releases
EOF
if [ -n "$CON_FORGEJO" ]; then
	cat <<EOF

2. Token de Forgejo para descargar las releases de $FORGEJO_REPO: créalo en
   Forgejo (Configuración > Aplicaciones) con permiso de solo lectura
   (read:repository) y escríbelo en $FORGEJO_TOKEN_FICHERO, de root y 0600:
     install -m 600 -o root -g wheel /dev/null $FORGEJO_TOKEN_FICHERO
     ee $FORGEJO_TOKEN_FICHERO
   Sin él (o legible por otros) el actualizador no usa Forgejo.
EOF
else
	cat <<EOF

2. Sin FORGEJO_URL y FORGEJO_REPO no se mira Forgejo: las versiones vienen
   solo de GitHub (GITHUB_REPO), si la hay.
EOF
fi
if [ -n "$POR_SSH" ]; then
	cat <<EOF

3. Clave de despliegue del host ($CLAVE_DESPLIEGUE): dala de alta en el
   servidor git como clave de solo lectura del repositorio:
EOF
	if [ -f "$P$CLAVE_DESPLIEGUE.pub" ]; then
		sed 's/^/     /' "$P$CLAVE_DESPLIEGUE.pub"
	else
		echo "     (ssh-keygen -y -f $CLAVE_DESPLIEGUE la escribe)"
	fi
	if [ -s "$P$HOSTS_CONOCIDOS" ]; then
		echo "   La clave del servidor git ya está fijada en $HOSTS_CONOCIDOS."
	else
		cat <<EOF
   Y fija la clave del servidor git en HOSTS_CONOCIDOS ($HOSTS_CONOCIDOS):
   pon su línea de known_hosts en REPO_HOST_CLAVE y repite install.sh, o
   escríbela tú (compara la huella con la del servidor git antes de fiarte):
     ssh-keyscan -p <puerto> -t ed25519 <servidor git> >$HOSTS_CONOCIDOS
     ssh-keygen -lf $HOSTS_CONOCIDOS
   Sin ella el actualizador no se conecta (StrictHostKeyChecking=yes).
EOF
	fi
else
	cat <<EOF

3. REPO_URL no es ssh: el actualizador lee las etiquetas sin clave de
   despliegue (el repositorio tiene que ser público).
EOF
fi
cat <<EOF

4. Servicio semillero en la jail $JAIL: su rc.d arranca
   /usr/local/semillero/current/semillero escuchando en $ESCUCHA. Publica
   la primera versión y pasa una vez el actualizador mirando el registro:
     /usr/local/sbin/semillero-actualizar --comprobar
     /usr/local/sbin/semillero-actualizar; tail /var/log/semillero-update.log
EOF
if [ -n "$JAIL_COMPILAR" ]; then
	cat <<EOF

La jail $JAIL_COMPILAR ya no hace falta: el servidor no compila. Sigue
«Migrar una instalación existente» en deploy/README.md (la clave de
despliegue y el known_hosts pasan al host; luego se puede destruir la jail
y quitar sus reglas de pf y su bloque de /etc/jail.conf).
EOF
fi
if [ -n "$USUARIO_BUZON" ]; then
	cat <<EOF

El usuario $USUARIO_BUZON ya no hace falta: las versiones llegan de la
release de Forgejo. Sigue «Migrar desde el buzón» en deploy/README.md
(quitar el usuario, su línea de authorized_keys y de AllowUsers, y
/var/db/semillero-buzon; poner el token de Forgejo).
EOF
fi
