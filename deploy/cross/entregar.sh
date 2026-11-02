#!/usr/bin/env bash
# Sube una versión precompilada (la salida de compilar-freebsd.sh) al buzón
# del servidor. Corre en el PC de compilación.
#
#   entregar.sh --destino USUARIO@SERVIDOR SALIDA
#
# SALIDA es salida/X.Y.Z/ de compilar-freebsd.sh. Antes de subir nada
# comprueba que el paquete cuadra con su SHA256SUMS. Se conecta como el
# usuario del buzón (smentrega), cuya única orden es recibir-precompilada.sh
# (restrict,command=… en su authorized_keys), con la clave
# SEMILLERO_BUZON_CLAVE (por defecto ~/.ssh/semillero-buzon). Una llamada
# «recibir X.Y.Z <fichero>» por fichero: el paquete primero y SHA256SUMS el
# último, porque el actualizador da la versión por llegada cuando está
# SHA256SUMS. Al terminar deja SALIDA/.entregada, que compilar-freebsd.sh
# --ultima mira para no repetir.

set -euo pipefail

AQUI=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../lib/precompiladas.sh
. "$AQUI/../lib/precompiladas.sh"
CLAVE=${SEMILLERO_BUZON_CLAVE:-$HOME/.ssh/semillero-buzon}

uso()
{
	echo "uso: $0 --destino USUARIO@SERVIDOR SALIDA" >&2
	exit 2
}

error()
{
	echo "ERROR: $*" >&2
	exit 1
}

[ "$#" -eq 3 ] && [ "$1" = --destino ] || uso
DESTINO=$2
SALIDA=${3%/}
[ -d "$SALIDA" ] || error "no existe $SALIDA"
VER=$(basename "$SALIDA")
version_valida "$VER" || error "«$VER» no es una versión que admita el buzón (las de --prueba no se entregan)"
[ -f "$CLAVE" ] || error "falta la clave del buzón ($CLAVE)"
ARCH=$(archivo_precompilada "$VER")
for f in "$ARCH" SHA256SUMS; do
	[ -f "$SALIDA/$f" ] || error "falta $SALIDA/$f"
done
# Antes de subir: que lo que se sube cuadre.
(cd "$SALIDA" && sha256sum -c --status SHA256SUMS) || error "SHA256SUMS no cuadra con $ARCH"

SSH=(ssh -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=15
	-o ServerAliveCountMax=4 -o IdentitiesOnly=yes -i "$CLAVE")
subir()
{
	"${SSH[@]}" "$DESTINO" recibir "$VER" "$1" <"$SALIDA/$1" ||
		error "no se puede subir $1"
}
subir "$ARCH"
subir SHA256SUMS
: >"$SALIDA/.entregada"
echo "entregada $VER"
