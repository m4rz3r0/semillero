#!/usr/bin/env bash
# Compila para FreeBSD la etiqueta vX.Y.Z que ha lanzado el flujo de Forgejo
# Actions (.forgejo/workflows/freebsd.yml) y la publica como la release de
# Forgejo de esa etiqueta, de donde la instala el actualizador del servidor
# (deploy/freebsd/actualizar.sh). Corre en el PC, en el runner de Forgejo
# Actions (etiqueta «pc», ejecutor host), como su usuario, desde la copia de
# la etiqueta que saca el primer paso del flujo (deploy/README.md, «Runner
# de Forgejo Actions en el PC»).
#
#   publicar-forgejo.sh
#
# Entradas, por el entorno (las pone el flujo):
#   FORGEJO_TOKEN        el token del trabajo (forgejo.token): traer la
#                        etiqueta y crear la release y subir sus ficheros.
#                        Solo va en la cabecera Authorization, por la
#                        entrada estándar de curl (-K -) y por el entorno de
#                        git (GIT_CONFIG_*); nunca en los argumentos ni en
#                        una URL.
#   FORGEJO_SERVER_URL   la URL de Forgejo (forgejo.server_url).
#   FORGEJO_REPOSITORY   propietario/nombre (forgejo.repository).
#   FORGEJO_REF_NAME     la etiqueta vX.Y.Z (forgejo.ref_name).
#   SEMILLERO_CROSS_DIR  opcional: la carpeta de trabajo de
#                        compilar-freebsd.sh (por defecto ~/semillero-cross;
#                        se conserva entre trabajos para que la compilación
#                        sea incremental).
#
# Pasos:
# 1. Comprueba las entradas: la etiqueta, una versión vX.Y.Z (version_valida).
# 2. compilar-freebsd.sh --release vX.Y.Z --repo FORGEJO_SERVER_URL/
#    FORGEJO_REPOSITORY.git, con el token en GIT_CONFIG_* (http.extraheader)
#    solo para traer la etiqueta, y sin FORGEJO_TOKEN ni otros tokens del
#    runner en su entorno: la compilación (cargo, build.rs) no los ve.
# 3. La release de la etiqueta: GET /api/v1/repos/{repo}/releases/tags/{tag};
#    si no existe (404), POST /releases con la etiqueta, un nombre y un
#    texto con el commit.
# 4. Sube los ficheros de salida/X.Y.Z/ (el paquete, otros que haya, como
#    una firma, y SHA256SUMS el último: con él, el servidor da la versión
#    por completa) con POST /releases/{id}/assets?name=… (multipart,
#    «attachment»). Antes borra los que la release ya tenga con el mismo
#    nombre, SHA256SUMS el primero: repetir el trabajo los reemplaza.
# Cada curl lleva --fail-with-body y tiempos máximos.

set -euo pipefail

AQUI=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../lib/precompiladas.sh
. "$AQUI/../lib/precompiladas.sh"
BASE=${SEMILLERO_CROSS_DIR:-$HOME/semillero-cross}

error()
{
	echo "ERROR: $*" >&2
	exit 1
}

# --- entradas -------------------------------------------------------------------
for v in FORGEJO_TOKEN FORGEJO_SERVER_URL FORGEJO_REPOSITORY FORGEJO_REF_NAME; do
	[ -n "${!v:-}" ] || error "falta $v (la pone el flujo de Forgejo Actions)"
done
# El token va dentro de la configuración de curl y de una cabecera: solo
# caracteres que no la rompan.
case $FORGEJO_TOKEN in
*[!A-Za-z0-9._-]*) error "FORGEJO_TOKEN no tiene la forma de un token" ;;
esac
TAG=$FORGEJO_REF_NAME
case $TAG in
v*) version_valida "${TAG#v}" || error "FORGEJO_REF_NAME «$TAG» no es una etiqueta vX.Y.Z" ;;
*) error "FORGEJO_REF_NAME «$TAG» no es una etiqueta vX.Y.Z" ;;
esac
VERSION=${TAG#v}
SERVIDOR=${FORGEJO_SERVER_URL%/}
case $SERVIDOR in
http://?* | https://?*) ;;
*) error "FORGEJO_SERVER_URL tiene que ser http:// o https://" ;;
esac
case $SERVIDOR in
*[!A-Za-z0-9.:/_\[\]-]*) error "FORGEJO_SERVER_URL lleva caracteres que no son de una URL" ;;
esac
REPO=$FORGEJO_REPOSITORY
case $REPO in
*/*/* | /* | */ | .* | -* | */.* | */-* | *[!A-Za-z0-9._/-]*) error "FORGEJO_REPOSITORY tiene que ser propietario/nombre" ;;
*/*) ;;
*) error "FORGEJO_REPOSITORY tiene que ser propietario/nombre" ;;
esac
API=$SERVIDOR/api/v1/repos/$REPO
for orden in curl jq tar; do
	command -v "$orden" >/dev/null 2>&1 || error "falta $orden"
done

# --- compilar -------------------------------------------------------------------
# El token, solo en el entorno de git (compilar-freebsd.sh lo quita en
# cuanto trae las etiquetas); ni FORGEJO_TOKEN ni los tokens que el runner
# pudiera dejar llegan a la compilación.
echo "compilando $TAG desde $SERVIDOR/$REPO"
env -u FORGEJO_TOKEN -u GITHUB_TOKEN -u GITEA_TOKEN -u ACTIONS_RUNTIME_TOKEN \
	GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=http.extraheader \
	GIT_CONFIG_VALUE_0="Authorization: token $FORGEJO_TOKEN" GIT_TERMINAL_PROMPT=0 \
	SEMILLERO_CROSS_DIR="$BASE" \
	bash "$AQUI/compilar-freebsd.sh" --release "$TAG" --repo "$SERVIDOR/$REPO.git"

SALIDA=$BASE/salida/$VERSION
PAQUETE=$(archivo_precompilada "$VERSION")
[ -f "$SALIDA/$PAQUETE" ] && [ -f "$SALIDA/SHA256SUMS" ] ||
	error "compilar-freebsd.sh no ha dejado $PAQUETE y SHA256SUMS en $SALIDA"
COMMIT=$(tar -xzOf "$SALIDA/$PAQUETE" "${PAQUETE%.tar.gz}/COMMIT" | head -n 1)
case $COMMIT in
'' | *[!0-9a-f]*) error "el paquete no trae un COMMIT válido" ;;
esac

# --- la release -----------------------------------------------------------------
TRABAJO=$(mktemp -d)
trap 'rm -rf "$TRABAJO"' EXIT

# api MÉTODO RUTA [ARGUMENTOS DE CURL…]: llama a la API del repositorio con
# el token por la entrada estándar de curl. Deja la respuesta en
# $TRABAJO/respuesta y escribe el código HTTP (000 si no hay respuesta).
api()
{
	local metodo=$1 ruta=$2 codigo
	shift 2
	codigo=$(printf 'header = "Authorization: token %s"\n' "$FORGEJO_TOKEN" |
		curl -K - --silent --show-error --fail-with-body --proto =http,https \
			--connect-timeout 30 --max-time 900 -X "$metodo" \
			-o "$TRABAJO/respuesta" -w '%{http_code}' "$@" "$API$ruta") || true
	printf '%s' "${codigo:-000}"
}

# El motivo que da Forgejo, corto y en una línea.
respuesta()
{
	head -c 300 "$TRABAJO/respuesta" 2>/dev/null | tr -c '[:print:]' ' '
}

codigo=$(api GET "/releases/tags/$TAG")
case $codigo in
200) ;;
404)
	echo "creando la release $TAG"
	jq -n --arg t "$TAG" --arg n "semillero $VERSION" \
		--arg b "semillero $VERSION para FreeBSD (x86_64-unknown-freebsd), compilado en el runner del PC desde el commit $COMMIT de $TAG. Lo instala semillero-actualizar (deploy/README.md)." \
		'{tag_name: $t, name: $n, body: $b, draft: false, prerelease: false}' >"$TRABAJO/release.json"
	codigo=$(api POST /releases -H 'Content-Type: application/json' --data-binary "@$TRABAJO/release.json")
	[ "$codigo" = 201 ] || error "Forgejo no crea la release $TAG (HTTP $codigo): $(respuesta)"
	;;
*) error "Forgejo no da la release $TAG (HTTP $codigo): $(respuesta)" ;;
esac
cp "$TRABAJO/respuesta" "$TRABAJO/release-actual.json"
ID=$(jq -r '.id' "$TRABAJO/release-actual.json")
case $ID in
'' | *[!0-9]*) error "Forgejo no da el número de la release $TAG" ;;
esac

# --- los ficheros ---------------------------------------------------------------
# El paquete primero, lo demás (una firma, si la hubiera) después y
# SHA256SUMS el último.
ficheros=("$PAQUETE")
while IFS= read -r f; do
	case $f in
	"$PAQUETE" | SHA256SUMS) ;;
	*) ficheros+=("$f") ;;
	esac
done < <(find "$SALIDA" -mindepth 1 -maxdepth 1 -type f ! -name '.*' -printf '%f\n' | LC_ALL=C sort)
ficheros+=(SHA256SUMS)
for f in "${ficheros[@]}"; do
	case $f in
	*[!A-Za-z0-9._-]*) error "el nombre $f no vale para la release" ;;
	esac
done

# Los que la release ya tiene con el mismo nombre se borran antes de subir
# nada, SHA256SUMS el primero: mientras tanto el servidor ve una versión
# incompleta y espera, nunca un SHA256SUMS viejo con un paquete nuevo.
for f in SHA256SUMS "${ficheros[@]}"; do
	while IFS= read -r aid; do
		[ -n "$aid" ] || continue
		case $aid in
		*[!0-9]*) error "Forgejo da un número de fichero que no vale" ;;
		esac
		echo "borrando $f de la release $TAG"
		codigo=$(api DELETE "/releases/$ID/assets/$aid")
		[ "$codigo" = 204 ] || error "Forgejo no borra $f de la release $TAG (HTTP $codigo): $(respuesta)"
	done < <(jq -r --arg n "$f" '.assets[]? | select(.name == $n) | .id' "$TRABAJO/release-actual.json")
	# Que no se intente borrar dos veces (SHA256SUMS está dos veces en la lista).
	jq --arg n "$f" '.assets = [.assets[]? | select(.name != $n)]' "$TRABAJO/release-actual.json" >"$TRABAJO/sin.json"
	mv "$TRABAJO/sin.json" "$TRABAJO/release-actual.json"
done
for f in "${ficheros[@]}"; do
	echo "subiendo $f"
	codigo=$(api POST "/releases/$ID/assets?name=$f" -F "attachment=@$SALIDA/$f")
	[ "$codigo" = 201 ] || error "Forgejo no acepta $f en la release $TAG (HTTP $codigo): $(respuesta)"
done
echo "publicada $SERVIDOR/$REPO/releases/tag/$TAG"
