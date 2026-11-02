#!/bin/sh
# Pruebas de packaging/empaquetar.sh (los paquetes de cada versión) sin
# compilar nada: un binario y una web de mentira en una carpeta temporal
# fuera del repositorio, que se borra al terminar si todo va bien.
#
# 1. Linux: tar.gz con una sola carpeta semillero-X.Y.Z-<objetivo>/ que
#    trae semillero (ejecutable), site/ sin .gitkeep, COMMIT, la unidad de
#    systemd y su ejemplo de configuración.
# 2. FreeBSD: el mismo nombre que espera el buzón del servidor
#    (archivo_precompilada de deploy/lib/precompiladas.sh) y lo que pide:
#    semillero, site/ y COMMIT, y además el arranque rc.d/semillero.
# 3. Windows: zip con semillero.exe, site/, semillero.cmd con finales de
#    línea CRLF, el ejemplo de configuración y LEEME-windows.md.
# 4. Dos pasadas con los mismos datos dan el mismo fichero byte a byte.
# 5. Errores: sin argumentos (uso, sale con 2), objetivo desconocido,
#    versión que no es X.Y.Z, binario o web que no existen y commit que no
#    es un hash completo; en ningún caso queda un paquete a medias.
#
# Uso: packaging/pruebas/empaquetar.sh (desde cualquier sitio; necesita tar,
# gzip, zip, unzip y sha256sum). Con SEMILLERO_PRUEBA_CONSERVAR=1 no borra la
# carpeta temporal; si algo falla, tampoco.

set -eu

RAIZ=$(cd "$(dirname "$0")/../.." && pwd)
EMP=$RAIZ/packaging/empaquetar.sh
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

comprobar()
{
	_desc=$1
	shift
	if "$@" >/dev/null 2>&1; then ok "$_desc"; else mal "$_desc"; fi
}

COMMIT=0123456789abcdef0123456789abcdef01234567
V=1.2.3
export COMMIT SOURCE_DATE_EPOCH=1700000000

mkdir -p "$T/web/pkg" "$T/salida"
printf 'binario de mentira\n' >"$T/semillero"
printf 'wasm\n' >"$T/web/pkg/semillero.wasm"
printf 'abc\n' >"$T/web/hash.txt"
: >"$T/web/pkg/.gitkeep"

# --- 1. Linux -------------------------------------------------------------------
L=semillero-$V-x86_64-unknown-linux-gnu
comprobar "linux: empaqueta" sh "$EMP" x86_64-unknown-linux-gnu "$V" "$T/semillero" "$T/web" "$T/salida"
mkdir -p "$T/l"
tar -xzf "$T/salida/$L.tar.gz" -C "$T/l" 2>/dev/null || true
comprobar "linux: una sola carpeta" test "$(ls "$T/l")" = "$L"
comprobar "linux: semillero ejecutable" test -x "$T/l/$L/semillero"
comprobar "linux: site/ con la web" test -f "$T/l/$L/site/pkg/semillero.wasm"
comprobar "linux: site/hash.txt" test -f "$T/l/$L/site/hash.txt"
comprobar "linux: sin .gitkeep" test ! -e "$T/l/$L/site/pkg/.gitkeep"
comprobar "linux: COMMIT" test "$(cat "$T/l/$L/COMMIT" 2>/dev/null)" = "$COMMIT"
comprobar "linux: unidad de systemd" test -f "$T/l/$L/semillero.service"
comprobar "linux: ejemplo de configuración" test -f "$T/l/$L/semillero.env.example"

# --- 2. FreeBSD -----------------------------------------------------------------
# shellcheck source=deploy/lib/precompiladas.sh
. "$RAIZ/deploy/lib/precompiladas.sh"
F=$(archivo_precompilada "$V")
comprobar "freebsd: empaqueta" sh "$EMP" x86_64-unknown-freebsd "$V" "$T/semillero" "$T/web" "$T/salida"
comprobar "freebsd: nombre del buzón ($F)" test -f "$T/salida/$F"
mkdir -p "$T/f"
tar -xzf "$T/salida/$F" -C "$T/f" 2>/dev/null || true
D=$T/f/${F%.tar.gz}
comprobar "freebsd: semillero, site/ y COMMIT" test -x "$D/semillero" -a -d "$D/site" -a -f "$D/COMMIT"
comprobar "freebsd: rc.d/semillero ejecutable" test -x "$D/rc.d/semillero"
comprobar "freebsd: rc.d/semillero es el del repositorio" cmp -s "$RAIZ/packaging/freebsd/semillero" "$D/rc.d/semillero"

# --- 3. Windows -----------------------------------------------------------------
W=semillero-$V-x86_64-pc-windows-msvc
comprobar "windows: empaqueta" sh "$EMP" x86_64-pc-windows-msvc "$V" "$T/semillero" "$T/web" "$T/salida"
mkdir -p "$T/w"
unzip -q "$T/salida/$W.zip" -d "$T/w" 2>/dev/null || true
comprobar "windows: una sola carpeta" test "$(ls "$T/w")" = "$W"
comprobar "windows: semillero.exe" test -f "$T/w/$W/semillero.exe"
comprobar "windows: site/" test -f "$T/w/$W/site/pkg/semillero.wasm"
comprobar "windows: semillero.cmd" test -f "$T/w/$W/semillero.cmd"
crlf()
{
	# Todas las líneas acaban en CR LF.
	[ -s "$1" ] && ! LC_ALL=C grep -q "$(printf '[^\r]$')" "$1"
}
comprobar "windows: semillero.cmd con CRLF" crlf "$T/w/$W/semillero.cmd"
comprobar "windows: ejemplo de configuración con CRLF" crlf "$T/w/$W/semillero.env.example"
comprobar "windows: LEEME-windows.md" test -f "$T/w/$W/LEEME-windows.md"
comprobar "windows: COMMIT" test "$(cat "$T/w/$W/COMMIT" 2>/dev/null)" = "$COMMIT"

# --- 4. Reproducible ------------------------------------------------------------
for f in "$L.tar.gz" "$W.zip"; do
	a=$(sha256sum "$T/salida/$f" 2>/dev/null | cut -d' ' -f1)
	mkdir -p "$T/otra"
	touch "$T/semillero" "$T/web/pkg/semillero.wasm"
	case $f in
	*linux*) sh "$EMP" x86_64-unknown-linux-gnu "$V" "$T/semillero" "$T/web" "$T/otra" >/dev/null 2>&1 || true ;;
	*) sh "$EMP" x86_64-pc-windows-msvc "$V" "$T/semillero" "$T/web" "$T/otra" >/dev/null 2>&1 || true ;;
	esac
	b=$(sha256sum "$T/otra/$f" 2>/dev/null | cut -d' ' -f1)
	comprobar "reproducible: $f" test -n "$a" -a "$a" = "$b"
done

# --- 5. Errores -----------------------------------------------------------------
falla_con()
{
	# falla_con CÓDIGO ARGUMENTOS...: sale con CÓDIGO y no deja nada en mal/.
	_c=$1
	shift
	rm -rf "$T/mal"
	mkdir -p "$T/mal"
	_e=0
	sh "$EMP" "$@" >/dev/null 2>&1 || _e=$?
	[ "$_e" -eq "$_c" ] && [ -z "$(ls -A "$T/mal")" ]
}
comprobar "error: sin argumentos" falla_con 2
comprobar "error: objetivo desconocido" falla_con 1 sparc-sun-solaris "$V" "$T/semillero" "$T/web" "$T/mal"
comprobar "error: versión no válida" falla_con 1 x86_64-unknown-linux-gnu 1.2 "$T/semillero" "$T/web" "$T/mal"
comprobar "error: versión con letras" falla_con 1 x86_64-unknown-linux-gnu 1.2.3-rc1 "$T/semillero" "$T/web" "$T/mal"
comprobar "error: sin binario" falla_con 1 x86_64-unknown-linux-gnu "$V" "$T/no-existe" "$T/web" "$T/mal"
comprobar "error: sin web" falla_con 1 x86_64-unknown-linux-gnu "$V" "$T/semillero" "$T/no-existe" "$T/mal"
COMMIT=abc
comprobar "error: commit corto" falla_con 1 x86_64-unknown-linux-gnu "$V" "$T/semillero" "$T/web" "$T/mal"
COMMIT=0123456789abcdef0123456789abcdef01234567

echo "$n comprobaciones, $fallos fallos"
if [ "$fallos" -eq 0 ] && [ "${SEMILLERO_PRUEBA_CONSERVAR:-}" != 1 ]; then
	rm -rf "$T"
else
	echo "carpeta temporal: $T"
fi
[ "$fallos" -eq 0 ]
