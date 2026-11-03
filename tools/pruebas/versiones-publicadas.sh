#!/bin/sh
# Pruebas de tools/ci/versiones-publicadas.sh, la lista de versiones desde
# las que la CI prueba el salto (.github/workflows/actualizacion.yml), sin
# red: las etiquetas de las releases se le pasan en un fichero en vez de
# pedírselas a GitHub.
#
# Casos: solo etiquetas vX.Y.Z desde la 0.3.0 y menores que la nueva (ni las
# anteriores a la 0.3.0, ni las de prueba -rc, ni la nueva ni las de
# después), ordenadas número a número (0.10 después de la 0.9) y sin
# repetir; en JSON para la matriz; la nueva por defecto es la de
# crates/semillero/Cargo.toml; sin ninguna versión o con argumentos malos,
# falla y lo dice.
#
# Uso: tools/pruebas/versiones-publicadas.sh (desde cualquier sitio).

set -eu

RAIZ=$(cd "$(dirname "$0")/../.." && pwd)
VP=$RAIZ/tools/ci/versiones-publicadas.sh
T=$(mktemp -d "${TMPDIR:-/tmp}/prueba-versiones.XXXXXX")

fallos=0
n_ok=0
bien()
{
	n_ok=$((n_ok + 1))
}
mal()
{
	echo "FALLO: $*" >&2
	fallos=$((fallos + 1))
}

# igual DESCRIPCIÓN ESPERADO OBTENIDO
igual()
{
	if [ "$2" = "$3" ]; then bien; else mal "$1: se esperaba «$2» y sale «$3»"; fi
}

# en_linea: las líneas de la entrada, separadas por un espacio.
en_linea()
{
	tr '\n' ' ' | sed 's/ $//'
}

if sh -n "$VP"; then bien; else mal "sh -n $VP"; fi

cat >"$T/etiquetas" <<'EOF'
v0.2.0
v0.1.0
v0.3.0
v0.3.1
v0.3.2
v0.4.0-rc.1
v0.4.0
v0.4.1
v0.9.0
v0.10.0
v0.10.0
v1.0.0
v1.2.0
0.5.0
v0.5
otra-cosa
EOF

salida=$(sh "$VP" --hasta 1.0.0 "$T/etiquetas")
igual "lista" "0.3.0 0.3.1 0.3.2 0.4.0 0.4.1 0.9.0 0.10.0" "$(printf '%s\n' "$salida" | en_linea)"

salida=$(sh "$VP" --json --hasta 0.4.1 "$T/etiquetas")
igual "JSON" '["0.3.0","0.3.1","0.3.2","0.4.0"]' "$salida"

salida=$(sh "$VP" --hasta 0.4.1 - <"$T/etiquetas")
igual "de la entrada estándar" "0.3.0 0.3.1 0.3.2 0.4.0" "$(printf '%s\n' "$salida" | en_linea)"

# Sin --hasta: la de Cargo.toml (las etiquetas son todas menores que ella).
nueva=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$RAIZ/crates/semillero/Cargo.toml" | head -n 1)
printf 'v0.3.0\nv%s\n' "$nueva" >"$T/hasta-la-nueva"
salida=$(sh "$VP" "$T/hasta-la-nueva")
igual "hasta la de Cargo.toml" "0.3.0" "$salida"

# Sin ninguna: falla (una matriz vacía no prueba nada).
printf 'v0.2.0\nv0.3.0-rc.1\n' >"$T/ninguna"
if sh "$VP" --hasta 1.0.0 "$T/ninguna" >"$T/salida" 2>"$T/errores"; then
	mal "sin ninguna versión desde la 0.3.0 no falla"
else
	if grep -q 'ninguna release' "$T/errores"; then
		bien
	else
		mal "sin ninguna versión no dice por qué: $(cat "$T/errores")"
	fi
fi
igual "sin ninguna, nada en la salida" "" "$(cat "$T/salida")"

for malos in "--hasta 1.0" "--hasta" "--otra" "$T/etiquetas $T/etiquetas" "$T/no-existe"; do
	# shellcheck disable=SC2086 # se parten a propósito
	if sh "$VP" $malos >/dev/null 2>"$T/errores"; then
		mal "acepta «$malos»"
	else
		if [ -s "$T/errores" ]; then bien; else mal "con «$malos» falla sin decir nada"; fi
	fi
done

if [ "$fallos" -ne 0 ]; then
	echo "prueba-versiones: $fallos fallos, $n_ok bien; se queda $T" >&2
	exit 1
fi
echo "prueba-versiones: ok ($n_ok comprobaciones)"
rm -rf "$T"
