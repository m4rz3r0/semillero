# shellcheck shell=sh
#
# Versiones de semillero para FreeBSD: las compila el runner de Forgejo
# Actions del PC (.forgejo/workflows/freebsd.yml, con deploy/cross/) y las
# publica como release de Forgejo de la etiqueta, o las publica GitHub; el
# servidor nunca compila. Se carga con «.»; solo funciones, sin efectos al
# cargarse. Las usan el actualizador (deploy/freebsd/actualizar.sh, en el
# host como root), los guiones del PC (deploy/cross/) y las pruebas
# (deploy/pruebas/).
#
# El paquete de la versión X.Y.Z (etiqueta vX.Y.Z):
# semillero-X.Y.Z-x86_64-unknown-freebsd.tar.gz, con una única carpeta del
# mismo nombre (sin .tar.gz) que trae semillero (el binario), site/ (la web
# de cargo-leptos), COMMIT (el commit completo de la etiqueta) y LICENSE si
# la hay; y SHA256SUMS, una línea «<sha256>  <archivo>» como la escribe
# sha256sum. SHA256SUMS se sube el último: con él, la versión está completa.
#
# La release de Forgejo vX.Y.Z (preparar_forgejo) trae esos dos ficheros;
# se descargan con un token de solo lectura que va a curl por la entrada
# estándar, nunca en los argumentos ni en la URL.
#
# Si se configura un repositorio de GitHub (GITHUB_REPO del actualizador),
# la misma versión se puede traer de su release vX.Y.Z (preparar_github):
# el paquete, SHA256SUMS (una línea por paquete publicado) y la atestación
# de origen, que comprueba «gh attestation verify».

PRECOMPILADA_OBJETIVO=x86_64-unknown-freebsd

# ¿Es $1 una versión X.Y.Z? Solo cifras y dos puntos, sin partes vacías y
# como mucho 40 caracteres (es un componente de ruta en releases/ y en las
# URL de las releases).
version_valida()
{
	case $1 in
	'' | *[!0-9.]* | *..* | *. | .*) return 1 ;;
	esac
	[ "$(printf '%s' "$1" | tr -cd . | wc -c | tr -d ' ')" -eq 2 ] && [ "${#1}" -le 40 ]
}

# Nombre del paquete de la versión $1.
archivo_precompilada()
{
	echo "semillero-$1-$PRECOMPILADA_OBJETIVO.tar.gz"
}

# Las etiquetas vX.Y.Z de la entrada estándar (una por línea), sin las demás.
solo_versiones()
{
	grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' || true
}

# La más reciente de las etiquetas que llegan por la entrada estándar: la
# última vX.Y.Z según sort -V, la misma que elige ultima_etiqueta_ls_remote
# (deploy/lib/git.sh) en el servidor.
ultima_version()
{
	solo_versiones | sort -V | tail -n 1
}

# La versión (version = "…") del [package] de un Cargo.toml que llega por la
# entrada estándar.
version_cargo()
{
	sed -n '/^\[package\]/,/^\[/s/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1
}

# sha256 de $1 en hexadecimal (sha256sum en Linux y en FreeBSD 14+; si no,
# sha256 -q de FreeBSD).
suma_sha256()
{
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum <"$1" | cut -d' ' -f1
	else
		sha256 -q <"$1"
	fi
}

# ¿Es $1 un ejecutable de FreeBSD x86-64 (según file)?
binario_freebsd()
{
	case $(file -b "$1" 2>/dev/null) in
	ELF\ 64-bit\ LSB\ *executable,\ x86-64,*FreeBSD*) return 0 ;;
	esac
	return 1
}

# ¿Trae la web $1 (target/site de cargo-leptos) hash.txt y los ficheros con
# hash de semillero que nombra (pkg/semillero.<hash>.{js,wasm,css})? Escribe
# lo que falta en la salida de error.
web_completa()
{
	if [ ! -f "$1/hash.txt" ]; then
		echo "falta site/hash.txt" >&2
		return 1
	fi
	for _ext in js wasm css; do
		_h=$(sed -n "s/^$_ext: *//p" "$1/hash.txt" | head -n 1)
		case $_h in
		'' | *[!A-Za-z0-9_-]*)
			echo "site/hash.txt no da el hash de $_ext" >&2
			return 1
			;;
		esac
		if [ ! -f "$1/pkg/semillero.$_h.$_ext" ]; then
			echo "falta site/pkg/semillero.$_h.$_ext" >&2
			return 1
		fi
	done
}

# Comprueba la lista del paquete $1 antes de desempaquetarlo: todo bajo $2/
# (se admite un «./» delante), sin rutas absolutas ni «..», y solo ficheros y
# carpetas normales (ni enlaces simbólicos ni duros). $3 es un fichero de
# trabajo.
lista_segura()
{
	tar -tzf "$1" >"$3" 2>/dev/null || return 1
	[ -s "$3" ] || return 1
	while IFS= read -r _n; do
		_n=${_n#./}
		_n=${_n%/}
		case $_n in
		"$2") ;;
		"$2"/*)
			case "/$_n/" in
			*/../* | *//*) return 1 ;;
			esac
			;;
		*) return 1 ;;
		esac
	done <"$3"
	tar -tvzf "$1" >"$3" 2>/dev/null || return 1
	while IFS= read -r _l; do
		case $_l in
		[d-]*) ;;
		*) return 1 ;;
		esac
		case $_l in
		*' -> '* | *' link to '*) return 1 ;;
		esac
	done <"$3"
}

# Comprueba el paquete de la versión $2 que está, con su SHA256SUMS, en la
# carpeta de trabajo $1 (propia, de quien comprueba), y lo deja desempaquetado
# en $4 (que no debe existir) con semillero, site/ y COMMIT y nada más. $3 es
# el commit que tiene que traer COMMIT; vacío solo con $5 = github: entonces
# vale cualquier commit (el del repositorio público es otro: lo que ata el
# paquete a la etiqueta es la atestación, ya comprobada) y además la web
# tiene que estar completa (web_completa). Lo demás que traiga la carpeta
# (LICENSE, README.md, rc.d/…) no se instala. Borra $1 siempre; si algo no
# cuadra, escribe el motivo en la salida de error, sale con 1 y no crea $4.
comprobar_paquete()
{
	_trabajo=$1
	_ver=$2
	_commit=$3
	_dst=$4
	_origen=${5:-}
	_arch=$(archivo_precompilada "$_ver")
	_raiz=${_arch%.tar.gz}
	# SHA256SUMS: exactamente una línea, la del paquete, con 64 cifras
	# hexadecimales en minúscula.
	_re=$(printf '%s' "$_arch" | sed 's/\./\\./g')
	_esperada=$(sed -n "s/^\([0-9a-f]\{64\}\)  $_re\$/\1/p" "$_trabajo/SHA256SUMS")
	if [ "$(wc -l <"$_trabajo/SHA256SUMS" | tr -d ' ')" -ne 1 ] || [ "${#_esperada}" -ne 64 ]; then
		echo "SHA256SUMS no es una sola línea «<sha256>  $_arch»" >&2
		rm -rf "$_trabajo"
		return 1
	fi
	if [ "$(suma_sha256 "$_trabajo/$_arch")" != "$_esperada" ]; then
		echo "el sha256 de $_arch no coincide con SHA256SUMS" >&2
		rm -rf "$_trabajo"
		return 1
	fi
	if ! lista_segura "$_trabajo/$_arch" "$_raiz" "$_trabajo/lista"; then
		echo "el paquete trae algo que no es fichero ni directorio normal, una ruta absoluta o con «..», o algo fuera de $_raiz/" >&2
		rm -rf "$_trabajo"
		return 1
	fi
	mkdir "$_trabajo/x" || return 1
	# Como root, tar conservaría dueños y modos del archivo (setuid incluido).
	if ! tar -xzf "$_trabajo/$_arch" -C "$_trabajo/x" --no-same-owner --no-same-permissions 2>/dev/null; then
		echo "no se puede desempaquetar $_arch" >&2
		rm -rf "$_trabajo"
		return 1
	fi
	# Lo mismo, ya en disco.
	if [ -n "$(find "$_trabajo/x" ! -type f ! -type d)" ] ||
		[ -n "$(find "$_trabajo/x" -type f -links +1)" ]; then
		echo "el paquete trae algo que no es fichero ni directorio, o enlaces duros" >&2
		rm -rf "$_trabajo"
		return 1
	fi
	if [ "$(find "$_trabajo/x" -mindepth 1 -maxdepth 1 | wc -l)" -ne 1 ] ||
		[ ! -d "$_trabajo/x/$_raiz" ]; then
		echo "el paquete no trae solo la carpeta $_raiz" >&2
		rm -rf "$_trabajo"
		return 1
	fi
	_d=$_trabajo/x/$_raiz
	if [ ! -f "$_d/semillero" ] || [ ! -d "$_d/site" ] || [ ! -f "$_d/COMMIT" ]; then
		echo "al paquete le faltan semillero, site/ o COMMIT" >&2
		rm -rf "$_trabajo"
		return 1
	fi
	if [ "$_origen" = github ] && [ -z "$_commit" ]; then
		:
	elif [ "$(head -n 1 "$_d/COMMIT")" != "$_commit" ]; then
		echo "el paquete es de otro commit ($(head -n 1 "$_d/COMMIT" | tr -cd '0-9a-f' | cut -c1-12)), no del de v$_ver" >&2
		rm -rf "$_trabajo"
		return 1
	fi
	if ! binario_freebsd "$_d/semillero"; then
		echo "semillero no es un ejecutable de FreeBSD x86-64 ($(file -b "$_d/semillero" 2>/dev/null | cut -c1-60))" >&2
		rm -rf "$_trabajo"
		return 1
	fi
	if [ "$_origen" = github ] && ! web_completa "$_d/site"; then
		rm -rf "$_trabajo"
		return 1
	fi
	if ! mkdir "$_dst" || ! mv "$_d/semillero" "$_d/site" "$_d/COMMIT" "$_dst/"; then
		echo "no se puede preparar $_dst" >&2
		rm -rf "$_trabajo" "$_dst"
		return 1
	fi
	rm -rf "$_trabajo"
}

# ¿Es $1 un repositorio de GitHub «propietario/nombre»? El propietario, de
# letras, cifras y «-» (39 como mucho); el nombre, también con «.» y «_»
# (100 como mucho); ninguno empieza por «-» ni el nombre es «.» o «..».
repo_github_valido()
{
	case $1 in
	*/*/* | -* | */-* | */ | /* | */. | */.. | *[!A-Za-z0-9._/-]*) return 1 ;;
	*/*) ;;
	*) return 1 ;;
	esac
	_propietario=${1%%/*}
	_nombre=${1#*/}
	case $_propietario in
	*[!A-Za-z0-9-]*) return 1 ;;
	esac
	[ "${#_propietario}" -le 39 ] && [ "${#_nombre}" -le 100 ]
}

# Última línea no vacía del fichero $1, corta y solo con caracteres
# corrientes, para llevarla al registro: sin lo que va detrás de un «?» en
# una dirección (por si llevara algo que no debe verse).
motivo_corto()
{
	grep -v '^[[:space:]]*$' "$1" 2>/dev/null | tail -n 1 | sed 's/?[^[:space:]]*//g' |
		tr -cd '[:alnum:][:space:].,:;()/_-' | cut -c1-160
}

# ¿Es $1 un repositorio de Forgejo «propietario/nombre»? Letras, cifras,
# «.», «_» y «-», sin empezar por «.» ni «-», y 100 caracteres como mucho
# cada parte.
repo_forgejo_valido()
{
	case $1 in
	*/*/* | */ | /* | .* | -* | */.* | */-* | *[!A-Za-z0-9._/-]*) return 1 ;;
	*/*) ;;
	*) return 1 ;;
	esac
	_fprop=${1%%/*}
	_fnombre=${1#*/}
	[ "${#_fprop}" -le 100 ] && [ "${#_fnombre}" -le 100 ]
}

# ¿Es $1 la URL de Forgejo? http:// o https://, un equipo y, si acaso,
# puerto y ruta; sin barra al final, sin espacios ni nada que curl o su
# configuración puedan leer de otro modo.
url_forgejo_valida()
{
	case $1 in
	http://?* | https://?*) ;;
	*) return 1 ;;
	esac
	case ${1#*://} in
	/* | */ | *//* | *..* | *[!A-Za-z0-9.:/_\[\]-]*) return 1 ;;
	esac
	[ "${#1}" -le 200 ]
}

# Descarga $1 (https) en $2 con fetch: como mucho $3 bytes y 15 minutos
# (60 s sin datos). Sale con 1 si fetch falla o si trae más de $3 bytes, y
# deja el motivo en DESCARGA_MOTIVO (lo que dice fetch en su salida de error).
descargar_https()
{
	_salida=$2
	DESCARGA_MOTIVO=
	{
		timeout 900 fetch -q -T 60 -o - "$1" 2>"$_salida.err"
		echo $? >"$_salida.rc"
	} | head -c "$(($3 + 1))" >"$_salida"
	_rc=$(cat "$_salida.rc" 2>/dev/null)
	_bytes=$(wc -c <"$_salida" | tr -d ' ')
	if [ "$_bytes" -gt "$3" ]; then
		DESCARGA_MOTIVO="trae más de $3 bytes"
	elif [ "$_rc" = 124 ]; then
		DESCARGA_MOTIVO="fetch no termina en 15 minutos"
	elif [ "$_rc" != 0 ]; then
		DESCARGA_MOTIVO=$(motivo_corto "$_salida.err")
		DESCARGA_MOTIVO=${DESCARGA_MOTIVO:-fetch sale con ${_rc:-?}}
	fi
	rm -f "$_salida.rc" "$_salida.err"
	[ -z "$DESCARGA_MOTIVO" ]
}

# curl a Forgejo con la cabecera del token ($_ftoken) por la entrada
# estándar («-K -»): nunca en los argumentos (que ve cualquiera con ps) ni
# en la URL. Solo http y https; el resto de argumentos, los de quien llama.
_curl_forgejo()
{
	printf 'header = "Authorization: token %s"\n' "$_ftoken" |
		curl -K - --silent --show-error --proto =http,https --connect-timeout 30 "$@"
}

# Descarga $1 de Forgejo en $2: como mucho $3 bytes y 15 minutos. Sale con
# 1 si curl falla (también con un 404: la release aún no lo trae) o si trae
# más de $3 bytes.
descargar_forgejo()
{
	_salida=$2
	{
		_curl_forgejo --fail --max-time 900 -o - "$1" 2>/dev/null
		echo $? >"$_salida.rc"
	} | head -c "$(($3 + 1))" >"$_salida"
	_rc=$(cat "$_salida.rc" 2>/dev/null)
	rm -f "$_salida.rc"
	[ "$_rc" = 0 ] && [ "$(wc -c <"$_salida" | tr -d ' ')" -le "$3" ]
}

# Trae de la release v$4 del repositorio $2 de Forgejo ($1) la versión $4
# para FreeBSD y la deja desempaquetada en $6 (que no debe existir), con
# semillero, site/ y COMMIT y nada más. $3 es el fichero del token de
# lectura (quien llama comprueba que es de root y que nadie más lo lee) y
# $5 el commit completo de la etiqueta (git ls-remote del servidor): la
# release la compila el runner de esa misma etiqueta, así que COMMIT tiene
# que ser ese. Pasos, en una carpeta de trabajo propia:
# 1. GET $1/api/v1/repos/$2/releases/tags/v$4: un 404 es que aún no está.
#    De la respuesta no se lee nada: solo cuenta el código.
# 2. Descarga SHA256SUMS y el paquete de $1/$2/releases/download/v$4/, con
#    los nombres de siempre (archivo_precompilada), nunca los que diga
#    Forgejo. Si falta alguno, aún no está completa.
# 3. comprobar_paquete: SHA256SUMS de una sola línea, sha256, solo ficheros
#    y carpetas normales, una sola carpeta, COMMIT el de la etiqueta y
#    semillero un ejecutable de FreeBSD x86-64.
# Nada de lo descargado se ejecuta. Sale con 0 si queda en $6; 1 si no se
# puede usar (aún no publicada o incompleta, sin red, token que no vale,
# falta curl); 2 si se rechaza (sumas o paquete que no cuadran). En los dos
# casos dice el motivo en la salida de error y no deja $6.
# FORGEJO_MAX_PAQUETE: bytes como mucho del paquete.
preparar_forgejo()
{
	_furl=$1
	_frepo=$2
	_ftoken_f=$3
	_fver=$4
	_fcommit=$5
	_fdst=$6
	_fmax=${FORGEJO_MAX_PAQUETE:-268435456}
	case $_fmax in
	'' | *[!0-9]*) _fmax=268435456 ;;
	esac
	if ! url_forgejo_valida "$_furl" || ! repo_forgejo_valido "$_frepo"; then
		echo "URL o repositorio de Forgejo no válidos" >&2
		return 2
	fi
	if ! version_valida "$_fver"; then
		echo "versión no válida" >&2
		return 2
	fi
	case $_fcommit in
	'' | *[!0-9a-f]*)
		echo "commit no válido" >&2
		return 2
		;;
	esac
	if [ "${#_fcommit}" -ne 40 ]; then
		echo "commit no válido" >&2
		return 2
	fi
	if ! command -v curl >/dev/null 2>&1; then
		echo "falta curl (pkg install curl)" >&2
		return 1
	fi
	# El token: la primera línea, sin espacios; solo caracteres que no
	# rompen la configuración de curl.
	_ftoken=$(head -n 1 "$_ftoken_f" 2>/dev/null | tr -d ' \t\r')
	case $_ftoken in
	'' | *[!A-Za-z0-9._-]*)
		echo "el token de Forgejo está vacío o no vale" >&2
		_ftoken=
		return 1
		;;
	esac
	_farch=$(archivo_precompilada "$_fver")
	_ftrabajo=$(mktemp -d) || return 1
	_fcod=$(_curl_forgejo --max-time 60 --max-filesize 1048576 -o "$_ftrabajo/release.json" \
		-w '%{http_code}' "$_furl/api/v1/repos/$_frepo/releases/tags/v$_fver" 2>/dev/null) || _fcod=000
	case $_fcod in
	200) ;;
	404)
		echo "la release v$_fver aún no está publicada" >&2
		_ftoken=
		rm -rf "$_ftrabajo"
		return 1
		;;
	401 | 403)
		echo "Forgejo no deja leer la release v$_fver (HTTP $_fcod: ¿token sin permiso de lectura?)" >&2
		_ftoken=
		rm -rf "$_ftrabajo"
		return 1
		;;
	*)
		echo "no se puede preguntar a Forgejo por la release v$_fver (HTTP $(printf '%s' "$_fcod" | tr -cd 0-9 | cut -c1-3))" >&2
		_ftoken=
		rm -rf "$_ftrabajo"
		return 1
		;;
	esac
	_fdesc=$_furl/$_frepo/releases/download/v$_fver
	mkdir "$_ftrabajo/paquete" || {
		rm -rf "$_ftrabajo"
		return 1
	}
	if ! descargar_forgejo "$_fdesc/SHA256SUMS" "$_ftrabajo/paquete/SHA256SUMS" 65536 ||
		! descargar_forgejo "$_fdesc/$_farch" "$_ftrabajo/paquete/$_farch" "$_fmax"; then
		echo "la release v$_fver aún no trae SHA256SUMS y $_farch (o no se pueden descargar)" >&2
		_ftoken=
		rm -rf "$_ftrabajo"
		return 1
	fi
	_ftoken=
	if ! comprobar_paquete "$_ftrabajo/paquete" "$_fver" "$_fcommit" "$_fdst"; then
		rm -rf "$_ftrabajo"
		return 2
	fi
	rm -rf "$_ftrabajo"
}

# Trae de la release v$2 del repositorio de GitHub $1 la versión $2 para
# FreeBSD y la deja desempaquetada en $3 (que no debe existir), como
# preparar_forgejo. Pasos, en una carpeta de trabajo propia:
# 1. Descarga el paquete y SHA256SUMS de
#    https://github.com/$1/releases/download/v$2/ y comprueba el sha256 con
#    la línea del paquete (SHA256SUMS trae una por paquete publicado).
# 2. Trae, sin token, las atestaciones de ese sha256 de
#    https://api.github.com/repos/$1/attestations/sha256:<sha256> y saca con
#    jq cada «bundle» a una línea (JSON lines, lo que acepta gh --bundle).
# 3. gh attestation verify --bundle (sin conexión a la API de GitHub, que sin
#    sesión gh no usa): la atestación tiene que ser de $1, de la etiqueta
#    refs/tags/v$2 y firmada por .github/workflows/release.yml.
# 4. comprobar_paquete con origen github: todo lo de las precompiladas salvo
#    que COMMIT sea el de la etiqueta privada; además, la web completa.
# Nada de lo descargado se ejecuta: jq y gh solo lo leen. Sale con 0 si
# queda en $3; 1 si no se puede usar (aún no publicada, sin red, sin
# atestación, falta gh o jq, gh no puede hacer la comprobación); 2 si se
# rechaza (sumas, atestación o paquete que no cuadran). En los dos casos dice el motivo en la salida de error y
# no deja $3. GITHUB_MAX_PAQUETE: bytes como mucho del paquete.
preparar_github()
{
	_grepo=$1
	_gver=$2
	_gdst=$3
	_gmax=${GITHUB_MAX_PAQUETE:-268435456}
	case $_gmax in
	'' | *[!0-9]*) _gmax=268435456 ;;
	esac
	if ! repo_github_valido "$_grepo"; then
		echo "repositorio de GitHub no válido" >&2
		return 2
	fi
	if ! version_valida "$_gver"; then
		echo "versión no válida" >&2
		return 2
	fi
	for _go in gh jq fetch; do
		if ! command -v "$_go" >/dev/null 2>&1; then
			echo "falta $_go (pkg install gh jq)" >&2
			return 1
		fi
	done
	_garch=$(archivo_precompilada "$_gver")
	_gurl=https://github.com/$_grepo/releases/download/v$_gver
	_gtrabajo=$(mktemp -d) || return 1
	if ! descargar_https "$_gurl/$_garch" "$_gtrabajo/$_garch" "$_gmax" ||
		! descargar_https "$_gurl/SHA256SUMS" "$_gtrabajo/SHA256SUMS.github" 65536; then
		echo "no se puede descargar $_garch o SHA256SUMS de la release v$_gver de github.com/$_grepo (¿aún no está publicada?): $DESCARGA_MOTIVO" >&2
		rm -rf "$_gtrabajo"
		return 1
	fi
	_gre=$(printf '%s' "$_garch" | sed 's/\./\\./g')
	_gesperada=$(sed -n "s/^\([0-9a-f]\{64\}\)  $_gre\$/\1/p" "$_gtrabajo/SHA256SUMS.github")
	if [ "${#_gesperada}" -ne 64 ]; then
		echo "SHA256SUMS de GitHub no tiene una sola línea «<sha256>  $_garch»" >&2
		rm -rf "$_gtrabajo"
		return 2
	fi
	_gsuma=$(suma_sha256 "$_gtrabajo/$_garch")
	if [ "$_gsuma" != "$_gesperada" ]; then
		echo "el sha256 de $_garch no coincide con SHA256SUMS de GitHub" >&2
		rm -rf "$_gtrabajo"
		return 2
	fi
	if ! descargar_https "https://api.github.com/repos/$_grepo/attestations/sha256:$_gsuma" \
		"$_gtrabajo/atestaciones.json" 16777216; then
		echo "no se pueden traer las atestaciones de $_garch de api.github.com (¿no tiene?): $DESCARGA_MOTIVO" >&2
		rm -rf "$_gtrabajo"
		return 1
	fi
	if ! jq -c '.attestations | arrays | .[] | objects | .bundle | objects' \
		<"$_gtrabajo/atestaciones.json" >"$_gtrabajo/atestaciones.jsonl" 2>/dev/null ||
		[ ! -s "$_gtrabajo/atestaciones.jsonl" ]; then
		echo "api.github.com no da ninguna atestación de $_garch" >&2
		rm -rf "$_gtrabajo"
		return 1
	fi
	# gh con una casa vacía: ni sesión, ni configuración, ni avisos.
	if ! mkdir "$_gtrabajo/casa" "$_gtrabajo/paquete"; then
		rm -rf "$_gtrabajo"
		return 1
	fi
	_grc=0
	HOME=$_gtrabajo/casa GH_CONFIG_DIR=$_gtrabajo/casa/gh XDG_CACHE_HOME=$_gtrabajo/casa/cache \
		GH_TOKEN='' GITHUB_TOKEN='' GH_NO_UPDATE_NOTIFIER=1 GH_PROMPT_DISABLED=1 NO_COLOR=1 \
		timeout 300 gh attestation verify "$_gtrabajo/$_garch" --bundle "$_gtrabajo/atestaciones.jsonl" \
		--repo "$_grepo" --source-ref "refs/tags/v$_gver" \
		--signer-workflow "$_grepo/.github/workflows/release.yml" >"$_gtrabajo/gh.txt" 2>&1 || _grc=$?
	if [ "$_grc" -ne 0 ]; then
		# gh sale con 1 tanto si la atestación no cuadra como si no puede
		# comprobarla; solo se distingue por lo que dice. «error creating
		# Sigstore verifier» es que no ha podido preparar la comprobación (sin
		# red no trae la raíz TUF de Sigstore): no se usa, como sin red. Un
		# timeout (124) o gh que no arranca (125 o más), igual. Cualquier otra
		# cosa se da por rechazo: es lo seguro.
		_gmotivo=$(motivo_corto "$_gtrabajo/gh.txt")
		if [ "$_grc" -eq 124 ]; then
			_gmotivo="gh no termina en 5 minutos"
		elif [ "$_grc" -lt 125 ] && ! grep -Eq '^(Error: )?error creating Sigstore verifier' "$_gtrabajo/gh.txt"; then
			echo "la atestación de $_garch no es de $_grepo, refs/tags/v$_gver y release.yml ($_gmotivo)" >&2
			rm -rf "$_gtrabajo"
			return 2
		fi
		echo "no se puede comprobar la atestación de $_garch (¿sin red?): ${_gmotivo:-gh sale con $_grc}" >&2
		rm -rf "$_gtrabajo"
		return 1
	fi
	# El paquete, con su línea de SHA256SUMS, a una carpeta solo suya.
	if ! mv "$_gtrabajo/$_garch" "$_gtrabajo/paquete/$_garch" ||
		! printf '%s  %s\n' "$_gesperada" "$_garch" >"$_gtrabajo/paquete/SHA256SUMS" ||
		! comprobar_paquete "$_gtrabajo/paquete" "$_gver" "" "$_gdst" github; then
		rm -rf "$_gtrabajo"
		return 2
	fi
	rm -rf "$_gtrabajo"
}
