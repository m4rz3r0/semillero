#!/bin/sh
# Pruebas del arranque de FreeBSD (packaging/freebsd/semillero) que se
# pueden hacer fuera de FreeBSD: cómo lee semillero_env_file. Se toma la
# función semillero_leer_env del guion y se ejecuta con err, warn y stat de
# mentira, en una carpeta temporal fuera del repositorio.
#
# 1. Exporta las SEMILLERO_* del fichero, sin comillas, con CRLF o sin el
#    salto de línea final, y se salta comentarios y líneas vacías.
# 2. Ignora (con aviso) lo que no es SEMILLERO_*: LD_PRELOAD, PATH…
# 3. Un valor no se ejecuta nunca como orden del shell.
# 4. Un fichero que no es de root, o que otros pueden escribir, no se lee.
#
# Uso: packaging/pruebas/rcd.sh (desde cualquier sitio).

set -eu

RAIZ=$(cd "$(dirname "$0")/../.." && pwd)
RC=$RAIZ/packaging/freebsd/semillero
T=$(mktemp -d)
fallos=0
n=0

ok()
{
	n=$((n + 1))
	echo "ok   $1"
}

mal()
{
	n=$((n + 1))
	fallos=$((fallos + 1))
	echo "MAL  $1"
}

# La función tal cual, desde su línea hasta la primera «}» al principio.
sed -n '/^semillero_leer_env()$/,/^}$/p' "$RC" >"$T/funcion.sh"
grep -q 'semillero_leer_env' "$T/funcion.sh" || {
	echo "MAL  no se encuentra semillero_leer_env en $RC"
	exit 1
}

# Lee el fichero $1 con la función, con el dueño $2 (uid), y escribe las
# SEMILLERO_* resultantes, ordenadas; los avisos van a $T/avisos.
leer()
{
	# shellcheck disable=SC2016 # se expande en el sh de dentro
	env -i PATH="$PATH" DUENO="$2" sh -c '
		err() { shift; echo "ERROR: $*"; exit 1; }
		warn() { echo "AVISO: $*" >&2; }
		stat() { echo "$DUENO"; }
		. "$1"
		semillero_leer_env "$2"
		env | grep -E "^(SEMILLERO_|LD_PRELOAD|PATH=/malo)" | LC_ALL=C sort
	' sh "$T/funcion.sh" "$1" 2>"$T/avisos"
}

printf '%s\n' \
	'# comentario' \
	'' \
	'SEMILLERO_LISTEN=192.0.2.10:8090' \
	'SEMILLERO_ORIGEN=https://semillero.example,https://otra.example' \
	'SEMILLERO_QBT_CLAVE=/usr/local/etc/semillero/qbt clave' \
	'LD_PRELOAD=/tmp/malo.so' \
	'PATH=/malo' \
	'SEMILLERO_mal-nombre=1' \
	"SEMILLERO_DB=\$(touch $T/ejecutado);\`touch $T/ejecutado\`" \
	'sin igual' >"$T/env"
printf 'SEMILLERO_QBT=http://192.0.2.20:8080\r\nSEMILLERO_PROXY=192.0.2.1' >>"$T/env"
chmod 600 "$T/env"

salida=$(leer "$T/env" 0) || true
esperado="SEMILLERO_DB=\$(touch $T/ejecutado);\`touch $T/ejecutado\`
SEMILLERO_LISTEN=192.0.2.10:8090
SEMILLERO_ORIGEN=https://semillero.example,https://otra.example
SEMILLERO_PROXY=192.0.2.1
SEMILLERO_QBT=http://192.0.2.20:8080
SEMILLERO_QBT_CLAVE=/usr/local/etc/semillero/qbt clave"
if [ "$salida" = "$esperado" ]; then ok "exporta solo las SEMILLERO_*, tal cual"; else
	mal "exporta otra cosa:"
	printf '%s\n' "$salida"
fi
if [ ! -e "$T/ejecutado" ]; then ok "un valor no se ejecuta"; else mal "un valor se ha ejecutado"; fi
if [ "$(grep -c 'se ignora' "$T/avisos")" -eq 4 ]; then ok "avisa de las 4 líneas ignoradas"; else
	mal "avisos: $(cat "$T/avisos")"
fi

salida=$(leer "$T/env" 1001) || true
case $salida in
"ERROR: "*"tiene que ser de root"*) ok "no lee un fichero que no es de root" ;;
*) mal "lee un fichero que no es de root: $salida" ;;
esac
chmod 620 "$T/env"
salida=$(leer "$T/env" 0) || true
case $salida in
"ERROR: "*) ok "no lee un fichero que el grupo puede escribir" ;;
*) mal "lee un fichero que el grupo puede escribir: $salida" ;;
esac
chmod 602 "$T/env"
salida=$(leer "$T/env" 0) || true
case $salida in
"ERROR: "*) ok "no lee un fichero que otros pueden escribir" ;;
*) mal "lee un fichero que otros pueden escribir: $salida" ;;
esac

echo "$n comprobaciones, $fallos fallos"
if [ "$fallos" -ne 0 ]; then
	echo "carpeta de la prueba: $T"
	exit 1
fi
rm -rf "$T"
