#!/usr/bin/env bash
# Desde el equipo de trabajo: compila en el PC de compilación una etiqueta
# (o, con --prueba, cualquier referencia) y, si es una etiqueta, la entrega al
# buzón del servidor. Se lanza a mano después de crear y subir la etiqueta.
#
#   deploy/cross/lanzar.sh vX.Y.Z           compila la etiqueta y la entrega
#   deploy/cross/lanzar.sh --prueba [REF]   solo compila REF (por defecto HEAD)
#
# Lee de deploy/config.local PC_COMPILAR_SSH (cómo se entra al PC de
# compilación) y PRECOMPILADAS_BUZON_SSH (el usuario del buzón en el servidor,
# como lo ve el PC). Manda al PC deploy/cross/, deploy/lib/ y un git bundle de
# la referencia; nada de la copia de trabajo acaba en el binario.
#
# La etiqueta tiene que ser la versión de crates/semillero/Cargo.toml en ese
# commit (es la que dice /salud y la que espera el actualizador).
#
# Si el PC no responde (apagado o en el otro sistema), lo dice y sale con 3:
# el servidor compilará él mismo cuando pase PRECOMPILADAS_ESPERA.
#
# SEMILLERO_CONFIG: otra configuración en lugar de deploy/config.local.

set -euo pipefail

RAIZ=$(cd "$(dirname "$0")/../.." && pwd)
DEPLOY_DIR=$RAIZ/deploy
# shellcheck source=../lib/config.sh
. "$DEPLOY_DIR/lib/config.sh"
# shellcheck source=../lib/precompiladas.sh
. "$DEPLOY_DIR/lib/precompiladas.sh"

uso()
{
	echo "uso: $0 vX.Y.Z | --prueba [REF]" >&2
	exit 2
}

error()
{
	echo "ERROR: $*" >&2
	exit 1
}

prueba=
ref=
case ${1:-} in
--prueba)
	prueba=1
	ref=${2:-HEAD}
	[ "$#" -le 2 ] || uso
	;;
'' | -*) uso ;;
*)
	ref=$1
	[ "$#" -eq 1 ] || uso
	;;
esac

cargar_config "${SEMILLERO_CONFIG:-}" >/dev/null || exit 1
[ -n "${PC_COMPILAR_SSH:-}" ] || error "falta PC_COMPILAR_SSH en la configuración"
if [ -z "$prueba" ]; then
	[ -n "${PRECOMPILADAS_BUZON_SSH:-}" ] || error "falta PRECOMPILADAS_BUZON_SSH en la configuración"
	case $ref in
	v*) version_valida "${ref#v}" || error "«$ref» no es una versión vX.Y.Z (para otra referencia, --prueba)" ;;
	*) error "«$ref» no es una versión vX.Y.Z (para otra referencia, --prueba)" ;;
	esac
	git -C "$RAIZ" rev-parse --verify --quiet "refs/tags/$ref^{commit}" >/dev/null ||
		error "$ref no es una etiqueta (solo se entregan etiquetas vX.Y.Z)"
	# Igual que antes de crear la etiqueta: la copia de trabajo limpia.
	if [ -n "$(git -C "$RAIZ" status --porcelain --untracked-files=no)" ]; then
		error "hay cambios sin confirmar; no se compila una versión desde una copia sucia"
	fi
	cargo_v=$(git -C "$RAIZ" show "refs/tags/$ref:crates/semillero/Cargo.toml" 2>/dev/null | version_cargo)
	[ "v$cargo_v" = "$ref" ] ||
		error "la etiqueta $ref no coincide con la versión de crates/semillero/Cargo.toml en ese commit («$cargo_v»)"
	ref=refs/tags/$ref
else
	git -C "$RAIZ" rev-parse --verify --quiet "$ref^{commit}" >/dev/null ||
		error "$ref no existe"
fi

SSH=(ssh -o BatchMode=yes -o ConnectTimeout=5 -o ServerAliveInterval=15 -o ServerAliveCountMax=4)
if ! "${SSH[@]}" "$PC_COMPILAR_SSH" true 2>/dev/null; then
	echo "el PC de compilación no responde; el servidor compilará la versión él mismo" >&2
	exit 3
fi

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
# Las órdenes remotas valen para cualquier shell (sh, bash o fish): sin
# $(…) ni redirecciones raras.
tar -cf - -C "$DEPLOY_DIR" cross lib |
	"${SSH[@]}" "$PC_COMPILAR_SSH" 'mkdir -p semillero-cross/bin && tar -xf - -C semillero-cross/bin'
if [ -n "$prueba" ]; then
	# HEAD y, si REF tiene nombre (rama o etiqueta), también REF; un commit
	# suelto tiene que ser de la historia de HEAD.
	refs=(HEAD)
	nombre=$(git -C "$RAIZ" rev-parse --symbolic-full-name "$ref" 2>/dev/null || true)
	case $nombre in
	refs/*) refs+=("$nombre") ;;
	esac
	git -C "$RAIZ" bundle create --quiet "$TMP/ref.bundle" "${refs[@]}"
	args=(--prueba "$(git -C "$RAIZ" rev-parse "$ref^{commit}")")
else
	git -C "$RAIZ" bundle create --quiet "$TMP/ref.bundle" "$ref"
	args=(--release "${ref#refs/tags/}")
fi
"${SSH[@]}" "$PC_COMPILAR_SSH" 'cat > semillero-cross/ref.bundle' <"$TMP/ref.bundle"
if [ -z "$prueba" ]; then
	args+=(--entregar "$PRECOMPILADAS_BUZON_SSH")
fi
"${SSH[@]}" "$PC_COMPILAR_SSH" bash semillero-cross/bin/cross/compilar-freebsd.sh \
	--bundle semillero-cross/ref.bundle "${args[@]}"
