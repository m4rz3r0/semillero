#!/bin/sh
# Comprobación de la CI: el escáner con las reglas genéricas (sin lista
# privada, que nunca está en el repositorio) sobre el árbol y sobre todo el
# historial y las etiquetas. En GitHub Actions el checkout necesita
# «fetch-depth: 0»; con un clon superficial se niega, porque el historial
# estaría incompleto.
#
# Uso: tools/publicar/ci.sh (desde cualquier sitio)
# Sale con 0 sin hallazgos, 1 con hallazgos y 2 si no puede comprobarlo.

set -eu

HERRAMIENTA=$(cd "$(dirname "$0")" && pwd)
raiz=$(git -C "$HERRAMIENTA" rev-parse --show-toplevel)

if [ "$(git -C "$raiz" rev-parse --is-shallow-repository)" = true ]; then
	echo "ci: el clon es superficial; hace falta el historial entero (fetch-depth: 0)" >&2
	exit 2
fi

estado=0
echo "ci: árbol"
sh "$HERRAMIENTA/escanear.sh" "$raiz" || estado=$?
echo "ci: historial y etiquetas"
sh "$HERRAMIENTA/escanear.sh" --historia "$raiz" || {
	e=$?
	[ "$estado" -ge "$e" ] || estado=$e
}
exit "$estado"
