# shellcheck shell=sh
#
# Clon del repositorio que sigue al remoto por etiquetas. Se carga con «.»;
# solo usa git, grep y sort, así que el actualizador lo manda tal cual, como
# texto, a la jail de compilación (el host nunca ejecuta nada del clon).
#
# Nunca hace merge: el clon se pone en la etiqueta con checkout --detach, así
# que da igual que el remoto haya reescrito su historial (etiquetas borradas
# o movidas).
#
# Solo cuentan las etiquetas vX.Y.Z; la elegida es la más alta según sort -V,
# la misma que ultima_version (deploy/lib/precompiladas.sh) en el PC.

_etiquetas_version()
{
	grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sort -V | tail -n 1
}

# Trae ramas y etiquetas de origin al clon $1: --force acepta etiquetas
# movidas y --prune/--prune-tags borran las que el remoto ya no tiene.
traer_referencias()
{
	git -C "$1" fetch --quiet --force --prune --prune-tags --tags origin
}

# Trae las referencias, pone el clon $1 en la última etiqueta vX.Y.Z y la
# escribe en la salida estándar. Sin ninguna no escribe nada ni cambia el
# clon. Sale con error si no se puede traer o sacar.
traer_ultima_etiqueta()
{
	traer_referencias "$1" || return 1
	_etiqueta=$(git -C "$1" tag -l 'v*' | _etiquetas_version)
	if [ -z "$_etiqueta" ]; then
		return 0
	fi
	git -C "$1" -c advice.detachedHead=false checkout --quiet --force --detach \
		"refs/tags/$_etiqueta" || return 1
	echo "$_etiqueta"
}

# La última etiqueta vX.Y.Z del remoto $1 sin traer nada ni tocar el clon
# (para --comprobar).
ultima_etiqueta_remota()
{
	_lista=$(git ls-remote --tags --refs "$1" 'v*') || return 1
	printf '%s\n' "$_lista" | sed -n 's|^[0-9a-f]*[[:space:]]*refs/tags/||p' | _etiquetas_version
}
