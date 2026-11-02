#!/bin/sh
# Receptor del buzón de versiones precompiladas de semillero. Corre EN EL
# HOST como el usuario del buzón (por defecto smentrega, sin privilegios), como
# orden forzada de su authorized_keys (restrict,from="…",command="…"): la
# única clave que entra es la del PC de compilación y lo único que puede
# hacer es esto. Se instala como
# /usr/local/libexec/semillero-actualizar/recibir-precompilada.sh.
#
#   ssh smentrega@servidor recibir <versión> <fichero> <fichero-local
#
# <versión> es X.Y.Z y <fichero>, semillero-X.Y.Z-x86_64-unknown-freebsd.tar.gz
# o SHA256SUMS (deploy/lib/precompiladas.sh). El contenido llega por la
# entrada estándar y se guarda en <buzón>/<versión>/<fichero>: primero en un
# temporal y luego con un rename, así que el actualizador nunca ve un fichero
# a medias. El PC sube SHA256SUMS el último. Como mucho 300 MB por fichero y
# las 3 versiones más recientes (las demás se borran).
#
# Nada de lo que llega se ejecuta ni se interpreta aquí: lo comprueba el
# actualizador (sha256, commit de la etiqueta, tipo de binario) antes de
# instalarlo.
#
# Solo para las pruebas: SEMILLERO_BUZON_LIB, SEMILLERO_BUZON y
# SEMILLERO_BUZON_MAXIMO cambian la biblioteca, la carpeta y el máximo. El
# cliente SSH no puede ponerlas: el sshd de FreeBSD no admite ninguna
# variable del cliente (AcceptEnv) salvo que se configure.

set -u
set -f
umask 027

LIB=${SEMILLERO_BUZON_LIB:-/usr/local/libexec/semillero-actualizar/precompiladas.sh}
BUZON=${SEMILLERO_BUZON:-/var/db/semillero-buzon/entrada}
# 300 MB: el paquete pesa unas decenas.
MAXIMO=${SEMILLERO_BUZON_MAXIMO:-314572800}
CONSERVAR=3

error()
{
	echo "ERROR: $*" >&2
	exit 1
}

[ -f "$LIB" ] || error "falta $LIB"
# shellcheck source=../lib/precompiladas.sh
. "$LIB"

# Con restrict,command=… la orden pedida llega en SSH_ORIGINAL_COMMAND; sin
# ella (pruebas, o a mano), en los argumentos.
if [ -n "${SSH_ORIGINAL_COMMAND:-}" ]; then
	# shellcheck disable=SC2086 # se parte en palabras a propósito (set -f)
	set -- $SSH_ORIGINAL_COMMAND
fi
[ "$#" -eq 3 ] && [ "$1" = recibir ] || error "uso: recibir <versión> <fichero>"
VER=$2
FICHERO=$3
version_valida "$VER" || error "versión no válida"
fichero_de_buzon "$VER" "$FICHERO" || error "fichero no admitido para $VER"
[ -d "$BUZON" ] || error "no existe el buzón"

mkdir -p "$BUZON/$VER" || error "no se puede crear la carpeta de $VER"
TMP=$BUZON/$VER/.$FICHERO.$$
trap 'rm -f "$TMP"' EXIT
trap 'exit 143' INT TERM HUP
head -c "$((MAXIMO + 1))" >"$TMP" || error "no se puede escribir $FICHERO"
TAM=$(wc -c <"$TMP" | tr -cd '0-9')
[ "$TAM" -gt 0 ] || error "$FICHERO llega vacío"
[ "$TAM" -le "$MAXIMO" ] || error "$FICHERO pasa de $MAXIMO bytes"
mv -f "$TMP" "$BUZON/$VER/$FICHERO" || error "no se puede guardar $FICHERO"

# Solo las versiones más recientes (por fecha de modificación).
# shellcheck disable=SC2012 # nombres validados: sin espacios ni saltos
ls -1t "$BUZON" | tail -n +$((CONSERVAR + 1)) | while read -r _vieja; do
	if version_valida "$_vieja" && [ "$_vieja" != "$VER" ]; then
		rm -rf "${BUZON:?}/$_vieja"
	fi
done
echo "recibido $VER/$FICHERO ($TAM bytes)"
