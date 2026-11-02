#!/bin/sh
# Sintaxis de todos los guiones de deploy/: sh -n en cada *.sh (bash -n en
# los de bash, los del PC de compilación en deploy/cross/) y, si está
# instalado, shellcheck con avisos y errores (el shell lo toma del #! o de la directiva
# «shellcheck shell=sh» de deploy/lib/). Termina con error si alguno falla.
#
# Uso: deploy/pruebas/sintaxis.sh (desde cualquier sitio).

set -eu

DEPLOY=$(cd "$(dirname "$0")/.." && pwd)
fallos=0
n=0
lista=$(find "$DEPLOY" -name '*.sh' -type f | sort)
for f in $lista; do
	n=$((n + 1))
	case $(head -n 1 "$f") in
	*bash*) shell="bash" ;;
	*) shell="sh" ;;
	esac
	if ! "$shell" -n "$f"; then
		echo "FALLO: $shell -n ${f#"$DEPLOY"/}" >&2
		fallos=$((fallos + 1))
	fi
done
if command -v shellcheck >/dev/null 2>&1; then
	# -x -P SCRIPTDIR: sigue los «source=../lib/…» desde la carpeta de cada guion.
	# shellcheck disable=SC2086 # rutas de deploy/, sin espacios
	if ! shellcheck -x -P SCRIPTDIR -S warning $lista; then
		echo "FALLO: shellcheck" >&2
		fallos=$((fallos + 1))
	fi
else
	echo "aviso: no hay shellcheck; solo sh -n y bash -n" >&2
fi
if [ "$fallos" -ne 0 ]; then
	echo "$fallos fallos" >&2
	exit 1
fi
echo "sintaxis: ok ($n guiones)"
