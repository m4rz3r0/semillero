#!/bin/sh
# Las versiones publicadas desde las que hay que probar el salto a la nueva
# (tools/ci/salto-de-version.sh; la CI las prueba todas en
# .github/workflows/actualizacion.yml): las releases X.Y.Z de GitHub (sin
# borradores ni versiones de prueba) desde la 0.3.0 y menores que la nueva,
# de la más vieja a la más nueva, una por línea.
#
# Desde la 0.3.0, porque las anteriores buscaban la clave de Jellyfin en una
# ruta fija del sistema (ver salto-de-version.sh) y la promesa de la 1.0 es
# que se actualiza sin pasos a mano cualquier 0.3 o posterior.
#
# Uso: tools/ci/versiones-publicadas.sh [--json] [--hasta X.Y.Z] [ETIQUETAS]
#
# --json        en una línea, como lista JSON (para una matriz de la CI).
# --hasta       la nueva; por defecto, la de crates/semillero/Cargo.toml.
# ETIQUETAS     un fichero con las etiquetas, una por línea («-»: la entrada
#               estándar); sin él, las de las releases de $GITHUB_REPOSITORY
#               (por defecto m4rz3r0/semillero), con gh (en la CI, con
#               GH_TOKEN).
#
# Sin ninguna versión, falla: una lista vacía no prueba nada.

set -eu

RAIZ=$(cd "$(dirname "$0")/../.." && pwd)
REPO=${GITHUB_REPOSITORY:-m4rz3r0/semillero}
DESDE=0.3.0

error()
{
	echo "versiones-publicadas: $*" >&2
	exit 1
}

uso()
{
	sed -n '/^# Uso: /s/^# Uso: //p' "$0" >&2
	exit 2
}

es_version()
{
	printf '%s\n' "$1" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'
}

# menor A B: A < B, comparando X.Y.Z número a número.
menor()
{
	[ "$1" != "$2" ] &&
		[ "$(printf '%s\n%s\n' "$1" "$2" | sort -t. -k1,1n -k2,2n -k3,3n | head -n 1)" = "$1" ]
}

json=0
hasta=
etiquetas=
while [ "$#" -gt 0 ]; do
	case $1 in
	--json) json=1 ;;
	--hasta)
		[ "$#" -ge 2 ] || uso
		hasta=$2
		shift
		;;
	-?*) uso ;;
	*)
		if [ -n "$etiquetas" ]; then uso; fi
		etiquetas=$1
		;;
	esac
	shift
done

if [ -z "$hasta" ]; then
	hasta=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$RAIZ/crates/semillero/Cargo.toml" | head -n 1)
	es_version "$hasta" || error "no se lee la versión de crates/semillero/Cargo.toml"
fi
es_version "$hasta" || error "«$hasta» no es una versión X.Y.Z"

leer()
{
	case $etiquetas in
	'')
		command -v gh >/dev/null 2>&1 || error "falta gh para ver las releases de $REPO"
		gh release list --repo "$REPO" --exclude-drafts --exclude-pre-releases \
			--limit 1000 --json tagName -q '.[].tagName' ||
			error "no se pueden ver las releases de $REPO"
		;;
	-) cat ;;
	*)
		[ -f "$etiquetas" ] || error "no existe $etiquetas"
		cat "$etiquetas"
		;;
	esac
}

lista=$(leer | sed -n 's/^v\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)$/\1/p' |
	while read -r v; do
		if ! menor "$v" "$DESDE" && menor "$v" "$hasta"; then echo "$v"; fi
	done | sort -u -t. -k1,1n -k2,2n -k3,3n)
[ -n "$lista" ] || error "no hay ninguna release de $REPO desde la $DESDE y anterior a la $hasta"

if [ "$json" = 1 ]; then
	printf '%s\n' "$lista" | awk 'BEGIN { printf "[" } { printf "%s\"%s\"", (NR > 1 ? "," : ""), $0 } END { print "]" }'
else
	printf '%s\n' "$lista"
fi
