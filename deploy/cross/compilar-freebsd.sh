#!/usr/bin/env bash
# Compila semillero para FreeBSD (x86_64-unknown-freebsd) en el PC, un Linux
# con rustup, podman y cross, y lo empaqueta como lo espera el actualizador
# del servidor. Así el servidor no tiene que compilar (compilar Rust pide
# mucha memoria): su actualizador instala la release de Forgejo que publica
# con esto deploy/cross/publicar-forgejo.sh desde el runner de Forgejo
# Actions del PC (deploy/README.md).
#
#   compilar-freebsd.sh --release vX.Y.Z [--bundle FICHERO] [--repo URL]
#   compilar-freebsd.sh --prueba [REF] [--bundle FICHERO]
#
# --release   compila la etiqueta vX.Y.Z (tiene que ser una etiqueta, no una
#             rama ni un commit, y la versión de crates/semillero/Cargo.toml
#             en ese commit) y deja en salida/X.Y.Z/ el paquete
#             semillero-X.Y.Z-x86_64-unknown-freebsd.tar.gz y su SHA256SUMS.
# --prueba    compila REF (por defecto HEAD del bundle o del clon) para probar
#             la compilación: la versión se llama prueba-<sha corto> y el
#             servidor no la admite.
# --bundle    un `git bundle` que trae la referencia (git bundle create … --all
#             desde una copia del repositorio); sin él, el clon tiene que
#             tenerla ya.
# --repo      URL de git de la que traer las etiquetas (publicar-forgejo.sh
#             pasa la de Forgejo, con el token en GIT_CONFIG_COUNT,
#             GIT_CONFIG_KEY_0 y GIT_CONFIG_VALUE_0: en el entorno, nunca en
#             la URL). Esas tres variables se quitan del entorno en cuanto
#             se traen las etiquetas: la compilación (cargo, build.rs de las
#             dependencias) no las ve.
#
# Todo vive en SEMILLERO_CROSS_DIR (por defecto ~/semillero-cross): repo/
# (clon desnudo), fuente/ (el código de la referencia, con target/ dentro
# para que la siguiente compilación sea incremental), salida/<versión>/ y
# compilacion.log. El código sale de git (git archive del commit), nunca de
# una copia de trabajo: un cambio sin confirmar no puede acabar en una
# versión. Una compilación a la vez (cerrojo .lock en esa carpeta).
#
# Cómo se compila: cargo leptos build --release --precompress, igual que el
# servidor, pero con el binario para FreeBSD (LEPTOS_BIN_TARGET_TRIPLE) y
# compilado con cross (LEPTOS_BIN_CARGO_COMMAND=cross: un contenedor de
# podman con el enlazador y la libc de FreeBSD; SQLite va compilado dentro
# del binario). La web (wasm, JS y CSS) no depende de la plataforma; la hace
# cargo-leptos con la misma versión que el servidor (CARGO_LEPTOS_VERSION de
# deploy/freebsd/install.sh), que es la que fija wasm-bindgen.

set -euo pipefail

BASE=${SEMILLERO_CROSS_DIR:-$HOME/semillero-cross}
AQUI=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../lib/precompiladas.sh
. "$AQUI/../lib/precompiladas.sh"
OBJ=$PRECOMPILADA_OBJETIVO
CARGO_LEPTOS_VERSION=0.3.11
TIEMPO_MAXIMO=3600

error()
{
	echo "ERROR: $*" >&2
	exit 1
}

uso()
{
	sed -n '/^#   compilar/s/^#   //p' "$0" >&2
	exit 2
}

modo=
ref=
bundle=
repo_url=
while [ "$#" -gt 0 ]; do
	case $1 in
	--release)
		modo=release
		ref=${2:-}
		[ -n "$ref" ] || uso
		shift 2
		;;
	--prueba)
		modo=prueba
		shift
		if [ "$#" -gt 0 ] && [ "${1#--}" = "$1" ]; then
			ref=$1
			shift
		fi
		;;
	--bundle)
		bundle=${2:-}
		[ -n "$bundle" ] || uso
		shift 2
		;;
	--repo)
		repo_url=${2:-}
		[ -n "$repo_url" ] || uso
		shift 2
		;;
	*) uso ;;
	esac
done
[ -n "$modo" ] || uso
if [ -n "$bundle" ]; then
	bundle=$(realpath -e "$bundle") || error "no existe el bundle"
fi

for orden in git cross cargo rustup podman file sha256sum tar gzip rsync flock timeout; do
	command -v "$orden" >/dev/null 2>&1 || error "falta $orden"
done
mkdir -p "$BASE/salida"
exec 9>"$BASE/.lock"
flock -n 9 || error "ya hay otra compilación en marcha en $BASE"

REPO=$BASE/repo
if [ ! -d "$REPO" ]; then
	git init --quiet --bare "$REPO"
fi

# --- referencia --------------------------------------------------------------
if [ -n "$bundle" ]; then
	git -C "$REPO" bundle verify --quiet "$bundle" >/dev/null 2>&1 ||
		error "el bundle no es válido"
	# Etiquetas tal cual (--force: una etiqueta movida en el origen gana) y
	# el resto de referencias bajo refs/semillero/.
	git -C "$REPO" fetch --quiet --force "$bundle" \
		'+refs/tags/*:refs/tags/*' '+refs/heads/*:refs/semillero/heads/*' '+HEAD:refs/semillero/HEAD' 2>/dev/null ||
		git -C "$REPO" fetch --quiet --force "$bundle" '+refs/tags/*:refs/tags/*' ||
		error "no se pueden traer las referencias del bundle"
fi
if [ -n "$repo_url" ]; then
	git -C "$REPO" fetch --quiet --force --prune --prune-tags "$repo_url" '+refs/tags/*:refs/tags/*' ||
		error "no se pueden traer las etiquetas del repositorio"
fi
# La cabecera con el token (si la hay) solo hacía falta para traer las
# etiquetas: nada de lo que sigue la ve.
unset GIT_CONFIG_COUNT GIT_CONFIG_KEY_0 GIT_CONFIG_VALUE_0

if [ "$modo" = release ]; then
	case $ref in
	v*) version_valida "${ref#v}" || error "«$ref» no es una versión vX.Y.Z" ;;
	*) error "«$ref» no es una versión vX.Y.Z" ;;
	esac
	# Solo etiquetas: una rama con el nombre de la versión no vale.
	commit=$(git -C "$REPO" rev-parse --verify --quiet "refs/tags/$ref^{commit}") ||
		error "$ref no es una etiqueta del repositorio (--release solo compila etiquetas vX.Y.Z)"
	version=${ref#v}
	cargo_v=$(git -C "$REPO" show "$commit:crates/semillero/Cargo.toml" 2>/dev/null | version_cargo)
	[ "$cargo_v" = "$version" ] ||
		error "la etiqueta $ref no coincide con la versión de crates/semillero/Cargo.toml («$cargo_v»)"
else
	if [ -z "$ref" ]; then
		if git -C "$REPO" rev-parse --verify --quiet 'refs/semillero/HEAD^{commit}' >/dev/null; then
			ref=refs/semillero/HEAD
		else
			ref=HEAD
		fi
	fi
	commit=$(git -C "$REPO" rev-parse --verify --quiet "$ref^{commit}" ||
		git -C "$REPO" rev-parse --verify --quiet "refs/semillero/heads/$ref^{commit}") ||
		error "$ref no existe en el repositorio"
	version=prueba-$(printf '%s' "$commit" | cut -c1-12)
fi
echo "compilando $version ($commit) para $OBJ"

# --- código ------------------------------------------------------------------
# git archive a una carpeta nueva y rsync --checksum a fuente/: lo que no
# cambia conserva su fecha y cargo no lo recompila.
NUEVA=$BASE/.fuente-nueva
rm -rf "$NUEVA"
mkdir -p "$NUEVA" "$BASE/fuente"
git -C "$REPO" archive --format=tar "$commit" | tar -xf - -C "$NUEVA"
rsync -rlpD --checksum --delete --exclude /target "$NUEVA/" "$BASE/fuente/"
rm -rf "$NUEVA"
FUENTE=$BASE/fuente
cd "$FUENTE"

# --- herramientas --------------------------------------------------------------
quiere=$(sed -n 's/^CARGO_LEPTOS_VERSION=//p' deploy/freebsd/install.sh 2>/dev/null | head -n 1)
quiere=${quiere:-$CARGO_LEPTOS_VERSION}
tiene=$(cargo leptos --version 2>/dev/null | cut -d' ' -f2 || true)
if [ "$quiere" != "$tiene" ]; then
	echo "instalando cargo-leptos $quiere (había: ${tiene:-ninguno})"
	cargo install --locked cargo-leptos --version "$quiere"
fi
# La Rust de rust-toolchain.toml con el objetivo de FreeBSD (rustup la
# instala si falta; cross usa la misma, montada en el contenedor).
rustup target add "$OBJ" >/dev/null

# --- compilación ---------------------------------------------------------------
inicio=$(date +%s)
if ! LEPTOS_BIN_TARGET_TRIPLE=$OBJ LEPTOS_BIN_CARGO_COMMAND=cross \
	CROSS_BUILD_ENV_PASSTHROUGH=LEPTOS_OUTPUT_NAME \
	CROSS_CONTAINER_ENGINE=${CROSS_CONTAINER_ENGINE:-podman} \
	timeout -k 60 "$TIEMPO_MAXIMO" nice -n 10 \
	cargo leptos build --release --precompress >"$BASE/compilacion.log" 2>&1; then
	tail -n 20 "$BASE/compilacion.log" >&2
	error "falló la compilación (ver $BASE/compilacion.log)"
fi
echo "compilado en $(($(date +%s) - inicio)) s"

BINARIO=$FUENTE/target/$OBJ/release/semillero
SITE=$FUENTE/target/site
[ -f "$BINARIO" ] || error "falta $BINARIO"
binario_freebsd "$BINARIO" || error "el binario no es de FreeBSD x86-64: $(file -b "$BINARIO")"
# Con el objetivo de FreeBSD, cargo-leptos deja hash.txt junto a la carpeta
# del binario (hash-file-name es relativo a target/<objetivo>/release);
# semillero lo busca dentro de site/.
if [ -f "$FUENTE/target/$OBJ/site/hash.txt" ]; then
	cp "$FUENTE/target/$OBJ/site/hash.txt" "$SITE/hash.txt"
fi
web_completa "$SITE" || error "la web de target/site no está completa"

# --- paquete -------------------------------------------------------------------
NOMBRE=$(archivo_precompilada "$version")
NOMBRE=${NOMBRE%.tar.gz}
SALIDA=$BASE/salida/$version
TRABAJO=$(mktemp -d)
trap 'rm -rf "$TRABAJO"' EXIT
D=$TRABAJO/$NOMBRE
mkdir -p "$D"
cp "$BINARIO" "$D/semillero"
# Solo lo que corresponde a esta compilación: sin restos de otras.
cp -R "$SITE" "$D/site"
find "$D/site" -name '.gitkeep*' -exec rm -f {} +
echo "$commit" >"$D/COMMIT"
if [ -f "$FUENTE/LICENSE" ]; then cp "$FUENTE/LICENSE" "$D/LICENSE"; fi
chmod -R u=rwX,go=rX "$D"
chmod 755 "$D/semillero"
if [ -n "$(find "$D" ! -type f ! -type d)" ]; then
	error "site/ trae algo que no es fichero ni directorio"
fi
SOURCE_DATE_EPOCH=$(git -C "$REPO" log -1 --format=%ct "$commit")
rm -rf "$SALIDA"
mkdir -p "$SALIDA"
tar --sort=name --format=gnu --owner=0 --group=0 --numeric-owner \
	--mtime="@$SOURCE_DATE_EPOCH" -C "$TRABAJO" -cf - "$NOMBRE" |
	gzip -n -9 >"$SALIDA/$NOMBRE.tar.gz"
(cd "$SALIDA" && LC_ALL=C sha256sum -- "$NOMBRE.tar.gz" >SHA256SUMS)
echo "$SALIDA/$NOMBRE.tar.gz"
