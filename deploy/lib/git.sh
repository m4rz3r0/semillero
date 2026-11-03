# shellcheck shell=sh
#
# Última etiqueta del repositorio, sin clon: se lee la salida de
# «git ls-remote --tags» (la lista que da el servidor git), que no trae
# nada ni ejecuta nada del repositorio. Se carga con «.»; solo usa grep, sed
# y sort. La usa el actualizador en el host (deploy/freebsd/actualizar.sh).
#
# Solo cuentan las etiquetas vX.Y.Z; la elegida es la más alta según sort -V,
# la misma que ultima_version (deploy/lib/precompiladas.sh) en el PC.

_etiquetas_version()
{
	grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sort -V | tail -n 1
}

# Lee por la entrada estándar la salida de git ls-remote --tags y escribe
# «vX.Y.Z <commit>» de la última etiqueta vX.Y.Z. El commit es el de la línea
# «refs/tags/vX.Y.Z^{}» si la hay (etiqueta anotada: la otra línea es el
# objeto etiqueta, no el commit) y si no, el de «refs/tags/vX.Y.Z» (etiqueta
# ligera). Sin ninguna etiqueta vX.Y.Z no escribe nada. Las líneas que no
# son «<40 cifras hex> refs/tags/…» no cuentan.
ultima_etiqueta_ls_remote()
{
	_lista=$(sed -n 's/^\([0-9a-f]\{40\}\)[[:space:]]\{1,\}refs\/tags\/\([^[:space:]]*\)$/\1 \2/p')
	_etiqueta=$(printf '%s\n' "$_lista" | sed -n 's/^[0-9a-f]* //p' | _etiquetas_version)
	[ -n "$_etiqueta" ] || return 0
	# Los puntos de la etiqueta, literales en las expresiones de sed.
	_re=$(printf '%s' "$_etiqueta" | sed 's/\./\\./g')
	_commit=$(printf '%s\n' "$_lista" | sed -n "s/^\([0-9a-f]*\) $_re^{}\$/\1/p" | head -n 1)
	if [ -z "$_commit" ]; then
		_commit=$(printf '%s\n' "$_lista" | sed -n "s/^\([0-9a-f]*\) $_re\$/\1/p" | head -n 1)
	fi
	echo "$_etiqueta $_commit"
}

# ¿Se llega al repositorio $1 por ssh? ssh://…, git+ssh://…, ssh+git://… o
# [usuario@]equipo:ruta (la forma corta, como scp: dos puntos antes de la
# primera barra). No lo son https://…, file://… ni una ruta local.
repo_por_ssh()
{
	case $1 in
	ssh://* | git+ssh://* | ssh+git://*) return 0 ;;
	*://* | /* | ./* | ../*) return 1 ;;
	esac
	_antes=${1%%:*}
	[ "$_antes" != "$1" ] || return 1
	case $_antes in
	'' | */*) return 1 ;;
	esac
}
