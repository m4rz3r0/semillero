#!/bin/sh
# Empaqueta una versión de semillero para un objetivo: el binario, la web
# (site/ de cargo-leptos), COMMIT, la licencia y el LEEME del repositorio si
# los hay, y lo propio de cada sistema. Lo usa la CI al publicar una versión
# (.github/workflows/release.yml); también sirve a mano.
#
#   empaquetar.sh OBJETIVO X.Y.Z BINARIO SITE SALIDA
#
# OBJETIVO  x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu,
#           x86_64-unknown-freebsd o x86_64-pc-windows-msvc.
# BINARIO   el ejecutable compilado para ese objetivo.
# SITE      la web (target/site); no depende del objetivo.
# SALIDA    carpeta donde dejar el paquete (se crea si no existe).
#
# Deja SALIDA/semillero-X.Y.Z-OBJETIVO.tar.gz (.zip en Windows), con una
# sola carpeta del mismo nombre dentro. El de FreeBSD tiene el nombre y el
# contenido que espera el buzón del servidor (deploy/lib/precompiladas.sh).
# El commit sale de $COMMIT o, si no está, de git rev-parse HEAD. Las fechas
# de dentro son $SOURCE_DATE_EPOCH (o la del commit): con los mismos datos,
# el mismo paquete byte a byte.

set -eu
# Las fechas del zip son locales: siempre en UTC.
TZ=UTC
export TZ

RAIZ=$(cd "$(dirname "$0")/.." && pwd)
EMP=$RAIZ/packaging

error()
{
	echo "empaquetar: $*" >&2
	exit 1
}

if [ "$#" -ne 5 ]; then
	sed -n '/^#   empaquetar/s/^#   //p' "$0" >&2
	exit 2
fi
objetivo=$1
version=$2
binario=$3
site=$4
salida=$5

case $objetivo in
x86_64-unknown-linux-gnu | aarch64-unknown-linux-gnu) sistema=linux ;;
x86_64-unknown-freebsd) sistema=freebsd ;;
x86_64-pc-windows-msvc) sistema=windows ;;
*) error "objetivo desconocido: $objetivo" ;;
esac
printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' ||
	error "«$version» no es una versión X.Y.Z"
[ -f "$binario" ] || error "no existe el binario $binario"
[ -d "$site" ] || error "no existe la web $site"
commit=${COMMIT:-$(git -C "$RAIZ" rev-parse HEAD 2>/dev/null || true)}
printf '%s\n' "$commit" | grep -Eq '^[0-9a-f]{40}$' ||
	error "el commit no es un hash completo: «$commit» (COMMIT=...)"
fecha=${SOURCE_DATE_EPOCH:-$(git -C "$RAIZ" log -1 --format=%ct "$commit" 2>/dev/null || true)}
printf '%s\n' "$fecha" | grep -Eq '^[0-9]+$' || error "falta SOURCE_DATE_EPOCH"

nombre=semillero-$version-$objetivo
trabajo=$(mktemp -d)
trap 'rm -rf "$trabajo"' EXIT
d=$trabajo/$nombre
mkdir -p "$d"

# Los ficheros de texto de Windows, con CR LF (se lean con lo que se lean).
crlf()
{
	sed 's/\r$//; s/$/\r/' "$1" >"$2"
}

cp -R "$site" "$d/site"
find "$d/site" -name '.gitkeep*' -exec rm -f {} +
printf '%s\n' "$commit" >"$d/COMMIT"
for f in LICENSE LICENSE.md README.md README.en.md; do
	if [ -f "$RAIZ/$f" ]; then cp "$RAIZ/$f" "$d/$f"; fi
done
case $sistema in
linux)
	cp "$binario" "$d/semillero"
	cp "$EMP/systemd/semillero.service" "$EMP/systemd/semillero.env.example" "$d/"
	;;
freebsd)
	cp "$binario" "$d/semillero"
	mkdir "$d/rc.d"
	cp "$EMP/freebsd/semillero" "$d/rc.d/semillero"
	chmod 755 "$d/rc.d/semillero"
	;;
windows)
	cp "$binario" "$d/semillero.exe"
	crlf "$EMP/windows/semillero.cmd" "$d/semillero.cmd"
	crlf "$EMP/windows/semillero.env.example" "$d/semillero.env.example"
	cp "$EMP/windows/LEEME.md" "$d/LEEME-windows.md"
	;;
esac
if [ -n "$(find "$d" ! -type f ! -type d)" ]; then
	error "la web trae algo que no es fichero ni directorio"
fi
chmod -R u=rwX,go=rX "$d"
if [ "$sistema" = windows ]; then chmod 755 "$d/semillero.exe"; else chmod 755 "$d/semillero"; fi
find "$d" -exec touch -h -d "@$fecha" {} +

mkdir -p "$salida"
if [ "$sistema" = windows ]; then
	archivo=$nombre.zip
	(cd "$trabajo" && find "$nombre" -type f | LC_ALL=C sort |
		zip -X -q -9 "$trabajo/$archivo" -@)
else
	archivo=$nombre.tar.gz
	tar --sort=name --format=gnu --owner=0 --group=0 --numeric-owner \
		--mtime="@$fecha" -C "$trabajo" -cf - "$nombre" |
		gzip -n -9 >"$trabajo/$archivo"
fi
mv "$trabajo/$archivo" "$salida/$archivo"
echo "$salida/$archivo"
