#!/bin/sh
# Instala el gancho pre-push de tools/publicar/gancho-pre-push en un
# repositorio (por defecto, el de la carpeta actual) y guarda su
# configuración en git config.
#
# Uso: instalar-gancho.sh [--publico URL] [--privado FICHERO] [--forzar] [REPO]
#   --publico  dirección del repositorio público (semillero.publico)
#   --privado  lista privada (semillero.privado)
#   --forzar   sustituye un pre-push distinto que ya esté instalado
# Sale con 0 si queda instalado y 2 si no.

set -eu

HERRAMIENTA=$(cd "$(dirname "$0")" && pwd)

uso()
{
	sed -n '/^# Uso:/,/^# Sale/p' "$0" | sed 's/^# \{0,1\}//' >&2
	exit 2
}

error()
{
	echo "instalar-gancho: $*" >&2
	exit 2
}

publico=
privado=
forzar=0
repo=.
while [ $# -gt 0 ]; do
	case $1 in
	--publico)
		[ $# -ge 2 ] || uso
		publico=$2
		shift
		;;
	--privado)
		[ $# -ge 2 ] || uso
		privado=$2
		shift
		;;
	--forzar) forzar=1 ;;
	-*) uso ;;
	*)
		[ $# -eq 1 ] || uso
		repo=$1
		;;
	esac
	shift
done

git -C "$repo" rev-parse --git-dir >/dev/null 2>&1 || error "$repo no es un repositorio git"
# --git-path hooks respeta core.hooksPath; la ruta sale relativa al repo.
ganchos=$(cd "$repo" && d=$(git rev-parse --git-path hooks) && mkdir -p "$d" && cd "$d" && pwd)
destino=$ganchos/pre-push
if [ -e "$destino" ] && ! cmp -s "$destino" "$HERRAMIENTA/gancho-pre-push" && [ "$forzar" -eq 0 ]; then
	error "ya hay otro gancho pre-push en $destino; usa --forzar para sustituirlo"
fi
cp "$HERRAMIENTA/gancho-pre-push" "$destino.nuevo"
chmod 755 "$destino.nuevo"
mv "$destino.nuevo" "$destino"

git -C "$repo" config semillero.escaner "$HERRAMIENTA/escanear.sh"
[ -z "$publico" ] || git -C "$repo" config semillero.publico "$publico"
[ -z "$privado" ] || git -C "$repo" config semillero.privado "$privado"

echo "instalar-gancho: instalado en $destino"
if [ -z "$(git -C "$repo" config --get semillero.publico || true)" ]; then
	echo "instalar-gancho: aviso: falta semillero.publico; hasta entonces el gancho no comprueba nada" >&2
fi
