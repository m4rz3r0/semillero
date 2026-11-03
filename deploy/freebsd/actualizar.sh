#!/bin/sh
#
# Actualizador de semillero. Corre EN EL HOST (FreeBSD) como root; se instala
# como /usr/local/sbin/semillero-actualizar y lo lanza cron cada 15 minutos
# (/usr/local/etc/cron.d/semillero).
#
#   semillero-actualizar              despliega la última etiqueta vX.Y.Z
#   semillero-actualizar --comprobar  solo dice qué haría; no cambia nada
#
# El servidor nunca compila: solo instala versiones ya compiladas (la
# release de Forgejo que publica el runner del PC, o la de GitHub con su
# atestación). Si no hay ninguna, espera.
#
# Pasos:
# 1. Pregunta las etiquetas al repositorio (REPO_URL) con
#    «git ls-remote --tags» desde el host: sin clon, sin traer nada y sin
#    ejecutar nada del repositorio. Por ssh, con la clave de despliegue
#    CLAVE_DESPLIEGUE (de solo lectura en el servidor git) y la clave del
#    servidor git fijada en HOSTS_CONOCIDOS (StrictHostKeyChecking=yes,
#    BatchMode=yes, sin la configuración de ssh de root). Se queda con la
#    vX.Y.Z más alta (sort -V) y su commit (ultima_etiqueta_ls_remote,
#    deploy/lib/git.sh: el de la línea «^{}» si es anotada). Si es la
#    desplegada o una que ya falló dos veces seguidas (fichero «fallida»),
#    termina sin decir nada. La versión de Cargo.toml no se mira (haría
#    falta sacar el repositorio): si no es X.Y.Z, /salud lo dice al
#    desplegar (paso 4) y se vuelve atrás.
# 2. Si es nueva y se configuran FORGEJO_URL y FORGEJO_REPO, la trae de la
#    release vX.Y.Z de Forgejo (preparar_forgejo, deploy/lib/precompiladas.sh),
#    la que publica el runner de Forgejo Actions del PC
#    (.forgejo/workflows/freebsd.yml): pregunta por la release a la API
#    ($FORGEJO_URL/api/v1/repos/$FORGEJO_REPO/releases/tags/vX.Y.Z; un 404
#    es que aún no está) y descarga SHA256SUMS y el paquete de
#    $FORGEJO_URL/$FORGEJO_REPO/releases/download/vX.Y.Z/, con los nombres de
#    siempre, con curl y el token de lectura de FORGEJO_TOKEN_FICHERO por la
#    entrada estándar (nunca en los argumentos ni en la URL). Ese fichero
#    tiene que ser de root y que nadie más pueda leerlo; si no, Forgejo no
#    se usa (y el token no sale de ahí). Lo comprueba antes de instalarla:
#    sha256, solo ficheros y carpetas normales, una sola carpeta con
#    semillero, site/ y COMMIT, COMMIT igual al commit de la etiqueta y
#    semillero un ejecutable de FreeBSD x86-64. La deja en
#    /usr/local/semillero/releases/X.Y.Z (root; carpetas 755, ficheros 644,
#    el binario 755). Esa carpeta del host se monta (nullfs, solo lectura) en
#    la jail del servicio (JAIL) en /usr/local/semillero/releases. Necesita
#    curl (pkg install curl).
# 3. Si no, y se configura GITHUB_REPO (propietario/repositorio de GitHub),
#    la trae de la release vX.Y.Z de ese repositorio (preparar_github,
#    deploy/lib/precompiladas.sh): el paquete de FreeBSD y SHA256SUMS por
#    https con fetch, su atestación de la API de GitHub sin token y
#    «gh attestation verify --bundle», que tiene que ser de GITHUB_REPO, de
#    refs/tags/vX.Y.Z y firmada por .github/workflows/release.yml. Después,
#    las mismas comprobaciones que la de Forgejo salvo el commit (el del
#    repositorio público es otro: lo ata a la etiqueta la atestación), y la
#    web completa. Necesita gh y jq (pkg install gh jq).
#    Si ninguna de las dos está (o no cuadra), apunta una sola vez por
#    etiqueta «esperando la versión precompilada de X.Y.Z» y sale; lo vuelve
#    a probar en cada pasada, sin límite de tiempo y sin contarlo como
#    fallo. Un rechazo (sha256, paquete o atestación que no cuadran) es un
#    ERROR en el registro, una vez por etiqueta, fuente y motivo, y tampoco
#    cuenta como fallo. Forgejo sin configurar o sin token que se pueda usar
#    se dice una vez (hasta que cambie).
# 4. Para el servicio, copia la base de datos dentro de la jail JAIL como
#    el usuario semillero (/var/db/semillero/antes-de-actualizar), cambia de
#    golpe el enlace «current» de la jail (ln -sfh + mv -fh) y arranca. Espera
#    60 s como mucho a que /salud en ESCUCHA (preguntado desde dentro de la
#    jail: el cortafuegos puede no dejar llegar desde el host) responda
#    "ok":true y "version":"X.Y.Z", y a que quien escucha en el puerto sea
#    el binario de releases/X.Y.Z.
# 5. Si no: para, deja la base de datos como estaba, vuelve a la versión
#    anterior (o quita «current» si no había) y arranca; borra la versión
#    fallida. Tras 2 fallos seguidos de la misma etiqueta, la apunta en
#    /var/db/semillero-actualizar/fallida y deja de intentarlo hasta que haya
#    otra.
# Se conservan las 3 versiones más recientes; nunca se borra la desplegada.
#
# Configuración: /usr/local/etc/semillero-actualizar.conf (root, 600; la
# escribe deploy/freebsd/install.sh). Solo se lee si es de root y nadie más
# puede escribirla. REPO_URL y ESCUCHA no tienen valor por defecto: si faltan,
# no se hace nada y se dice cuál falta. Con REPO_URL por ssh, la clave de
# despliegue y el known_hosts tienen que ser de root y que nadie más pueda
# escribirlos (y la clave, ni leerla). Las claves de la configuración de
# antes (JAIL_COMPILAR, PRECOMPILADAS_ESPERA, TRABAJOS_COMPILACION,
# USUARIO_BUZON…) ya no se usan. Las funciones de deploy/lib/ van en
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
# Clave de despliegue (de solo lectura) y known_hosts con la clave del
# servidor git, en el host; solo se usan con REPO_URL por ssh.
CLAVE_DESPLIEGUE=/usr/local/etc/semillero-actualizar/id_ed25519
HOSTS_CONOCIDOS=/usr/local/etc/semillero-actualizar/known_hosts
JAIL=semillero
# Vacía: /usr/local/jails/containers/$JAIL.
JAIL_RAIZ=
# Forgejo (URL, sin barra al final) y su repositorio (propietario/nombre)
# de cuyas releases traer la versión que publica el runner del PC; con
# cualquiera de las dos vacía, no se mira Forgejo. El token de lectura, en
# un fichero de root que nadie más pueda leer.
FORGEJO_URL=
FORGEJO_REPO=
FORGEJO_TOKEN_FICHERO=/usr/local/etc/semillero-actualizar/forgejo-token
# Repositorio de GitHub (propietario/repositorio) del que traer la versión
# publicada, con su atestación; vacío, no se mira GitHub.
GITHUB_REPO=
# Dirección:puerto de escucha de semillero en la jail (la de su rc.d), p. ej.
# 192.0.2.10:8090 o [2001:db8::10]:8090.
ESCUCHA=

SERVICIO=semillero
USUARIO=semillero
RELEASES=$RAIZ/usr/local/semillero/releases
# Las mismas versiones vistas desde dentro de la jail (montaje nullfs).
RELEASES_JAIL=/usr/local/semillero/releases
DB_DIR=/var/db/semillero
DB_COPIA=$DB_DIR/antes-de-actualizar
ESTADO=$RAIZ/var/db/semillero-actualizar
LOG=$RAIZ/var/log/semillero-update.log
CERROJO=$RAIZ/var/run/semillero-actualizar.lock
ESPERA=${SEMILLERO_ACT_ESPERA:-60}
# Segundos como mucho de git ls-remote.
TIEMPO_GIT=120
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
	echo "uso: $0 [--comprobar]" >&2
	exit 2
}

# Texto que viene de fuera (el servidor git), recortado y sin nada raro,
# para el registro.
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

# Como fichero_seguro, y además que nadie más lo pueda leer (la clave de
# despliegue).
fichero_privado()
{
	fichero_seguro "$1" || return 1
	case $(ls -ldn -- "$1") in
	-???------*) return 0 ;;
	esac
	return 1
}

# Ruta absoluta sin nada que el shell de GIT_SSH_COMMAND pueda partir o
# interpretar.
ruta_limpia()
{
	case $1 in
	/*) ;;
	*) return 1 ;;
	esac
	case $1 in
	*[!A-Za-z0-9._/-]* | *..*) return 1 ;;
	esac
}

# Las etiquetas del repositorio: git ls-remote --tags de REPO_URL, desde el
# host. No clona, no trae objetos y no ejecuta nada del repositorio. Sin la
# configuración de git ni la de ssh de root, solo por ssh, https o una ruta
# local y sin preguntar nada (si pidiera contraseña, falla). Desde la
# carpeta de trabajo, vacía, y sin buscar un repositorio más arriba: git no
# mira ningún repositorio local.
ls_remoto()
{
	(
		cd "$TRABAJO" || exit 1
		if [ -n "$_ssh_orden" ]; then
			GIT_SSH_COMMAND=$_ssh_orden
			export GIT_SSH_COMMAND
		fi
		GIT_CEILING_DIRECTORIES=$(dirname "$TRABAJO") GIT_CONFIG_NOSYSTEM=1 \
			GIT_CONFIG_GLOBAL=/dev/null GIT_TERMINAL_PROMPT=0 GIT_ALLOW_PROTOCOL=ssh:https:file \
			timeout "$TIEMPO_GIT" git ls-remote --tags "$REPO_URL" 'v*'
	)
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

# /salud responde "ok":true con la versión de releases/$1 (X.Y.Z) y la sirve
# releases/$1, como mucho en $ESPERA segundos. fetch falla con cualquier
# respuesta que no sea 2xx.
sana()
{
	_ver=$1
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

# Un aviso sobre Forgejo (desactivado, sin curl, token que no vale) que se
# dice una sola vez: hasta que cambie o Forgejo vuelva a poderse usar.
avisar_forgejo()
{
	if [ "$(cat "$ESTADO/forgejo-aviso" 2>/dev/null)" = "$1" ]; then
		return 0
	fi
	printf '%s\n' "$1" >"$ESTADO/forgejo-aviso"
	log "$1"
}

# ¿Se puede usar Forgejo? Configurado (FORGEJO_URL y FORGEJO_REPO), con
# curl y con el token en un fichero de root que nadie más puede leer. Si
# no, deja el motivo en _fj_motivo y lo avisa una vez; el token no se lee.
forgejo_listo()
{
	_fj_motivo=
	if [ -z "$FORGEJO_URL" ] || [ -z "$FORGEJO_REPO" ]; then
		avisar_forgejo "Forgejo desactivado (FORGEJO_URL o FORGEJO_REPO vacías en $CONF): no se mira su release"
		return 1
	fi
	if ! command -v curl >/dev/null 2>&1; then
		_fj_motivo="falta curl (pkg install curl)"
	elif ! fichero_privado "$RAIZ$FORGEJO_TOKEN_FICHERO"; then
		_fj_motivo="FORGEJO_TOKEN_FICHERO ($FORGEJO_TOKEN_FICHERO) falta o no es un fichero de root que solo root pueda leer; no se usa"
	fi
	if [ -n "$_fj_motivo" ]; then
		avisar_forgejo "Forgejo no se puede usar: $_fj_motivo"
		return 1
	fi
	rm -f "$ESTADO/forgejo-aviso"
}

# Instala en releases/$2 la versión de la etiqueta $1 (commit $3) de la
# release de Forgejo (preparar_forgejo). Sale con 0 si queda instalada; 1 si
# aún no se puede usar (sin release, incompleta, sin red…: el motivo queda
# en _fj_motivo, para la línea de la espera) y 2 si se rechaza (lo dice como
# ERROR una vez por etiqueta y motivo). La release se queda en Forgejo: si
# no valía, se vuelve a mirar en cada pasada (la puede rehacer el runner) y
# se sigue con GitHub. Nunca queda nada instalado si no sale con 0.
instalar_forgejo()
{
	_fj_motivo=
	_tmp=$RELEASES/.$2.tmp
	rm -rf "$_tmp"
	_err=$(preparar_forgejo "$FORGEJO_URL" "$FORGEJO_REPO" "$RAIZ$FORGEJO_TOKEN_FICHERO" "$2" "$3" "$_tmp" 2>&1)
	_rf=$?
	_err=$(printf '%s' "${_err:-no se puede instalar}" | head -n 1)
	if [ "$_rf" -eq 0 ] && asentar "$_tmp" && mv "$_tmp" "$RELEASES/$2"; then
		log "instalada la versión de Forgejo de $1 ($FORGEJO_REPO, commit $(printf '%.12s' "$3"))"
		return 0
	fi
	rm -rf "$_tmp"
	if [ "$_rf" -eq 1 ]; then
		_fj_motivo=$_err
		return 1
	fi
	# El mismo rechazo de la misma release no se repite en cada pasada.
	if [ "$(cat "$ESTADO/forgejo-rechazada" 2>/dev/null)" != "$1 $_err" ]; then
		printf '%s\n' "$1 $_err" >"$ESTADO/forgejo-rechazada"
		error "versión de Forgejo de $1 rechazada: $_err"
	fi
	return 2
}

# Instala en releases/$2 la versión de la etiqueta $1 publicada en GitHub
# (GITHUB_REPO), con su atestación comprobada (preparar_github). Sale con 0
# si queda instalada; 1 si aún no se puede usar (sin release, sin red…: el
# motivo queda en _gh_motivo, para la línea de la espera) y 2 si se rechaza
# (lo dice como ERROR una vez por etiqueta y motivo). Nunca queda nada
# instalado si no sale con 0.
instalar_github()
{
	_gh_motivo=
	_tmp=$RELEASES/.$2.tmp
	rm -rf "$_tmp"
	_err=$(preparar_github "$GITHUB_REPO" "$2" "$_tmp" 2>&1)
	_rg=$?
	_err=$(printf '%s' "${_err:-no se puede instalar}" | head -n 1)
	if [ "$_rg" -eq 0 ] && asentar "$_tmp" && mv "$_tmp" "$RELEASES/$2"; then
		log "instalada la versión de GitHub de $1 ($GITHUB_REPO, atestación comprobada)"
		return 0
	fi
	rm -rf "$_tmp"
	if [ "$_rg" -eq 1 ]; then
		_gh_motivo=$_err
		return 1
	fi
	# El mismo rechazo de la misma release no se repite en cada pasada.
	if [ "$(cat "$ESTADO/github-rechazada" 2>/dev/null)" != "$1 $_err" ]; then
		printf '%s\n' "$1 $_err" >"$ESTADO/github-rechazada"
		error "versión de GitHub de $1 rechazada: $_err"
	fi
	return 2
}

# Ninguna versión precompilada de la etiqueta $1 (versión $2) se puede usar
# todavía: se dice una vez por etiqueta y se reintenta en la pasada
# siguiente, sin límite y sin contarlo como fallo.
esperar()
{
	if [ "$(cat "$ESTADO/esperando" 2>/dev/null)" = "$1" ]; then
		return 0
	fi
	echo "$1" >"$ESTADO/esperando"
	_fuentes=
	if [ -n "$FORGEJO_URL" ] && [ -n "$FORGEJO_REPO" ]; then _fuentes="Forgejo $FORGEJO_REPO"; fi
	if [ -n "$GITHUB_REPO" ]; then _fuentes="${_fuentes:+$_fuentes o }GitHub $GITHUB_REPO"; fi
	_fuentes=${_fuentes:-ninguna fuente configurada: FORGEJO_URL y FORGEJO_REPO o GITHUB_REPO}
	if [ -n "${_fj_motivo:-}" ]; then _fuentes="$_fuentes; Forgejo: $_fj_motivo"; fi
	if [ -n "${_gh_motivo:-}" ]; then _fuentes="$_fuentes; GitHub: $_gh_motivo"; fi
	log "esperando la versión precompilada de $2 ($_fuentes); se reintenta en cada pasada"
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

main()
{
	_comprobar=
	case "$#:${1:-}" in
	1:--comprobar) _comprobar=1 ;;
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
	case $JAIL in
	'' | *[!A-Za-z0-9_-]*)
		error "JAIL no válida"
		exit 1
		;;
	esac
	JAIL_RAIZ=${JAIL_RAIZ:-/usr/local/jails/containers/$JAIL}
	case $JAIL_RAIZ in
	/*) ;;
	*)
		error "JAIL_RAIZ tiene que ser una ruta absoluta"
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
	# shellcheck source=../lib/git.sh
	. "$RAIZ$LIBEXEC/git.sh"
	if [ -n "$GITHUB_REPO" ] && ! repo_github_valido "$GITHUB_REPO"; then
		error "GITHUB_REPO tiene que ser propietario/repositorio de GitHub (p. ej. ejemplo/semillero) o vacía"
		exit 1
	fi
	FORGEJO_URL=${FORGEJO_URL%/}
	if [ -n "$FORGEJO_URL" ] && ! url_forgejo_valida "$FORGEJO_URL"; then
		error "FORGEJO_URL tiene que ser la URL de Forgejo (p. ej. https://forja.example) o vacía"
		exit 1
	fi
	if [ -n "$FORGEJO_REPO" ] && ! repo_forgejo_valido "$FORGEJO_REPO"; then
		error "FORGEJO_REPO tiene que ser propietario/nombre del repositorio en Forgejo (p. ej. ejemplo/semillero) o vacía"
		exit 1
	fi
	if ! ruta_limpia "$FORGEJO_TOKEN_FICHERO"; then
		error "FORGEJO_TOKEN_FICHERO tiene que ser una ruta absoluta sin espacios ni caracteres raros"
		exit 1
	fi
	# Por ssh: la clave de despliegue y la del servidor git, del host.
	_ssh_orden=
	if repo_por_ssh "$REPO_URL"; then
		if ! ruta_limpia "$CLAVE_DESPLIEGUE" || ! ruta_limpia "$RAIZ$CLAVE_DESPLIEGUE"; then
			error "CLAVE_DESPLIEGUE tiene que ser una ruta absoluta sin espacios ni caracteres raros; no se hace nada"
			exit 1
		fi
		if ! ruta_limpia "$HOSTS_CONOCIDOS" || ! ruta_limpia "$RAIZ$HOSTS_CONOCIDOS"; then
			error "HOSTS_CONOCIDOS tiene que ser una ruta absoluta sin espacios ni caracteres raros; no se hace nada"
			exit 1
		fi
		if ! fichero_privado "$RAIZ$CLAVE_DESPLIEGUE"; then
			error "la clave de despliegue CLAVE_DESPLIEGUE ($CLAVE_DESPLIEGUE) falta o no es un fichero de root que solo root pueda leer (se crea con deploy/freebsd/install.sh); no se hace nada"
			exit 1
		fi
		if ! fichero_seguro "$RAIZ$HOSTS_CONOCIDOS" || [ ! -s "$RAIZ$HOSTS_CONOCIDOS" ]; then
			error "falta HOSTS_CONOCIDOS ($HOSTS_CONOCIDOS), la clave del servidor git, o no es un fichero de root que nadie más pueda escribir (ver deploy/README.md); no se hace nada"
			exit 1
		fi
		_ssh_orden="ssh -F /dev/null -i $RAIZ$CLAVE_DESPLIEGUE -o IdentitiesOnly=yes"
		_ssh_orden="$_ssh_orden -o StrictHostKeyChecking=yes -o UserKnownHostsFile=$RAIZ$HOSTS_CONOCIDOS"
		_ssh_orden="$_ssh_orden -o GlobalKnownHostsFile=/dev/null -o BatchMode=yes -o ConnectTimeout=15"
	fi
	for _orden in git tar jexec sockstat file timeout; do
		command -v "$_orden" >/dev/null 2>&1 || {
			error "falta $_orden"
			exit 1
		}
	done

	# Global con nombre propio: el trap la borra siempre.
	TRABAJO=$(mktemp -d) || exit 1
	trap 'rm -rf "$TRABAJO"' EXIT
	trap 'exit 143' INT TERM

	if [ -n "$_comprobar" ]; then
		if [ -z "$FORGEJO_URL" ] || [ -z "$FORGEJO_REPO" ]; then
			echo "versiones de Forgejo: desactivado (FORGEJO_URL o FORGEJO_REPO vacías)"
		elif ! command -v curl >/dev/null 2>&1; then
			echo "versiones de Forgejo: $FORGEJO_URL $FORGEJO_REPO (falta curl; pkg install curl)"
		elif ! fichero_privado "$RAIZ$FORGEJO_TOKEN_FICHERO"; then
			echo "versiones de Forgejo: $FORGEJO_URL $FORGEJO_REPO (FORGEJO_TOKEN_FICHERO falta o no es de root y solo suyo)"
		else
			echo "versiones de Forgejo: $FORGEJO_URL $FORGEJO_REPO (curl y token listos)"
		fi
	fi
	if [ -n "$_comprobar" ] && [ -n "$GITHUB_REPO" ]; then
		_faltan=
		for _o in gh jq; do
			command -v "$_o" >/dev/null 2>&1 || _faltan="$_faltan $_o"
		done
		if [ -n "$_faltan" ]; then
			echo "versiones de GitHub: $GITHUB_REPO (falta:$_faltan; pkg install$_faltan)"
		else
			echo "versiones de GitHub: $GITHUB_REPO (gh y jq instalados)"
		fi
	fi
	if ! _lista=$(ls_remoto 2>"$TRABAJO/err"); then
		_e=$(grep -v '^[[:space:]]*$' "$TRABAJO/err" | head -n 1 | cut -c1-200)
		if [ -n "$_comprobar" ]; then
			echo "sin acceso al repositorio: $_e" >&2
			exit 1
		fi
		log "sin acceso al repositorio: $_e"
		exit 0
	fi
	_linea=$(printf '%s\n' "$_lista" | ultima_etiqueta_ls_remote)
	if [ -z "$_linea" ]; then
		if [ -n "$_comprobar" ]; then echo "no hay ninguna etiqueta vX.Y.Z"; fi
		exit 0
	fi
	_etiqueta=${_linea%% *}
	_commit=${_linea#* }
	# Lo que dice el servidor git no es de fiar: el nombre acaba en rutas que
	# root borra.
	if ! etiqueta_valida "$_etiqueta"; then
		error "la última etiqueta no es vX.Y.Z válida ($(limpio "$_etiqueta"))"
		exit 1
	fi
	if ! commit_valido "$_commit"; then
		error "git ls-remote no da un commit válido para $_etiqueta; no se hace nada"
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
		echo "hay una versión nueva: $_v (desplegada: ${_hay:-ninguna}); se desplegaría en cuanto haya una versión precompilada que cuadre"
		exit 0
	fi

	if [ ! -d "$RELEASES/$_v" ]; then
		# Restos de una ejecución cortada a medias.
		find "$RELEASES" -maxdepth 1 -name '.*.tmp' -exec rm -rf {} +
		_instalada=
		_rechazada=
		_fj_motivo=
		if forgejo_listo; then
			instalar_forgejo "$_etiqueta" "$_v" "$_commit"
			case $? in
			0) _instalada=1 ;;
			2) _rechazada=1 ;;
			esac
		fi
		_gh_motivo=
		if [ -z "$_instalada" ] && [ -n "$GITHUB_REPO" ]; then
			instalar_github "$_etiqueta" "$_v"
			case $? in
			0) _instalada=1 ;;
			2) _rechazada=1 ;;
			esac
		fi
		if [ -z "$_instalada" ]; then
			esperar "$_etiqueta" "$_v"
			if [ -n "$_rechazada" ]; then exit 1; fi
			exit 0
		fi
	else
		log "versión nueva $_v (desplegada: ${_hay:-ninguna}; ya estaba en releases)"
	fi
	jail_la_ve "$_v" || exit 1
	if ! desplegar "$_v"; then
		fallo_etiqueta "$_etiqueta"
		exit 1
	fi
	# «precompilada» es el estado de la espera de antes (PRECOMPILADAS_ESPERA).
	rm -f "$ESTADO/fallida" "$ESTADO/intentos" "$ESTADO/esperando" \
		"$ESTADO/github-rechazada" "$ESTADO/forgejo-rechazada" "$ESTADO/precompilada"
	podar
}

# Una sola ejecución a la vez (-s: sin mensajes; -t 0: si hay otra, se va).
if [ -z "${SEMILLERO_ACT_BLOQUEADO:-}" ]; then
	mkdir -p "$(dirname "$CERROJO")"
	SEMILLERO_ACT_BLOQUEADO=1 exec lockf -s -t 0 "$CERROJO" /bin/sh "$0" "$@"
fi

main "$@"
