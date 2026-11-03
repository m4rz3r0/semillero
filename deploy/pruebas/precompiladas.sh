#!/bin/sh
# Pruebas de las versiones precompiladas para FreeBSD (deploy/lib/ y
# deploy/cross/), sin servidor, sin PC, sin Forgejo y sin tocar el sistema:
# todo bajo una carpeta temporal. La parte del actualizador está en
# deploy/pruebas/actualizar.sh.
#
# 1. Nombres: version_valida, archivo_precompilada, version_cargo.
# 2. ultima_version (PC) elige la misma etiqueta que ultima_etiqueta_ls_remote
#    (deploy/lib/git.sh, servidor) con la salida de git ls-remote de un
#    repositorio de verdad, y solo vX.Y.Z; el commit es el de la etiqueta
#    (el de la línea «^{}» si es anotada, el suyo si es ligera).
#    repo_por_ssh distingue las URL de ssh de https, file y rutas locales.
# 4. preparar_forgejo (con un curl de mentira que hace de Forgejo) acepta un
#    paquete bueno (deja solo semillero, site/ y COMMIT), con el token solo
#    por la entrada estándar de curl, y rechaza (2), sin dejar nada: sha256
#    que no cuadra, SHA256SUMS de dos líneas, otro commit, un binario de
#    Linux, carpeta con otro nombre o algo más al lado, un enlace simbólico
#    o duro dentro, sin site/ y sin COMMIT. No la usa (1) sin release (404),
#    sin SHA256SUMS, con un token vacío, que Forgejo no acepta (401) o con
#    un paquete más grande que el máximo. url_forgejo_valida y
#    repo_forgejo_valido.
# 4b. preparar_github (con fetch y gh de mentira y el jq de verdad) acepta
#    la versión de una release buena (deja solo semillero, site/ y COMMIT,
#    aunque COMMIT sea otro y traiga LICENSE, README y rc.d/) y llama a gh
#    con el repositorio, la etiqueta y el flujo; rechaza (2) un sha256 que no
#    cuadra, SHA256SUMS sin la línea del paquete, una atestación de otra
#    etiqueta o de otro repositorio, un binario de Linux y la web incompleta;
#    no la usa (1) sin release, sin atestación o si gh no puede comprobarla
#    (sin red para la raíz TUF de Sigstore, colgado), y dice por qué (también
#    lo que contesta fetch); no descarga más del máximo.
#    repo_github_valido solo acepta propietario/repositorio.
# 6. compilar-freebsd.sh se niega antes de compilar nada con una rama
#    (aunque se llame como una versión), con otra versión en Cargo.toml y
#    con --entregar o --ultima, que ya no existen.
# 7. compilar-freebsd.sh de verdad con cargo, cross, rustup y podman de
#    mentira: el paquete (una carpeta con semillero, site/, COMMIT y
#    LICENSE; hash.txt dentro de site/), SHA256SUMS, que dos compilaciones
#    del mismo commit dan el mismo archivo y que preparar_forgejo lo acepta.
# 8. publicar-forgejo.sh (como el flujo .forgejo/workflows/freebsd.yml) con
#    el Forgejo de mentira: compila la etiqueta trayéndola de
#    FORGEJO_SERVER_URL/FORGEJO_REPOSITORY.git, crea la release que falta
#    (con el commit en su texto), sube el paquete y SHA256SUMS el último;
#    al repetirlo, usa la release que hay y reemplaza los ficheros que ya
#    tiene. El token nunca va en los argumentos de curl, y ni el token ni la
#    cabecera de git llegan a la compilación. Se niega sin token o con algo
#    que no es una etiqueta vX.Y.Z.
#
# Uso: deploy/pruebas/precompiladas.sh (desde cualquier sitio; necesita bash,
# git, tar, gzip, file, sha256sum, rsync, flock y jq).

set -eu

DEPLOY=$(cd "$(dirname "$0")/.." && pwd)
LIB=$DEPLOY/lib/precompiladas.sh
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
trap 'exit 143' INT TERM

HOME=$TMP/casa
mkdir -p "$HOME"
GIT_CONFIG_NOSYSTEM=1
GIT_AUTHOR_NAME=prueba
GIT_AUTHOR_EMAIL=prueba
GIT_COMMITTER_NAME=prueba
GIT_COMMITTER_EMAIL=prueba
export HOME GIT_CONFIG_NOSYSTEM GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL

for orden in bash git tar gzip file sha256sum rsync flock jq; do
	command -v "$orden" >/dev/null 2>&1 || {
		echo "FALLO: falta $orden" >&2
		exit 1
	}
done

fallos=0
fallo()
{
	echo "FALLO: $*" >&2
	fallos=$((fallos + 1))
}

# shellcheck source=../lib/precompiladas.sh
. "$LIB"
# shellcheck source=../lib/git.sh
. "$DEPLOY/lib/git.sh"
OBJ=x86_64-unknown-freebsd

# Cabecera ELF mínima de x86-64 con el ABI $1 (9: FreeBSD, 0: Linux/SYSV);
# file la reconoce igual que un binario de verdad.
elf()
{
	printf '\177ELF\002\001\001'
	printf "\\$(printf '%03o' "$1")"
	printf '\000\000\000\000\000\000\000\000\002\000\076\000\001\000\000\000'
	head -c 40 /dev/zero
}
elf 9 >"$TMP/elf-freebsd"

# Construye en $1 (los ficheros de la release de la versión $2) un paquete con
# COMMIT $3. Variantes ($4): buena, linux, otro-nombre, extra, enlace, duro,
# sin-site, sin-commit, sha-malo, dos-lineas.
hacer_paquete()
{
	_dir=$1
	_ver=$2
	_commit=$3
	_variante=${4:-buena}
	_arch=$(archivo_precompilada "$_ver")
	_raiz=${_arch%.tar.gz}
	_t=$TMP/hacer
	rm -rf "$_t"
	if [ "$_variante" = otro-nombre ]; then _raiz=semillero-otra; fi
	mkdir -p "$_t/$_raiz/site/pkg" "$_dir"
	if [ "$_variante" = linux ]; then
		elf 0 >"$_t/$_raiz/semillero"
	else
		elf 9 >"$_t/$_raiz/semillero"
	fi
	echo 'js: abc' >"$_t/$_raiz/site/hash.txt"
	echo js >"$_t/$_raiz/site/pkg/semillero.abc.js"
	echo "$_commit" >"$_t/$_raiz/COMMIT"
	echo licencia >"$_t/$_raiz/LICENSE"
	case $_variante in
	extra) echo x >"$_t/suelto" ;;
	enlace) ln -s /etc/passwd "$_t/$_raiz/site/pkg/enlace" ;;
	duro) ln "$_t/$_raiz/LICENSE" "$_t/$_raiz/site/duro" ;;
	sin-site) rm -rf "$_t/$_raiz/site" ;;
	sin-commit) rm -f "$_t/$_raiz/COMMIT" ;;
	esac
	(cd "$_t" && tar -czf "$_dir/$_arch" -- *)
	(cd "$_dir" && sha256sum -- "$_arch" >SHA256SUMS)
	case $_variante in
	sha-malo) echo basura >>"$_dir/$_arch" ;;
	dos-lineas) echo "$(printf '%064d' 0)  otro.tar.gz" >>"$_dir/SHA256SUMS" ;;
	esac
}

# --- 1. nombres ---------------------------------------------------------------
[ "$(archivo_precompilada 1.2.3)" = semillero-1.2.3-x86_64-unknown-freebsd.tar.gz ] ||
	fallo "archivo_precompilada 1.2.3: $(archivo_precompilada 1.2.3)"
for v in 1.2.3 0.10.0 10.20.30; do
	version_valida "$v" || fallo "version_valida rechaza $v"
done
largo=1.1.$(printf '%040d' 1)
for v in '' v1.2.3 1.2 1.2.3.40000 1.2.3.x 1..3 .1.2 1.2. '1.2.3 ' prueba-abc 1.2.3-rc1 ../1.2 "$largo" '1.2
3'; do
	if version_valida "$v"; then fallo "version_valida acepta «$v»"; fi
done
[ "$(printf '[workspace]\nversion = "9"\n[package]\nname = "x"\nversion = "1.2.3"\n[dependencies]\nversion = "8"\n' | version_cargo)" = 1.2.3 ] ||
	fallo "version_cargo no lee la versión de [package]"

# --- 2. la misma última versión que el servidor ---------------------------------
git init -q -b main "$TMP/versiones"
echo uno >"$TMP/versiones/f"
git -C "$TMP/versiones" add f
git -C "$TMP/versiones" commit -q -m uno
for t in v0.9.9 v0.9.10 v0.10.0 v1.0.0-rc1 v2-prueba otra; do
	git -C "$TMP/versiones" tag "$t"
done
echo dos >"$TMP/versiones/f"
git -C "$TMP/versiones" commit -q -am dos
# La última, anotada: ls-remote da el objeto etiqueta y, con «^{}», el commit.
git -C "$TMP/versiones" tag -a -m v1.0.0 v1.0.0
git -C "$TMP/versiones" ls-remote --tags "$TMP/versiones" >"$TMP/ls-remote"
remota=$(ultima_etiqueta_ls_remote <"$TMP/ls-remote")
pc=$(git -C "$TMP/versiones" tag -l | ultima_version)
[ "$pc" = v1.0.0 ] || fallo "ultima_version da $pc, no v1.0.0"
[ "$remota" = "v1.0.0 $(git -C "$TMP/versiones" rev-parse 'v1.0.0^{commit}')" ] ||
	fallo "ultima_etiqueta_ls_remote con una etiqueta anotada da «$remota»"
grep -q 'refs/tags/v1.0.0^{}' "$TMP/ls-remote" || fallo "la prueba no tiene la línea «^{}» de la etiqueta anotada"
# Ligera: sin «^{}», el commit es el de su línea.
git -C "$TMP/versiones" tag v1.0.1
remota=$(git -C "$TMP/versiones" ls-remote --tags "$TMP/versiones" | ultima_etiqueta_ls_remote)
[ "$remota" = "v1.0.1 $(git -C "$TMP/versiones" rev-parse HEAD)" ] ||
	fallo "ultima_etiqueta_ls_remote con una etiqueta ligera da «$remota»"
# Una etiqueta con otros caracteres donde van los puntos no se confunde.
C1=$(printf '%040d' 0 | tr 0 a)
C2=$(printf '%040d' 0 | tr 0 b)
C3=$(printf '%040d' 0 | tr 0 c)
remota=$(printf '%s\trefs/tags/v3a0a0^{}\n%s\trefs/tags/v3.0.0\n%s\trefs/tags/v3a0a0\n' "$C1" "$C2" "$C3" |
	ultima_etiqueta_ls_remote)
[ "$remota" = "v3.0.0 $C2" ] || fallo "ultima_etiqueta_ls_remote confunde v3.0.0 con otra etiqueta: «$remota»"
[ -z "$(printf '%s\trefs/tags/otra\nbasura\n' "$C1" | ultima_etiqueta_ls_remote)" ] ||
	fallo "ultima_etiqueta_ls_remote da algo sin etiquetas vX.Y.Z"
for u in ssh://git@git.example:2200/u/semillero.git git+ssh://git.example/u/s.git \
	git@git.example:u/semillero.git git.example:semillero.git; do
	repo_por_ssh "$u" || fallo "repo_por_ssh no ve ssh en $u"
done
for u in https://git.example/u/semillero.git file:///srv/semillero.git /srv/semillero.git \
	./semillero ../semillero semillero carpeta/con:dos-puntos :semillero; do
	if repo_por_ssh "$u"; then fallo "repo_por_ssh ve ssh en $u"; fi
done
[ "$(printf 'v0.9.9\nv0.9.10\n' | ultima_version)" = v0.9.10 ] || fallo "ultima_version no ordena v0.9.10 después de v0.9.9"
[ -z "$(printf 'otra\nv1\n' | ultima_version)" ] || fallo "ultima_version da algo sin etiquetas vX.Y.Z"

# --- el Forgejo de mentira ------------------------------------------------------
# curl de mentira: https://forja.example con el repositorio ejemplo/semillero.
# Cada release vive en $FJS/<etiqueta>/ (id: su número; a/: sus ficheros;
# i/: el número de cada fichero; cuerpo.json: lo que se mandó al crearla).
# Responde a la API (GET releases/tags/<etiqueta>, POST releases, POST
# releases/<id>/assets?name=…, DELETE releases/<id>/assets/<id>) y a las
# descargas (releases/download/<etiqueta>/<fichero>). Subir un fichero que
# ya está da 409, como un nombre repetido. Apunta los argumentos en
# $FJS/curl.log y lo que le llega por «-K -» en $FJS/config.log; sin la
# cabecera con el token de $FJS/token-esperado, 401.
FJS=$TMP/forjas
FF=$TMP/falsos-forgejo
mkdir -p "$FJS" "$FF"
echo 0 >"$FJS/contador"
{
	printf '#!/bin/sh\nFJS=%s\n' "$FJS"
	cat <<'EOF'
printf '%s\n' "$*" >>"$FJS/curl.log"
cfg= salida= formato= url= falla= metodo=GET datos= adjunto=
while [ "$#" -gt 0 ]; do
	case $1 in
	-K)
		if [ "$2" = - ]; then cfg=$(cat); fi
		shift 2
		;;
	-o) salida=$2; shift 2 ;;
	-w) formato=$2; shift 2 ;;
	-X) metodo=$2; shift 2 ;;
	--data-binary) datos=${2#@}; shift 2 ;;
	-F) adjunto=${2#attachment=@}; shift 2 ;;
	--fail | --fail-with-body | -f) falla=1; shift ;;
	--connect-timeout | --max-time | --max-filesize | --proto | -H) shift 2 ;;
	-*) shift ;;
	*) url=$1; shift ;;
	esac
done
if [ -n "$adjunto" ] && [ "$metodo" = GET ]; then metodo=POST; fi
if [ -n "$datos" ] && [ "$metodo" = GET ]; then metodo=POST; fi
printf '%s\n' "$cfg" >>"$FJS/config.log"
cuerpo=$FJS/.cuerpo
printf '{"message":"no"}\n' >"$cuerpo"
# responder CÓDIGO: con el cuerpo de $cuerpo.
responder()
{
	if [ -z "$salida" ] || [ "$salida" = - ]; then cat "$cuerpo"; else cat "$cuerpo" >"$salida"; fi
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
json_release()
{
	printf '{"id":%s,"tag_name":"%s","assets":[' "$(cat "$FJS/$1/id")" "$1"
	_c=
	for _f in "$FJS/$1/a"/*; do
		[ -f "$_f" ] || continue
		printf '%s{"id":%s,"name":"%s"}' "$_c" "$(cat "$FJS/$1/i/${_f##*/}")" "${_f##*/}"
		_c=,
	done
	printf ']}\n'
}
etiqueta_de_id()
{
	for _d in "$FJS"/*/id; do
		[ -f "$_d" ] || continue
		if [ "$(cat "$_d")" = "$1" ]; then
			_e=${_d%/id}
			echo "${_e##*/}"
			return 0
		fi
	done
	return 1
}
nuevo_id()
{
	_n=$(($(cat "$FJS/contador") + 1))
	echo "$_n" >"$FJS/contador"
	echo "$_n"
}
if [ "$cfg" != "header = \"Authorization: token $(cat "$FJS/token-esperado")\"" ]; then
	responder 401
fi
API=https://forja.example/api/v1/repos/ejemplo/semillero
case $metodo:$url in
GET:$API/releases/tags/*)
	e=${url##*/tags/}
	[ -f "$FJS/$e/id" ] || responder 404
	json_release "$e" >"$cuerpo"
	responder 200
	;;
POST:$API/releases)
	e=$(jq -r .tag_name "$datos")
	[ ! -e "$FJS/$e/id" ] || responder 409
	mkdir -p "$FJS/$e/a" "$FJS/$e/i"
	cp "$datos" "$FJS/$e/cuerpo.json"
	nuevo_id >"$FJS/$e/id"
	json_release "$e" >"$cuerpo"
	responder 201
	;;
POST:$API/releases/*/assets\?name=*)
	r=${url#"$API"/releases/}
	r=${r%%/*}
	n=${url##*name=}
	e=$(etiqueta_de_id "$r") || responder 404
	[ ! -e "$FJS/$e/a/$n" ] || responder 409
	cp "$adjunto" "$FJS/$e/a/$n"
	nuevo_id >"$FJS/$e/i/$n"
	printf 'subido %s %s\n' "$e" "$n" >>"$FJS/subidas.log"
	printf '{"id":%s,"name":"%s"}\n' "$(cat "$FJS/$e/i/$n")" "$n" >"$cuerpo"
	responder 201
	;;
DELETE:$API/releases/*/assets/*)
	r=${url#"$API"/releases/}
	r=${r%%/*}
	a=${url##*/}
	e=$(etiqueta_de_id "$r") || responder 404
	for f in "$FJS/$e/i"/*; do
		if [ -f "$f" ] && [ "$(cat "$f")" = "$a" ]; then
			n=${f##*/}
			rm -f -- "${FJS:?}/${e:?}/i/${n:?}" "${FJS:?}/${e:?}/a/${n:?}"
			printf 'borrado %s %s\n' "$e" "$n" >>"$FJS/subidas.log"
			: >"$cuerpo"
			responder 204
		fi
	done
	responder 404
	;;
GET:https://forja.example/ejemplo/semillero/releases/download/*/*)
	n=${url##*/}
	r=${url%/*}
	e=${r##*/}
	[ -f "$FJS/$e/a/$n" ] || responder 404
	cp "$FJS/$e/a/$n" "$cuerpo"
	responder 200
	;;
esac
echo "curl: (6) Could not resolve host" >&2
exit 6
EOF
} >"$FF/curl"
chmod 755 "$FF/curl"
FU=https://forja.example
FR=ejemplo/semillero
TOK=$TMP/token-forgejo
printf 'token-de-prueba-lectura\n' >"$TOK"
printf 'token-de-prueba-lectura' >"$FJS/token-esperado"
# Una release de Forgejo vacía para la etiqueta $1; escribe la carpeta de
# sus ficheros.
release_forgejo()
{
	rm -rf -- "${FJS:?}/${1:?}"
	mkdir -p "$FJS/$1/a" "$FJS/$1/i"
	echo 99 >"$FJS/$1/id"
	echo "$FJS/$1/a"
}

# --- 4. la versión de una release de Forgejo -------------------------------------
# Commits de 40 cifras hex, generados (sin literales largos en el repositorio).
C=$(printf "%040d" 0 | tr 0 a)
OTRO=$(printf "%040d" 0 | tr 0 b)
V=1.2.3

for u in https://forja.example http://192.0.2.1:3000 https://forja.example/git 'https://[2001:db8::1]:3000'; do
	url_forgejo_valida "$u" || fallo "url_forgejo_valida rechaza $u"
done
for u in '' forja.example https:// https://forja.example/ 'https://forja.example/a b' 'https://x;id' \
	'https://x"y' https://x/../y ftp://forja.example file:///srv; do
	if url_forgejo_valida "$u"; then fallo "url_forgejo_valida acepta «$u»"; fi
done
for r in ejemplo/semillero a.b/c_d-e Ejemplo-1/s; do
	repo_forgejo_valido "$r" || fallo "repo_forgejo_valido rechaza $r"
done
for r in '' ejemplo ejemplo/ /semillero ejemplo/semillero/x -ejemplo/s ejemplo/-s .ejemplo/s ejemplo/.. \
	'ejemplo/semi llero' 'ejemplo/semillero;id' 'ejemplo/$(id)'; do
	if repo_forgejo_valido "$r"; then fallo "repo_forgejo_valido acepta «$r»"; fi
done

# forjar COMMIT DESTINO: preparar_forgejo de la versión $V con el curl de
# mentira; sale con lo que salga.
forjar()
{
	PATH=$FF:$PATH preparar_forgejo "$FU" "$FR" "$TOK" "$V" "$1" "$2"
}
hacer_paquete "$(release_forgejo "v$V")" "$V" "$C"
: >"$FJS/curl.log"
: >"$FJS/config.log"
if forjar "$C" "$TMP/bueno" 2>"$TMP/err"; then
	[ "$(find "$TMP/bueno" -mindepth 1 -maxdepth 1 -printf '%f\n' | sort | tr '\n' ' ')" = "COMMIT semillero site " ] ||
		fallo "el paquete bueno no queda solo con semillero, site/ y COMMIT"
	[ "$(cat "$TMP/bueno/COMMIT")" = "$C" ] || fallo "COMMIT no es el de la etiqueta"
else
	fallo "rechaza el paquete bueno: $(cat "$TMP/err")"
fi
grep -q " $FU/api/v1/repos/$FR/releases/tags/v$V\$" "$FJS/curl.log" || fallo "no pregunta a la API por la release: $(cat "$FJS/curl.log")"
for f in SHA256SUMS "$(archivo_precompilada "$V")"; do
	grep -q " $FU/$FR/releases/download/v$V/$f\$" "$FJS/curl.log" || fallo "no descarga $f: $(cat "$FJS/curl.log")"
done
grep -q token-de-prueba-lectura "$FJS/curl.log" && fallo "el token va en los argumentos de curl"
grep -qxF 'header = "Authorization: token token-de-prueba-lectura"' "$FJS/config.log" ||
	fallo "el token no llega por la entrada estándar de curl: $(sort -u "$FJS/config.log")"
grep -q -- '--proto =http,https' "$FJS/curl.log" || fallo "curl sin --proto"

# rechazar NOMBRE VARIANTE COMMIT_ESPERADO MOTIVO_ESPERADO
rechazar()
{
	rm -rf -- "${TMP:?}/malo"
	hacer_paquete "$(release_forgejo "v$V")" "$V" "$C" "$2"
	_rc=0
	forjar "$3" "$TMP/malo" 2>"$TMP/err" || _rc=$?
	if [ "$_rc" -ne 2 ]; then
		fallo "$1: sale con $_rc, no con 2 ($(cat "$TMP/err"))"
	elif ! grep -q "$4" "$TMP/err"; then
		fallo "$1: el motivo no dice «$4»: $(cat "$TMP/err")"
	fi
	[ ! -e "$TMP/malo" ] || fallo "$1: deja la carpeta de destino"
}
rechazar "sha256 alterado" sha-malo "$C" sha256
rechazar "SHA256SUMS de dos líneas" dos-lineas "$C" "una sola línea"
rechazar "otro commit" buena "$OTRO" "otro commit"
rechazar "commit no válido" buena "abc" "commit no válido"
rechazar "binario de Linux" linux "$C" FreeBSD
rechazar "carpeta con otro nombre" otro-nombre "$C" "fuera de"
rechazar "algo al lado de la carpeta" extra "$C" "fuera de"
rechazar "enlace simbólico dentro" enlace "$C" "normal"
rechazar "enlace duro dentro" duro "$C" "normal"
rechazar "sin site/" sin-site "$C" "faltan"
rechazar "sin COMMIT" sin-commit "$C" "faltan"

# no_usar NOMBRE MOTIVO: preparar_forgejo sale con 1 (aún no se puede usar),
# lo dice y no deja nada.
no_usar()
{
	rm -rf -- "${TMP:?}/malo"
	_rc=0
	forjar "$C" "$TMP/malo" 2>"$TMP/err" || _rc=$?
	if [ "$_rc" -ne 1 ]; then
		fallo "$1: sale con $_rc, no con 1 ($(cat "$TMP/err"))"
	elif ! grep -q "$2" "$TMP/err"; then
		fallo "$1: el motivo no dice «$2»: $(cat "$TMP/err")"
	fi
	[ ! -e "$TMP/malo" ] || fallo "$1: deja la carpeta de destino"
}
rm -rf -- "${FJS:?}/v$V"
no_usar "sin release" "aún no está publicada"
hacer_paquete "$(release_forgejo "v$V")" "$V" "$C"
rm -f -- "${FJS:?}/v$V/a/SHA256SUMS"
no_usar "sin SHA256SUMS" "aún no trae"
hacer_paquete "$(release_forgejo "v$V")" "$V" "$C"
printf 'otro-token' >"$FJS/token-esperado"
no_usar "token que Forgejo no acepta" "HTTP 401"
printf 'token-de-prueba-lectura' >"$FJS/token-esperado"
cp "$TOK" "$TMP/token-aparte"
: >"$TOK"
: >"$FJS/curl.log"
no_usar "token vacío" "token"
[ ! -s "$FJS/curl.log" ] || fallo "llama a curl con el token vacío"
printf 'token "raro"\n' >"$TOK"
no_usar "token con comillas" "token"
[ ! -s "$FJS/curl.log" ] || fallo "llama a curl con un token con comillas"
cp "$TMP/token-aparte" "$TOK"
rm -rf -- "${TMP:?}/malo"
if PATH=$FF:$PATH FORGEJO_MAX_PAQUETE=100 preparar_forgejo "$FU" "$FR" "$TOK" "$V" "$C" "$TMP/malo" 2>"$TMP/err"; then
	fallo "Forgejo: acepta un paquete más grande que el máximo"
fi
[ ! -e "$TMP/malo" ] || fallo "Forgejo: deja la carpeta con un paquete demasiado grande"
rm -rf -- "${FJS:?}/v$V" "${TMP:?}/bueno"

# --- 4b. la versión publicada en GitHub ------------------------------------------
# fetch y gh de mentira: fetch sirve lo de $W/web (github.com) y $W/api
# (api.github.com); gh da por buena una atestación {"falsa":"<sha256>
# <repo> <ref> <flujo>"} que case con el paquete y con lo que se le pide. jq
# es el de verdad.
W=$TMP/github
FG=$TMP/falsos-github
mkdir -p "$W" "$FG"
cat >"$FG/fetch" <<EOF
#!/bin/sh
for u; do :; done
printf '%s\n' "\$u" >>"$W/fetch.log"
case \$u in
https://github.com/*) f="$W/web/\${u#https://github.com/}" ;;
https://api.github.com/*) f="$W/api/\${u#https://api.github.com/}" ;;
*) exit 1 ;;
esac
[ -f "\$f" ] || { echo "fetch: \$u: Not Found" >&2; exit 1; }
cat "\$f"
EOF
cat >"$FG/gh" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >>"$W/gh.log"
[ "\$1 \$2" = "attestation verify" ] || exit 2
# gh-sin-red: no puede traer la raíz TUF de Sigstore (sin red); gh-colgado:
# timeout lo corta.
[ ! -e "$W/gh-sin-red" ] || { echo "error creating Sigstore verifier: no valid Sigstore verifiers could be initialized" >&2; exit 1; }
[ ! -e "$W/gh-colgado" ] || exit 124
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
chmod 755 "$FG"/*
RG=ejemplo/semillero

# Publica en el GitHub de mentira la versión $1 del repositorio $RG como la
# publica release.yml: el paquete con LICENSE, README.md, README.en.md y
# rc.d/, un COMMIT del repositorio público, SHA256SUMS con más paquetes y
# su atestación. Variantes ($2): buena, sha-malo, sin-linea, otra-etiqueta,
# otro-repo, sin-atestacion, linux, sin-web.
publicar_github()
{
	_ver=$1
	_variante=${2:-buena}
	_arch=$(archivo_precompilada "$_ver")
	_raiz=${_arch%.tar.gz}
	_rel=$W/web/$RG/releases/download/v$_ver
	_t=$TMP/hacer-github
	rm -rf "$_t" "$_rel" "$W/api"
	mkdir -p "$_t/$_raiz/site/pkg" "$_t/$_raiz/rc.d" "$_rel" "$W/api/repos/$RG/attestations"
	if [ "$_variante" = linux ]; then
		elf 0 >"$_t/$_raiz/semillero"
	else
		elf 9 >"$_t/$_raiz/semillero"
	fi
	printf 'js: abc\nwasm: abc\ncss: abc\n' >"$_t/$_raiz/site/hash.txt"
	for _e in js wasm css; do echo "$_e" >"$_t/$_raiz/site/pkg/semillero.abc.$_e"; done
	if [ "$_variante" = sin-web ]; then rm -f "$_t/$_raiz/site/pkg/semillero.abc.wasm"; fi
	printf '%040d\n' 0 | tr 0 c >"$_t/$_raiz/COMMIT"
	for _f in LICENSE README.md README.en.md rc.d/semillero; do echo "$_f" >"$_t/$_raiz/$_f"; done
	(cd "$_t" && tar -czf "$_rel/$_arch" -- "$_raiz")
	_d=$(sha256sum <"$_rel/$_arch" | cut -c1-64)
	{
		echo "$(printf '%064d' 1)  semillero-$_ver-x86_64-unknown-linux-gnu.tar.gz"
		case $_variante in
		sha-malo) echo "$(printf '%064d' 2)  $_arch" ;;
		sin-linea) ;;
		*) echo "$_d  $_arch" ;;
		esac
		echo "$(printf '%064d' 3)  semillero-$_ver-x86_64-pc-windows-msvc.zip"
	} >"$_rel/SHA256SUMS"
	_ref=refs/tags/v$_ver
	_r=$RG
	case $_variante in
	otra-etiqueta) _ref=refs/tags/v9.9.9 ;;
	otro-repo) _r=otro/semillero ;;
	esac
	if [ "$_variante" != sin-atestacion ]; then
		printf '{"attestations":[{"bundle":{"falsa":"%s %s %s %s/.github/workflows/release.yml"},"repository_id":1}]}\n' \
			"$_d" "$_r" "$_ref" "$_r" >"$W/api/repos/$RG/attestations/sha256:$_d"
	fi
}

for r in ejemplo/semillero Ejemplo-1/semi.llero_2 a/b; do
	repo_github_valido "$r" || fallo "repo_github_valido rechaza $r"
done
for r in '' ejemplo ejemplo/ /semillero ejemplo/semillero/x -ejemplo/semillero ejemplo/. ejemplo/.. \
	'ejemplo/semi llero' 'ejemplo/semillero;id' 'ej_emplo/semillero' 'ejemplo/$(id)' \
	"$(printf '%040d' 0 | tr 0 a)/semillero" 'ejemplo/semillero
x'; do
	if repo_github_valido "$r"; then fallo "repo_github_valido acepta «$r»"; fi
done

V=2.0.0
publicar_github "$V"
: >"$W/gh.log"
if PATH=$FG:$PATH preparar_github "$RG" "$V" "$TMP/gh-bueno" 2>"$TMP/err"; then
	[ "$(find "$TMP/gh-bueno" -mindepth 1 -maxdepth 1 -printf '%f\n' | sort | tr '\n' ' ')" = "COMMIT semillero site " ] ||
		fallo "GitHub: no queda solo con semillero, site/ y COMMIT"
else
	fallo "GitHub: rechaza la versión buena: $(cat "$TMP/err")"
fi
case $(cat "$W/gh.log") in
"attestation verify "*" --bundle "*" --repo $RG --source-ref refs/tags/v$V --signer-workflow $RG/.github/workflows/release.yml") ;;
*) fallo "GitHub: gh no se llama como se espera: $(cat "$W/gh.log")" ;;
esac
grep -qxF "https://github.com/$RG/releases/download/v$V/$(archivo_precompilada "$V")" "$W/fetch.log" ||
	fallo "GitHub: no descarga el paquete de la release: $(cat "$W/fetch.log")"
grep -q "^https://api.github.com/repos/$RG/attestations/sha256:[0-9a-f]\{64\}\$" "$W/fetch.log" ||
	fallo "GitHub: no trae la atestación de la API: $(cat "$W/fetch.log")"

# rechazar_github VARIANTE CÓDIGO MOTIVO: preparar_github sale con CÓDIGO (1:
# no está, 2: no vale), lo dice y no deja nada.
rechazar_github()
{
	rm -rf "$TMP/gh-malo"
	_rc=0
	PATH=$FG:$PATH preparar_github "$RG" "$V" "$TMP/gh-malo" 2>"$TMP/err" || _rc=$?
	if [ "$_rc" -ne "$2" ]; then
		fallo "GitHub $1: sale con $_rc, no con $2 ($(cat "$TMP/err"))"
	elif ! grep -q "$3" "$TMP/err"; then
		fallo "GitHub $1: el motivo no dice «$3»: $(cat "$TMP/err")"
	fi
	[ ! -e "$TMP/gh-malo" ] || fallo "GitHub $1: deja la carpeta de destino"
}
for v in sha-malo sin-linea otra-etiqueta otro-repo sin-atestacion linux sin-web; do
	publicar_github "$V" "$v"
	case $v in
	sha-malo) rechazar_github "$v" 2 "no coincide con SHA256SUMS" ;;
	sin-linea) rechazar_github "$v" 2 "SHA256SUMS de GitHub" ;;
	otra-etiqueta | otro-repo) rechazar_github "$v" 2 "atestación" ;;
	sin-atestacion) rechazar_github "$v" 1 "atestaci" ;;
	linux) rechazar_github "$v" 2 FreeBSD ;;
	sin-web) rechazar_github "$v" 2 "falta site/pkg" ;;
	esac
done
rm -rf "$W/web/$RG/releases/download/v$V"
rechazar_github "sin release" 1 "no se puede descargar"
# El motivo de fetch va en la misma línea.
grep -q "no se puede descargar.*Not Found" "$TMP/err" || fallo "GitHub sin release: no dice qué contesta fetch: $(cat "$TMP/err")"
publicar_github "$V"
# Que gh no pueda hacer la comprobación (sin red para la raíz TUF de
# Sigstore, o colgado) no es un rechazo: no se usa (1) y dice por qué.
touch "$W/gh-sin-red"
rechazar_github "gh sin red" 1 "no se puede comprobar la atestación.*Sigstore"
rm -f "$W/gh-sin-red"
touch "$W/gh-colgado"
rechazar_github "gh colgado" 1 "no se puede comprobar la atestación.*no termina"
rm -f "$W/gh-colgado"
rechazar_github_repo()
{
	if PATH=$FG:$PATH preparar_github "$1" "$V" "$TMP/gh-malo" 2>"$TMP/err"; then
		fallo "GitHub: acepta el repositorio «$1»"
	fi
	[ ! -e "$TMP/gh-malo" ] || fallo "GitHub: deja la carpeta con el repositorio «$1»"
}
rechazar_github_repo 'ejemplo/semillero/../x'
rechazar_github_repo '-ejemplo/semillero'
# Un paquete más grande que el máximo no se descarga entero.
publicar_github "$V"
if PATH=$FG:$PATH GITHUB_MAX_PAQUETE=100 preparar_github "$RG" "$V" "$TMP/gh-malo" 2>"$TMP/err"; then
	fallo "GitHub: acepta un paquete más grande que el máximo"
fi
grep -q "no se puede descargar.*más de 100 bytes" "$TMP/err" || fallo "GitHub: no dice que el paquete es demasiado grande: $(cat "$TMP/err")"
rm -rf "$TMP/gh-bueno" "$TMP/gh-malo"
V=1.2.3

# --- 6. compilar-freebsd.sh se niega antes de compilar -------------------------
L=$TMP/fuente
mkdir -p "$L/crates/semillero"
cp -R "$DEPLOY" "$L/deploy"
echo licencia >"$L/LICENSE"
printf '[package]\nname = "semillero"\nversion = "1.0.0"\n' >"$L/crates/semillero/Cargo.toml"
printf '[[package]]\nname = "wasm-bindgen"\nversion = "0.2.100"\n' >"$L/Cargo.lock"
git init -q -b main "$L"
git -C "$L" add -A
git -C "$L" commit -q -m v1.0.0
git -C "$L" tag -a -m v1.0.0 v1.0.0
git -C "$L" tag v1.1.0
git -C "$L" branch v2.0.0

# compilar-freebsd.sh con cargo, cross, rustup y podman de mentira. En las
# negativas no llega a compilar: se niega antes. El cargo de mentira apunta
# también si a la compilación le llegan el token de Forgejo o la cabecera
# de git (no deben).
F=$TMP/falsos
mkdir -p "$F"
cat >"$F/cargo" <<EOF
#!/bin/sh
echo "cargo \$*" >>"$TMP/cargo.log"
case "\$*" in
"leptos --version") echo "cargo-leptos 0.3.11" ;;
"leptos build --release --precompress")
	echo "entorno \${FORGEJO_TOKEN:-sin-token} \${GIT_CONFIG_VALUE_0:-sin-cabecera}" >>"$TMP/cargo.log"
	[ "\$LEPTOS_BIN_TARGET_TRIPLE" = $OBJ ] && [ "\$LEPTOS_BIN_CARGO_COMMAND" = cross ] || exit 1
	mkdir -p target/$OBJ/release target/$OBJ/site target/site/pkg
	cat "$TMP/elf-freebsd" >target/$OBJ/release/semillero
	printf 'js: h1\nwasm: h2\ncss: h3\n' >target/$OBJ/site/hash.txt
	echo js >target/site/pkg/semillero.h1.js
	echo wasm >target/site/pkg/semillero.h2.wasm
	echo css >target/site/pkg/semillero.h3.css
	: >target/site/pkg/.gitkeep
	;;
*) exit 1 ;;
esac
EOF
printf '#!/bin/sh\nexit 0\n' >"$F/rustup"
printf '#!/bin/sh\nexit 0\n' >"$F/cross"
printf '#!/bin/sh\nexit 0\n' >"$F/podman"
chmod 755 "$F"/*
git -C "$L" bundle create -q "$TMP/l.bundle" --all 2>/dev/null
compilar()
{
	SEMILLERO_CROSS_DIR=$TMP/semillero-cross PATH="$F:$PATH" \
		bash "$DEPLOY/cross/compilar-freebsd.sh" "$@" >"$TMP/salida" 2>&1
}
if compilar --release v2.0.0 --bundle "$TMP/l.bundle"; then
	fallo "compilar-freebsd.sh --release con una rama: no se niega"
elif ! grep -q "no es una etiqueta" "$TMP/salida"; then
	fallo "compilar-freebsd.sh --release con una rama: $(cat "$TMP/salida")"
fi
if compilar --release main --bundle "$TMP/l.bundle"; then
	fallo "compilar-freebsd.sh --release main: no se niega"
fi
if compilar --release v1.1.0 --bundle "$TMP/l.bundle" || ! grep -q "no coincide" "$TMP/salida"; then
	fallo "compilar-freebsd.sh --release con otra versión en Cargo.toml: $(cat "$TMP/salida")"
fi
for a in "--release v1.0.0 --bundle $TMP/l.bundle --entregar destino" "--ultima --repo $L"; do
	st=0
	# shellcheck disable=SC2086 # se parte a propósito
	compilar $a || st=$?
	[ "$st" -eq 2 ] && grep -q "compilar-freebsd.sh --release" "$TMP/salida" ||
		fallo "compilar-freebsd.sh $a: no da el uso (sale con $st): $(cat "$TMP/salida")"
	grep -qE -- '--entregar|--ultima' "$TMP/salida" && fallo "el uso todavía habla de --entregar o --ultima"
done
[ ! -e "$TMP/cargo.log" ] || fallo "compilar-freebsd.sh compila antes de negarse"

# --- 7. compilar y empaquetar ----------------------------------------------------
COMMIT_L=$(git -C "$L" rev-parse 'v1.0.0^{commit}')
if ! compilar --release v1.0.0 --bundle "$TMP/l.bundle"; then
	fallo "compilar-freebsd.sh --release v1.0.0: $(cat "$TMP/salida")"
fi
S=$TMP/semillero-cross/salida/1.0.0
A=$(archivo_precompilada 1.0.0)
[ "$(tar -tzf "$S/$A" 2>/dev/null | sort | tr '\n' ' ')" = "semillero-1.0.0-$OBJ/ semillero-1.0.0-$OBJ/COMMIT semillero-1.0.0-$OBJ/LICENSE semillero-1.0.0-$OBJ/semillero semillero-1.0.0-$OBJ/site/ semillero-1.0.0-$OBJ/site/hash.txt semillero-1.0.0-$OBJ/site/pkg/ semillero-1.0.0-$OBJ/site/pkg/semillero.h1.js semillero-1.0.0-$OBJ/site/pkg/semillero.h2.wasm semillero-1.0.0-$OBJ/site/pkg/semillero.h3.css " ] ||
	fallo "el paquete no trae lo esperado: $(tar -tzf "$S/$A" 2>&1 | tr '\n' ' ')"
tar -tvzf "$S/$A" 2>/dev/null | grep -q '^-rwxr-xr-x 0/0 .*/semillero$' || fallo "el binario no va 755 y de 0:0"
[ "$(cat "$S/SHA256SUMS" 2>/dev/null)" = "$(cd "$S" && sha256sum "$A")" ] || fallo "SHA256SUMS no es la línea de sha256sum"
[ "$(find "$S" -mindepth 1 -printf '%f\n' | LC_ALL=C sort | tr '\n' ' ')" = "SHA256SUMS $A " ] ||
	fallo "salida/1.0.0 no trae solo el paquete y SHA256SUMS: $(ls -a "$S" | tr '\n' ' ')"
# Lo que compila el PC lo acepta el servidor, servido por la release.
V=1.0.0
cp "$S/$A" "$S/SHA256SUMS" "$(release_forgejo v1.0.0)/"
if forjar "$COMMIT_L" "$TMP/instalada" 2>"$TMP/err"; then
	[ -f "$TMP/instalada/site/hash.txt" ] || fallo "hash.txt no va dentro de site/"
else
	fallo "el actualizador rechaza lo que compila el PC: $(cat "$TMP/err")"
fi
rm -rf -- "${FJS:?}/v1.0.0" "${TMP:?}/instalada"
grep -q 'leptos build --release --precompress' "$TMP/cargo.log" || fallo "no compila con cargo leptos build --release --precompress"
# Reproducible: el mismo commit da el mismo archivo.
cp "$S/$A" "$TMP/primero.tar.gz"
sleep 1
compilar --release v1.0.0 --bundle "$TMP/l.bundle" || fallo "segunda compilación: $(cat "$TMP/salida")"
cmp -s "$TMP/primero.tar.gz" "$S/$A" || fallo "dos compilaciones del mismo commit dan archivos distintos"

# --- 8. publicar-forgejo.sh -----------------------------------------------------
# El repositorio de Forgejo: un clon desnudo de la fuente. git llega a él
# por https://forja.example/ejemplo/semillero.git gracias a un insteadOf de
# la configuración de git de la casa de la prueba.
mkdir -p "$TMP/forja/ejemplo"
git clone -q --bare "$L" "$TMP/forja/ejemplo/semillero.git"
git -C "$HOME" config --file "$HOME/.gitconfig" url."file://$TMP/forja/".insteadOf https://forja.example/
TOKEN_JOB=token-de-prueba-del-trabajo
printf '%s' "$TOKEN_JOB" >"$FJS/token-esperado"
: >"$FJS/curl.log"
: >"$FJS/config.log"
: >"$FJS/subidas.log"
: >"$TMP/cargo.log"
# publicar [VARIABLE=valor…]: publicar-forgejo.sh con el entorno del flujo.
publicar()
{
	env FORGEJO_TOKEN="$TOKEN_JOB" FORGEJO_SERVER_URL=https://forja.example FORGEJO_REPOSITORY=ejemplo/semillero \
		FORGEJO_REF_NAME=v1.0.0 SEMILLERO_CROSS_DIR="$TMP/semillero-cross" PATH="$FF:$F:$PATH" "$@" \
		bash "$DEPLOY/cross/publicar-forgejo.sh" >"$TMP/salida" 2>&1
}
# Negativas: no compila ni llama a Forgejo.
for mala in FORGEJO_TOKEN= FORGEJO_REF_NAME=main FORGEJO_REF_NAME=v1.0 FORGEJO_REPOSITORY=ejemplo \
	'FORGEJO_TOKEN=con espacio' FORGEJO_SERVER_URL=; do
	if publicar "$mala"; then fallo "publicar-forgejo.sh acepta $mala"; fi
	grep -q "${mala%%=*}" "$TMP/salida" || fallo "publicar-forgejo.sh con $mala no dice qué pasa: $(cat "$TMP/salida")"
done
[ ! -s "$TMP/cargo.log" ] && [ ! -s "$FJS/curl.log" ] || fallo "publicar-forgejo.sh compila o llama a Forgejo antes de negarse"
rm -rf -- "${TMP:?}/semillero-cross/repo"
if ! publicar; then
	fallo "publicar-forgejo.sh falla: $(cat "$TMP/salida")"
fi
[ -f "$FJS/v1.0.0/id" ] || fallo "no crea la release de v1.0.0: $(cat "$FJS/curl.log")"
grep -q -- "-X POST .* https://forja.example/api/v1/repos/ejemplo/semillero/releases\$" "$FJS/curl.log" ||
	fallo "no crea la release con POST: $(cat "$FJS/curl.log")"
[ "$(jq -r .tag_name "$FJS/v1.0.0/cuerpo.json" 2>&1)" = v1.0.0 ] || fallo "la release no es de v1.0.0: $(cat "$FJS/v1.0.0/cuerpo.json")"
jq -r .body "$FJS/v1.0.0/cuerpo.json" | grep -q "$COMMIT_L" || fallo "el texto de la release no dice el commit: $(cat "$FJS/v1.0.0/cuerpo.json")"
cmp -s "$S/$A" "$FJS/v1.0.0/a/$A" && cmp -s "$S/SHA256SUMS" "$FJS/v1.0.0/a/SHA256SUMS" ||
	fallo "lo que sube no es lo compilado"
[ "$(tr '\n' ' ' <"$FJS/subidas.log")" = "subido v1.0.0 $A subido v1.0.0 SHA256SUMS " ] ||
	fallo "no sube el paquete primero y SHA256SUMS el último: $(cat "$FJS/subidas.log")"
grep -q 'leptos build' "$TMP/cargo.log" || fallo "publicar-forgejo.sh no compila"
grep -qx 'entorno sin-token sin-cabecera' "$TMP/cargo.log" ||
	fallo "a la compilación le llegan el token o la cabecera de git: $(grep '^entorno' "$TMP/cargo.log")"
[ "$(git -C "$TMP/semillero-cross/repo" rev-parse 'v1.0.0^{commit}' 2>/dev/null)" = "$COMMIT_L" ] ||
	fallo "no trae la etiqueta de FORGEJO_SERVER_URL/FORGEJO_REPOSITORY.git"
grep -q "$TOKEN_JOB" "$FJS/curl.log" && fallo "el token va en los argumentos de curl"
grep -q -- '--fail-with-body' "$FJS/curl.log" && grep -q -- '--max-time' "$FJS/curl.log" ||
	fallo "curl sin --fail-with-body o sin --max-time: $(cat "$FJS/curl.log")"
[ "$(sort -u "$FJS/config.log")" = "header = \"Authorization: token $TOKEN_JOB\"" ] ||
	fallo "no todas las llamadas llevan el token por la entrada estándar: $(sort -u "$FJS/config.log")"
# El servidor acepta lo publicado.
printf 'token-de-prueba-lectura' >"$FJS/token-esperado"
if forjar "$COMMIT_L" "$TMP/instalada" 2>"$TMP/err"; then :; else
	fallo "el actualizador rechaza lo que publica el runner: $(cat "$TMP/err")"
fi
rm -rf -- "${TMP:?}/instalada"
printf '%s' "$TOKEN_JOB" >"$FJS/token-esperado"

# Otra vez la misma etiqueta: la release ya está y trae un SHA256SUMS
# viejo; se reemplaza (sin un 409 por el nombre repetido) y no se crea otra.
echo viejo >"$FJS/v1.0.0/a/SHA256SUMS"
id_antes=$(cat "$FJS/v1.0.0/id")
: >"$FJS/curl.log"
: >"$FJS/subidas.log"
if ! publicar; then
	fallo "publicar-forgejo.sh falla la 2.ª vez: $(cat "$TMP/salida")"
fi
[ "$(cat "$FJS/v1.0.0/id")" = "$id_antes" ] || fallo "crea otra release la 2.ª vez"
grep -q -- '-X POST .*/releases$' "$FJS/curl.log" && fallo "vuelve a crear la release: $(cat "$FJS/curl.log")"
cmp -s "$S/SHA256SUMS" "$FJS/v1.0.0/a/SHA256SUMS" || fallo "no reemplaza el SHA256SUMS que había"
grep -q '^borrado v1.0.0 SHA256SUMS$' "$FJS/subidas.log" || fallo "no borra el SHA256SUMS que había: $(cat "$FJS/subidas.log")"
[ "$(grep '^subido' "$FJS/subidas.log" | tail -n 1)" = "subido v1.0.0 SHA256SUMS" ] ||
	fallo "la 2.ª vez SHA256SUMS no va el último: $(cat "$FJS/subidas.log")"
grep -q "$TOKEN_JOB" "$FJS/curl.log" && fallo "el token va en los argumentos de curl (2.ª vez)"

# --- sintaxis ------------------------------------------------------------------
for f in "$LIB" "$DEPLOY/lib/git.sh" "$DEPLOY/freebsd/actualizar.sh" "$DEPLOY/freebsd/install.sh"; do
	sh -n "$f" || fallo "sh -n ${f#"$DEPLOY"/}"
done
for f in "$DEPLOY"/cross/*.sh; do
	bash -n "$f" || fallo "bash -n ${f#"$DEPLOY"/}"
done

if [ "$fallos" -ne 0 ]; then
	echo "$fallos fallos" >&2
	exit 1
fi
echo "precompiladas: todo bien"
