#!/bin/sh
# shellcheck disable=SC2016 # los guiones de en_git y en_compilar se expanden en la jail
#
# Actualizador de semillero. Corre EN EL HOST (FreeBSD) como root; se instala
# como /usr/local/sbin/semillero-actualizar y lo lanza cron cada 15 minutos
# (/usr/local/etc/cron.d/semillero).
#
#   semillero-actualizar              despliega la última etiqueta vX.Y.Z
#   semillero-actualizar --comprobar  solo dice qué haría; no cambia nada
#   semillero-actualizar --ref REF    compila aquí y despliega REF (rama o
#                                     commit), a mano; la siguiente pasada
#                                     del cron vuelve a la última etiqueta
#
# Pasos:
# 1. Trae las etiquetas del repositorio (REPO_URL) en la jail de compilación
#    (JAIL_COMPILAR), como el usuario sin privilegios USUARIO_COMPILAR
#    (smbuild) en su clon /home/<usuario>/semillero, con
#    traer_ultima_etiqueta (deploy/lib/git.sh: nunca hace merge). Se queda
#    con la vX.Y.Z más alta (sort -V). Si es la desplegada o una que ya
#    falló dos veces seguidas (fichero «fallida»), termina sin decir nada.
#    La versión de crates/semillero/Cargo.toml en esa etiqueta tiene que ser
#    X.Y.Z (es la que dice /salud).
# 2. Si es nueva, mira el buzón (/var/db/semillero-buzon/entrada/X.Y.Z, lo
#    que deja el PC de compilación con deploy/cross/): espera hasta
#    PRECOMPILADAS_ESPERA minutos desde que vio la etiqueta («esperando la
#    versión precompilada»). Cuando llega (con SHA256SUMS, que el PC sube el
#    último), la copia a una carpeta propia y la comprueba antes de
#    instalarla (deploy/lib/precompiladas.sh): sha256, solo ficheros y
#    carpetas normales, una sola carpeta con semillero, site/ y COMMIT,
#    COMMIT igual al commit de la etiqueta y semillero un ejecutable de
#    FreeBSD x86-64. La deja en /usr/local/semillero/releases/X.Y.Z (root;
#    carpetas 755, ficheros 644, el binario 755). Esa carpeta del host se
#    monta (nullfs, solo lectura) en la jail del servicio (JAIL) en
#    /usr/local/semillero/releases.
# 3. Si no llega a tiempo o no cuadra, compila aquí, en la jail de
#    compilación: cargo leptos build --release --precompress, con prioridad
#    baja, una hora como mucho y TRABAJOS_COMPILACION trabajos de cargo a la
#    vez (salida en /var/log/semillero-build.log). Solo una compilación a la
#    vez en el host: se toma el cerrojo neutro /var/run/compilar.lock (no es
#    de ningún proyecto; otros del host pueden usar el mismo); si está cogido,
#    lo dice y lo deja para la siguiente pasada, sin contarlo como fallo. El
#    binario y la web salen de la jail como un tar que se genera DENTRO de
#    ella: este guion, que corre como root, nunca abre rutas de la jail de
#    compilación, que controla el código que se compila.
# 4. Para el servicio, copia la base de datos dentro de la jail JAIL como
#    el usuario semillero (/var/db/semillero/antes-de-actualizar), cambia de
#    golpe el enlace «current» de la jail (ln -sfh + mv -fh) y arranca. Espera
#    60 s como mucho a que /salud en ESCUCHA (preguntado desde dentro de la
#    jail: el cortafuegos puede no dejar llegar desde el host) responda
#    "ok":true y "version":"X.Y.Z", y a que quien escucha en el puerto sea
#    el binario de releases/X.Y.Z.
# 5. Si no: para, deja la base de datos como estaba, vuelve a la versión
#    anterior (o quita «current» si no había) y arranca; borra la versión
#    fallida. Tras 2 fallos seguidos de la misma etiqueta (también una
#    compilación que falla o una versión de Cargo.toml que no cuadra), la
#    apunta en /var/db/semillero-actualizar/fallida y deja de intentarlo
#    hasta que haya otra.
# Se conservan las 3 versiones más recientes; nunca se borra la desplegada.
#
# Configuración: /usr/local/etc/semillero-actualizar.conf (root, 600; la
# escribe deploy/freebsd/install.sh). Solo se lee si es de root y nadie más
# puede escribirla. REPO_URL y ESCUCHA no tienen valor por defecto: si faltan,
# no se hace nada y se dice cuál falta. Las funciones de deploy/lib/ van en
# /usr/local/libexec/semillero-actualizar.
#
# Registro: /var/log/semillero-update.log (lo rota newsyslog).
#
# Cada arranque de semillero hace un login fallido a propósito en
# qBittorrent (el canario de arranque) y qBittorrent bloquea tras 5 seguidos:
# por eso cada intento arranca la versión nueva una sola vez y, si falla, la
# anterior otra (daemon -r solo la relanza si se cae, y el canario va después
# de abrir el puerto).
#
# Solo para las pruebas (deploy/pruebas/actualizar.sh): SEMILLERO_ACT_RAIZ va
# delante de todas las rutas absolutas del host, SEMILLERO_ACT_PATH sustituye
# al PATH (para poner órdenes de mentira) y SEMILLERO_ACT_ESPERA a los 60 s
# de /salud.

set -u
umask 022

PATH=${SEMILLERO_ACT_PATH:-/sbin:/bin:/usr/sbin:/usr/bin:/usr/local/sbin:/usr/local/bin}
export PATH
RAIZ=${SEMILLERO_ACT_RAIZ:-}

CONF=/usr/local/etc/semillero-actualizar.conf
LIBEXEC=/usr/local/libexec/semillero-actualizar
# Valores por defecto; los de la instalación vienen de $CONF. Los vacíos sin
# comentario son obligatorios.
REPO_URL=
JAIL=semillero
# Vacía: /usr/local/jails/containers/$JAIL.
JAIL_RAIZ=
JAIL_COMPILAR=semillero-build
# Minutos de espera a la versión precompilada; vacía, no se mira el buzón.
PRECOMPILADAS_ESPERA=20
# Trabajos de cargo a la vez; vacía, los que cargo decida (uno por núcleo).
TRABAJOS_COMPILACION=
# Dirección:puerto de escucha de semillero en la jail (la de su rc.d), p. ej.
# 192.0.2.10:8090 o [2001:db8::10]:8090.
ESCUCHA=

SERVICIO=semillero
USUARIO=semillero
USUARIO_COMPILAR=smbuild
# git de la jail por ruta absoluta en todo lo que el host usa:
# ~$USUARIO_COMPILAR/.cargo/bin (lo primero del PATH de la compilación) lo puede
# escribir cualquier build.rs.
GIT=/usr/local/bin/git
RELEASES=$RAIZ/usr/local/semillero/releases
# Las mismas versiones vistas desde dentro de la jail (montaje nullfs).
RELEASES_JAIL=/usr/local/semillero/releases
DB_DIR=/var/db/semillero
DB_COPIA=$DB_DIR/antes-de-actualizar
ESTADO=$RAIZ/var/db/semillero-actualizar
BUZON=$RAIZ/var/db/semillero-buzon/entrada
LOG=$RAIZ/var/log/semillero-update.log
LOG_COMPILACION=$RAIZ/var/log/semillero-build.log
CERROJO=$RAIZ/var/run/semillero-actualizar.lock
# Cerrojo neutro del host: una sola compilación a la vez en el servidor.
CERROJO_COMPILAR=$RAIZ/var/run/compilar.lock
ESPERA=${SEMILLERO_ACT_ESPERA:-60}
TIEMPO_COMPILACION=3600
CONSERVAR=3
# Fallos seguidos de una etiqueta antes de darla por fallida.
MAX_INTENTOS=2

log()
{
	_linea="$(date '+%Y-%m-%d %H:%M:%S') $*"
	# Con --comprobar no se escribe nada: solo por pantalla.
	if [ -n "${_comprobar:-}" ]; then
		echo "$_linea" >&2
		return 0
	fi
	echo "$_linea" >>"$LOG"
	if [ -t 1 ]; then
		echo "$_linea"
	fi
}

error()
{
	log "ERROR: $*"
}

uso()
{
	echo "uso: $0 [--comprobar | --ref REF]" >&2
	exit 2
}

# Texto que viene de fuera (la jail de compilación), recortado y sin nada
# raro, para el registro.
limpio()
{
	printf '%s' "$1" | head -c 60 | tr -c 'A-Za-z0-9._-' '?'
}

# Fichero $1 normal (no enlace), de root (o, en las pruebas, de quien corre
# esto) y que nadie más puede escribir.
fichero_seguro()
{
	[ -f "$1" ] && [ ! -L "$1" ] || return 1
	_l=$(ls -ldn -- "$1") || return 1
	_modo=${_l%% *}
	set -f
	# shellcheck disable=SC2086 # se parte a propósito en campos
	set -- "$1" $_l
	set +f
	# $2 modo, $3 enlaces, $4 dueño (numérico).
	case $4 in
	0 | "$(id -u)") ;;
	*) return 1 ;;
	esac
	case $_modo in
	?????w* | ????????w*) return 1 ;;
	esac
}

# Etiqueta vX.Y.Z (version_valida, deploy/lib/precompiladas.sh).
etiqueta_valida()
{
	case $1 in
	v*) version_valida "${1#v}" ;;
	*) return 1 ;;
	esac
}

# Commit completo: 40 cifras hexadecimales en minúscula.
commit_valido()
{
	case $1 in
	'' | *[!0-9a-f]*) return 1 ;;
	esac
	[ "${#1}" -eq 40 ]
}

# Versión enlazada en «current» de la jail (vacía si no hay).
desplegada()
{
	_t=$(readlink "$CURRENT" 2>/dev/null) || return 0
	basename "$_t"
}

# Ejecuta el guion $2 en la jail de compilación como $USUARIO_COMPILAR,
# dentro del clon y con PATH=$1; el resto de argumentos le llegan como $1,
# $2… Nunca hereda el descriptor 9 (el cerrojo de compilación del host).
en_jail_compilar()
{
	_path=$1
	_guion=$2
	shift 2
	jexec -l -U "$USUARIO_COMPILAR" "$JAIL_COMPILAR" /bin/sh -c "
		PATH=$_path
		GIT_SSH_COMMAND='ssh -o BatchMode=yes -o ConnectTimeout=15'
		export PATH GIT_SSH_COMMAND
		cd $CLON || exit 1
		$_guion" sh "$@" 9<&-
}

# Para compilar: con ~/.cargo/bin delante.
en_compilar()
{
	en_jail_compilar "\$HOME/.cargo/bin:/usr/local/bin:/usr/bin:/bin" "$@"
}

# Para lo que el host usa (etiquetas, commits, Cargo.toml): PATH del sistema
# y `git` es siempre $GIT. Aun así, la salida no es de fiar (el clon lo puede
# tocar el código que se compila): se valida todo antes de usarlo.
en_git()
{
	_g=$1
	shift
	en_jail_compilar /usr/local/bin:/usr/bin:/bin "git() { $GIT \"\$@\"; }
		$_g" "$@"
}

servicio()
{
	jexec "$JAIL" /usr/sbin/service "$SERVICIO" "$1" >>"$LOG" 2>&1
}

# Guion $1 en la jail como el usuario del servicio; sus $1 y $2 son la
# carpeta de la base de datos y la de la copia.
en_jail()
{
	jexec -U "$USUARIO" "$JAIL" /bin/sh -c "$1" sh "$DB_DIR" "$DB_COPIA"
}

# Con el servicio parado: copia semillero.db (y -wal/-shm si los hay). Si aún
# no hay base de datos (primera vez), no hay nada que copiar.
copiar_db()
{
	en_jail '[ -d "$1" ] || exit 0
		cd "$1" || exit 1
		rm -rf "$2" && mkdir -m 700 "$2" || exit 1
		for f in semillero.db semillero.db-wal semillero.db-shm; do
			if [ -f "$f" ]; then cp -p "$f" "$2/" || exit 1; fi
		done'
}

# Con el servicio parado: deja la base de datos como antes del despliegue (la
# versión nueva puede haber aplicado una migración que la anterior no conoce).
restaurar_db()
{
	en_jail '[ -d "$2" ] || exit 0
		cd "$1" || exit 1
		rm -f semillero.db semillero.db-wal semillero.db-shm || exit 1
		for f in "$2"/*; do
			if [ -f "$f" ]; then cp -p "$f" . || exit 1; fi
		done'
}

# ¿Quien escucha en el puerto de semillero en la jail es el binario de
# releases/$1? Si el proceso viejo no hubiera muerto, seguiría contestando a
# /salud con la versión anterior.
sirve()
{
	_pid=$(sockstat -j "$JAIL" -l -p "${ESCUCHA##*:}" -q | awk '{ print $3; exit }')
	[ -n "$_pid" ] || return 1
	case "$(ps -ww -o command= -p "$_pid")" in
	"$RELEASES_JAIL/$1/semillero" | "$RELEASES_JAIL/$1/semillero "*) return 0 ;;
	esac
	return 1
}

# /salud responde "ok":true con la versión de releases/$1 (X.Y.Z, o lo que
# va antes del primer «-» en las de --ref) y la sirve releases/$1, como mucho
# en $ESPERA segundos. fetch falla con cualquier respuesta que no sea 2xx.
sana()
{
	_ver=${1%%-*}
	_fin=$(($(date +%s) + ESPERA))
	while [ "$(date +%s)" -lt "$_fin" ]; do
		_r=$(jexec "$JAIL" /usr/bin/fetch -T 2 -qo - "http://$ESCUCHA/salud" 2>/dev/null) || _r=
		case $_r in
		*'"ok":true'*)
			case $_r in
			*"\"version\":\"$_ver\""*)
				if sirve "$1"; then return 0; fi
				;;
			esac
			;;
		esac
		sleep 2
	done
	return 1
}

# Apunta «current» a releases/$1 de golpe (rename). El enlace es relativo:
# dentro de la jail lleva al montaje nullfs. En FreeBSD, ln -sfh y mv -fh;
# en las pruebas (Linux), sus equivalentes de GNU.
fijar_current()
{
	if [ "$(uname -s)" = FreeBSD ]; then
		ln -sfh "releases/$1" "$CURRENT.new" && mv -fh "$CURRENT.new" "$CURRENT"
	else
		ln -sfn "releases/$1" "$CURRENT.new" && mv -fT "$CURRENT.new" "$CURRENT"
	fi
}

# Deja la versión preparada en $1 de root, con 755/644 (el binario, 755) y
# la fecha de ahora (la poda ordena por fecha y tar deja la del commit).
asentar()
{
	if [ "$(id -u)" -eq 0 ] && ! chown -R root:wheel "$1"; then
		return 1
	fi
	find "$1" -type d -exec chmod 755 {} + &&
		find "$1" -type f -exec chmod 644 {} + &&
		chmod 755 "$1/semillero" && touch "$1"
}

# Qué hacer con la etiqueta $1 (versión $2) según el buzón: instalar,
# esperar o compilar (decidir_precompilada).
decidir()
{
	_llegada=no
	if precompilada_llegada "$BUZON" "$2"; then
		_llegada=si
	fi
	decidir_precompilada "$ESTADO/precompilada" "$1" "$(date +%s)" \
		"$PRECOMPILADAS_ESPERA" "$_llegada"
}

# Instala en releases/$2 la versión precompilada del buzón de la etiqueta $1
# con commit $3. Si no cuadra, lo registra y sale con 1. En los dos casos
# vacía la carpeta de esa versión del buzón: un segundo intento ya no la
# tiene y compila aquí.
instalar_precompilada()
{
	_tmp=$RELEASES/.$2.tmp
	rm -rf "$_tmp"
	_r=1
	if _err=$(preparar_precompilada "$BUZON" "$2" "$3" "$_tmp" 2>&1) &&
		asentar "$_tmp" && mv "$_tmp" "$RELEASES/$2"; then
		log "instalada la versión precompilada de $1 (commit $(printf '%.12s' "$3"))"
		_r=0
	else
		error "versión precompilada de $1 rechazada: $(printf '%s' "${_err:-no se puede instalar}" | head -n 1); se compila aquí"
	fi
	rm -rf "$_tmp" "${BUZON:?}/$2"
	return "$_r"
}

# Compila en la jail de compilación la referencia $1 (commit $3) y deja el
# resultado en releases/$2. Sale con 0 si queda instalada, 1 si falla la
# compilación (cuenta como fallo de la etiqueta), 2 si hay otra compilación
# en marcha en el host y 3 si falla algo del servidor.
compilar_aqui()
{
	# Cerrojo neutro del host, en este mismo proceso (lockf con descriptor:
	# dura lo que dure el descriptor 9 abierto). Solo de lectura; la jail de
	# compilación no lo hereda (en_jail_compilar lo cierra).
	: >>"$CERROJO_COMPILAR" 2>/dev/null
	if [ ! -f "$CERROJO_COMPILAR" ] || [ ! -r "$CERROJO_COMPILAR" ]; then
		error "no se puede abrir el cerrojo de compilación $CERROJO_COMPILAR"
		return 3
	fi
	exec 9<"$CERROJO_COMPILAR"
	# 75 (EX_TEMPFAIL): lo tiene otro. Cualquier otro error no es «ocupado»:
	# callarlo dejaría de compilar para siempre sin decir por qué.
	lockf -s -t 0 9
	case $? in
	0) ;;
	75)
		exec 9<&-
		log "hay otra compilación en marcha en el servidor; se reintenta en la próxima pasada"
		return 2
		;;
	*)
		exec 9<&-
		error "lockf no puede tomar el cerrojo de compilación $CERROJO_COMPILAR"
		return 3
		;;
	esac
	compilar_y_sacar "$@"
	_r=$?
	exec 9<&-
	return "$_r"
}

compilar_y_sacar()
{
	_ref=$1
	_nombre=$2
	_commit=$3
	if ! en_git 'git -c advice.detachedHead=false checkout --quiet --force --detach "$1" &&
		git clean -fdxq -e target' "$_ref" >"$LOG_COMPILACION" 2>&1; then
		error "no se puede sacar $(printf '%.12s' "$_commit") del repositorio (ver $LOG_COMPILACION)"
		return 1
	fi
	log "compilando $_nombre aquí ($(printf '%.12s' "$_commit"))"
	# wasm-bindgen-cli tiene que ser exactamente la versión de wasm-bindgen
	# del Cargo.lock; si el repositorio la cambia, se instala la nueva.
	if ! en_compilar '
		quiere=$(awk "/^name = \"wasm-bindgen\"\$/ { getline; gsub(/version = |\"/, \"\"); print; exit }" Cargo.lock)
		tiene=$(wasm-bindgen --version 2>/dev/null | cut -d" " -f2)
		if [ -n "$quiere" ] && [ "$quiere" != "$tiene" ]; then
			echo "instalando wasm-bindgen-cli $quiere (había: ${tiene:-ninguno})"
			cargo install --locked wasm-bindgen-cli --version "$quiere" || exit 1
		fi
		# Trabajos a la vez (menos trabajos, menos memoria) y prioridad
		# baja, para que el resto del servidor no lo note.
		case $1 in
		"") ;;
		*[!0-9]* | 0*) echo "TRABAJOS_COMPILACION no es válida; se usa el valor de cargo" ;;
		*)
			CARGO_BUILD_JOBS=$1
			export CARGO_BUILD_JOBS
			;;
		esac
		renice -n 15 $$ >/dev/null 2>&1
		# Solo se compila; las pruebas se pasan antes de crear la etiqueta.
		# Un compilador colgado no deja el actualizador esperando para
		# siempre: falla como cualquier error de compilación.
		timeout -k 60 "$2" cargo leptos build --release --precompress' \
		"$TRABAJOS_COMPILACION" "$TIEMPO_COMPILACION" >>"$LOG_COMPILACION" 2>&1; then
		error "falló la compilación de $_nombre (ver $LOG_COMPILACION)"
		return 1
	fi
	_tmp=$RELEASES/.$_nombre.tmp
	rm -rf "$_tmp"
	mkdir -m 700 "$_tmp" || return 3
	# El tar se genera dentro de la jail; el del host no extrae rutas
	# absolutas ni con «..». pipefail (en un subshell): si el tar de la jail
	# falla a medias, la versión se descarta.
	# shellcheck disable=SC3040 # el sh de FreeBSD (14+) tiene pipefail
	if ! (
		set -o pipefail
		en_compilar 'tar -cf - -C target release/semillero site' |
			tar -xf - -C "$_tmp" --no-same-owner --no-same-permissions
	) 2>>"$LOG_COMPILACION"; then
		error "no se pueden sacar el binario y la web de $_nombre de la jail de compilación"
		rm -rf "$_tmp"
		return 1
	fi
	if [ -n "$(find "$_tmp" ! -type f ! -type d)" ] ||
		[ -n "$(find "$_tmp" -type f -links +1)" ] ||
		[ ! -f "$_tmp/release/semillero" ] || [ ! -d "$_tmp/site" ]; then
		error "lo compilado de $_nombre no es release/semillero y site/ con ficheros y carpetas normales"
		rm -rf "$_tmp"
		return 1
	fi
	if ! _err=$(web_completa "$_tmp/site" 2>&1); then
		error "lo compilado de $_nombre no vale: $_err"
		rm -rf "$_tmp"
		return 1
	fi
	if ! mv "$_tmp/release/semillero" "$_tmp/semillero" || ! rmdir "$_tmp/release" ||
		! echo "$_commit" >"$_tmp/COMMIT" || ! asentar "$_tmp" ||
		! mv "$_tmp" "$RELEASES/$_nombre"; then
		error "no se puede preparar releases/$_nombre"
		rm -rf "$_tmp"
		return 3
	fi
	log "compilada $_nombre en el servidor"
	return 0
}

# Borra las versiones más antiguas; nunca la desplegada.
podar()
{
	_queda=$(desplegada)
	# shellcheck disable=SC2012 # nombres validados: sin espacios ni saltos
	ls -1t "$RELEASES" | tail -n +$((CONSERVAR + 1)) | while read -r _vieja; do
		if [ "$_vieja" != "$_queda" ]; then
			rm -rf "${RELEASES:?}/$_vieja"
			log "borrada la versión antigua $_vieja"
		fi
	done
}

# Despliega releases/$1; si no está sana, vuelve a la anterior con la base de
# datos de antes y borra la versión fallida. Sin versión anterior (primera
# vez), el servicio queda parado y sin «current»: el rc.d se niega a
# arrancar sin versión, también al reiniciar el host.
desplegar()
{
	_nueva=$1
	_antes=$(desplegada)
	# Puede fallar si no estaba en marcha (la primera vez): da igual.
	servicio stop
	if ! copiar_db; then
		error "no se puede copiar la base de datos; no se despliega $_nueva"
		servicio start
		return 1
	fi
	if ! fijar_current "$_nueva"; then
		error "no se puede cambiar current a $_nueva; sigue ${_antes:-sin versión}"
		servicio start
		return 1
	fi
	servicio start
	if sana "$_nueva"; then
		log "desplegada $_nueva (antes: ${_antes:-ninguna})"
		return 0
	fi
	error "$_nueva no responde bien en /salud (o no es quien escucha) tras $ESPERA s"
	servicio stop
	restaurar_db || error "no se puede restaurar la base de datos de antes de $_nueva"
	if [ -n "$_antes" ] && [ -d "$RELEASES/$_antes" ]; then
		fijar_current "$_antes"
		servicio start
		if sana "$_antes"; then
			log "vuelta atrás a $_antes"
		else
			error "vuelta atrás a $_antes, pero tampoco responde"
		fi
	else
		rm -f "$CURRENT"
		error "no hay versión anterior a la que volver; el servicio queda parado y sin versión desplegada"
	fi
	rm -rf "${RELEASES:?}/$_nueva"
	log "borrada la versión fallida $_nueva"
	return 1
}

# Un fallo de la etiqueta $1. Uno suelto puede ser pasajero: solo tras
# $MAX_INTENTOS seguidos queda en «fallida» y el cron deja de intentarlo.
fallo_etiqueta()
{
	_n=1
	if [ -f "$ESTADO/intentos" ] && [ "$(sed -n 1p "$ESTADO/intentos")" = "$1" ]; then
		_n=$(($(sed -n 2p "$ESTADO/intentos" | tr -cd '0-9') + 1))
	fi
	if [ "$_n" -ge "$MAX_INTENTOS" ]; then
		echo "$1" >"$ESTADO/fallida"
		rm -f "$ESTADO/intentos"
		error "$1 ha fallado $_n veces seguidas; no se reintenta hasta la próxima etiqueta"
	else
		printf '%s\n%s\n' "$1" "$_n" >"$ESTADO/intentos"
		log "fallo $_n de $MAX_INTENTOS de $1; se reintenta en la próxima ejecución"
	fi
}

# Sin el montaje nullfs, la jail no vería la versión $1 y la vuelta atrás
# tampoco funcionaría: es un problema del servidor, no de la etiqueta.
jail_la_ve()
{
	if [ ! -f "$RAIZ$JAIL_RAIZ$RELEASES_JAIL/$1/semillero" ]; then
		error "la jail $JAIL no ve $RELEASES_JAIL/$1 (¿falta el montaje nullfs de /etc/fstab.$JAIL?); no se despliega"
		return 1
	fi
}

# --ref REF: compila aquí y despliega REF. Nunca mira el buzón ni toca
# «fallida» ni «intentos» (son de las etiquetas).
desplegar_ref()
{
	_r=$1
	if ! en_git "$_git_sh
		git remote set-url origin \"\$1\" && traer_referencias ." "$REPO_URL" 2>"$TRABAJO/err"; then
		log "sin acceso al repositorio ($(head -n 1 "$TRABAJO/err" | cut -c1-200)); se compila $1 con lo que ya hay en el clon"
	fi
	# Las ramas se toman del remoto si existe; si no, la local.
	if en_git 'git rev-parse --verify --quiet "origin/$1^{commit}"' "$_r" >/dev/null 2>&1; then
		_r=origin/$_r
	fi
	_commit=$(en_git 'git rev-parse --verify --quiet "$1^{commit}"' "$_r" 2>/dev/null)
	if ! commit_valido "$_commit"; then
		error "$1 no existe en el repositorio"
		exit 1
	fi
	_cargo_v=$(en_git 'git show "$1:crates/semillero/Cargo.toml"' "$_commit" 2>/dev/null | version_cargo)
	if ! version_valida "$_cargo_v"; then
		error "crates/semillero/Cargo.toml de $1 no tiene una versión X.Y.Z ($(limpio "$_cargo_v"))"
		exit 1
	fi
	_nombre=$_cargo_v-$(printf '%.12s' "$_commit")
	if [ "$_nombre" = "$(desplegada)" ]; then
		log "$_nombre ya está desplegada"
		exit 0
	fi
	log "--ref $1: $_nombre (desplegada: $(desplegada))"
	find "$RELEASES" -maxdepth 1 -name '.*.tmp' -exec rm -rf {} +
	if [ ! -d "$RELEASES/$_nombre" ]; then
		compilar_aqui "$_commit" "$_nombre" "$_commit"
		case $? in
		0) ;;
		2) exit 0 ;;
		*) exit 1 ;;
		esac
	fi
	jail_la_ve "$_nombre" || exit 1
	desplegar "$_nombre" || exit 1
	podar
}

main()
{
	_comprobar=
	_ref=
	case "$#:${1:-}" in
	1:--comprobar) _comprobar=1 ;;
	2:--ref)
		_ref=$2
		case $_ref in
		'' | -* | *[!A-Za-z0-9._/-]* | *..*) uso ;;
		esac
		[ "${#_ref}" -le 100 ] || uso
		;;
	0: | 1:) ;;
	*) uso ;;
	esac
	# --comprobar no cambia nada: ni carpetas, ni restos, ni registro.
	if [ -z "$_comprobar" ]; then
		mkdir -p "$ESTADO" "$RELEASES" "$(dirname "$LOG")" || exit 1
		chmod 700 "$ESTADO"
	fi

	if [ -e "$RAIZ$CONF" ]; then
		if ! fichero_seguro "$RAIZ$CONF"; then
			error "$CONF tiene que ser un fichero de root que nadie más pueda escribir; no se hace nada"
			exit 1
		fi
		# shellcheck source=/dev/null
		. "$RAIZ$CONF"
	fi
	# Las obligatorias: sin ellas no se hace nada.
	for _k in REPO_URL ESCUCHA; do
		eval "_v=\${$_k}"
		if [ -z "$_v" ]; then
			error "falta $_k en $CONF; no se hace nada"
			exit 1
		fi
	done
	case $REPO_URL in
	-* | *[![:print:]]*)
		error "REPO_URL no válida"
		exit 1
		;;
	esac
	# dirección:puerto (IPv6 entre corchetes).
	_dir=${ESCUCHA%:*}
	_puerto=${ESCUCHA##*:}
	case $ESCUCHA in
	*:*) ;;
	*) _dir= ;;
	esac
	case $_dir/$_puerto in
	/* | */ | *[!A-Za-z0-9.:\[\]/-]* | */*[!0-9]*)
		error "ESCUCHA tiene que ser dirección:puerto (p. ej. 192.0.2.10:8090)"
		exit 1
		;;
	esac
	case $USUARIO_COMPILAR in
	'' | -* | *[!A-Za-z0-9_-]*)
		error "USUARIO_COMPILAR no válido"
		exit 1
		;;
	esac
	# El clon, dentro de la jail de compilación.
	CLON=/home/$USUARIO_COMPILAR/semillero
	for _j in "$JAIL" "$JAIL_COMPILAR"; do
		case $_j in
		'' | *[!A-Za-z0-9_-]*)
			error "JAIL o JAIL_COMPILAR no válida"
			exit 1
			;;
		esac
	done
	JAIL_RAIZ=${JAIL_RAIZ:-/usr/local/jails/containers/$JAIL}
	case $JAIL_RAIZ in
	/*) ;;
	*)
		error "JAIL_RAIZ tiene que ser una ruta absoluta"
		exit 1
		;;
	esac
	case $PRECOMPILADAS_ESPERA in
	'' | 0) ;;
	*[!0-9]* | 0*)
		error "PRECOMPILADAS_ESPERA tiene que ser un número de minutos (o vacía)"
		exit 1
		;;
	esac
	case $TRABAJOS_COMPILACION in
	'') ;;
	*[!0-9]* | 0*)
		error "TRABAJOS_COMPILACION tiene que ser un número mayor que 0 (o vacía)"
		exit 1
		;;
	esac
	CURRENT=$RAIZ$JAIL_RAIZ/usr/local/semillero/current
	for _f in precompiladas.sh git.sh; do
		if ! fichero_seguro "$RAIZ$LIBEXEC/$_f"; then
			error "falta $LIBEXEC/$_f o no es de root (se instala con deploy/freebsd/install.sh)"
			exit 1
		fi
	done
	# shellcheck source=../lib/precompiladas.sh
	. "$RAIZ$LIBEXEC/precompiladas.sh"
	for _orden in tar jexec sockstat file timeout lockf; do
		command -v "$_orden" >/dev/null 2>&1 || {
			error "falta $_orden"
			exit 1
		}
	done

	# Global con nombre propio: el trap la borra siempre.
	TRABAJO=$(mktemp -d) || exit 1
	trap 'rm -rf "$TRABAJO"' EXIT
	trap 'exit 143' INT TERM

	# Las funciones de git.sh se mandan a la jail como texto.
	_git_sh=$(cat "$RAIZ$LIBEXEC/git.sh")
	if [ -n "$_ref" ]; then
		desplegar_ref "$_ref"
		exit 0
	fi
	if [ -n "$_comprobar" ]; then
		# Sin traer nada: ls-remote no cambia el clon.
		if ! _etiqueta=$(en_git "$_git_sh
			ultima_etiqueta_remota \"\$1\"" "$REPO_URL" 2>"$TRABAJO/err"); then
			echo "sin acceso al repositorio: $(head -n 1 "$TRABAJO/err" | cut -c1-200)" >&2
			exit 1
		fi
	else
		if ! _salida=$(en_git "$_git_sh
			git remote set-url origin \"\$1\" && traer_ultima_etiqueta ." "$REPO_URL" 2>"$TRABAJO/err"); then
			log "sin acceso al repositorio desde $JAIL_COMPILAR: $(head -n 1 "$TRABAJO/err" | cut -c1-200)"
			exit 0
		fi
		_etiqueta=$(printf '%s\n' "$_salida" | tail -n 1)
	fi
	if [ -z "$_etiqueta" ]; then
		if [ -n "$_comprobar" ]; then echo "no hay ninguna etiqueta vX.Y.Z"; fi
		exit 0
	fi
	# Lo que sale de la jail de compilación no es de fiar: el nombre acaba en
	# rutas que root borra.
	if ! etiqueta_valida "$_etiqueta"; then
		error "la última etiqueta no es vX.Y.Z válida ($(limpio "$_etiqueta"))"
		exit 1
	fi
	_v=${_etiqueta#v}
	_hay=$(desplegada)
	if [ "$_v" = "$_hay" ]; then
		if [ -n "$_comprobar" ]; then echo "al día: $_v"; fi
		exit 0
	fi
	if [ "$_etiqueta" = "$(cat "$ESTADO/fallida" 2>/dev/null)" ]; then
		if [ -n "$_comprobar" ]; then echo "$_etiqueta ya falló; no se intenta (desplegada: ${_hay:-ninguna})"; fi
		exit 0
	fi
	if [ -n "$_comprobar" ]; then
		_buzon=no
		if precompilada_llegada "$BUZON" "$_v"; then _buzon=sí; fi
		echo "hay una versión nueva: $_v (desplegada: ${_hay:-ninguna}; en el buzón: $_buzon); se desplegaría"
		exit 0
	fi

	# La versión que dirá /salud es la de Cargo.toml: tiene que ser la de la
	# etiqueta, o el despliegue fallaría siempre.
	_cargo_v=$(en_git 'git show "$1:crates/semillero/Cargo.toml"' "refs/tags/$_etiqueta" 2>/dev/null | version_cargo)
	if [ "$_cargo_v" != "$_v" ]; then
		error "la versión de crates/semillero/Cargo.toml en $_etiqueta es «$(limpio "$_cargo_v")», no $_v; no se despliega"
		fallo_etiqueta "$_etiqueta"
		exit 1
	fi
	_commit=$(en_git 'git rev-parse --verify --quiet "$1^{commit}"' "refs/tags/$_etiqueta" 2>/dev/null)
	if ! commit_valido "$_commit"; then
		error "git no da un commit válido para $_etiqueta; no se hace nada"
		exit 1
	fi

	if [ ! -d "$RELEASES/$_v" ]; then
		_decision=compilar
		if [ -n "$PRECOMPILADAS_ESPERA" ]; then
			_decision=$(decidir "$_etiqueta" "$_v")
		fi
		if [ "$_decision" = esperar ]; then
			log "esperando la versión precompilada de $_etiqueta (hasta $PRECOMPILADAS_ESPERA min desde que se vio la etiqueta)"
			exit 0
		fi
		log "versión nueva $_v (desplegada: ${_hay:-ninguna})"
		# Restos de una ejecución cortada a medias.
		find "$RELEASES" -maxdepth 1 -name '.*.tmp' -exec rm -rf {} +
		if [ "$_decision" = instalar ] && instalar_precompilada "$_etiqueta" "$_v" "$_commit"; then
			_decision=instalada
		elif [ "$_decision" = compilar ] && [ -n "$PRECOMPILADAS_ESPERA" ]; then
			log "no ha llegado la versión precompilada de $_etiqueta; se compila aquí"
		fi
		if [ "$_decision" != instalada ]; then
			compilar_aqui "refs/tags/$_etiqueta" "$_v" "$_commit"
			case $? in
			0) ;;
			1)
				fallo_etiqueta "$_etiqueta"
				exit 1
				;;
			2) exit 0 ;;
			*) exit 1 ;;
			esac
		fi
	else
		log "versión nueva $_v (desplegada: ${_hay:-ninguna}; ya estaba en releases)"
	fi
	jail_la_ve "$_v" || exit 1
	if ! desplegar "$_v"; then
		fallo_etiqueta "$_etiqueta"
		exit 1
	fi
	rm -f "$ESTADO/fallida" "$ESTADO/intentos" "$ESTADO/precompilada"
	podar
}

# Una sola ejecución a la vez (-s: sin mensajes; -t 0: si hay otra, se va).
if [ -z "${SEMILLERO_ACT_BLOQUEADO:-}" ]; then
	mkdir -p "$(dirname "$CERROJO")"
	SEMILLERO_ACT_BLOQUEADO=1 exec lockf -s -t 0 "$CERROJO" /bin/sh "$0" "$@"
fi

main "$@"
