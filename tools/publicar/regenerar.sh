#!/bin/sh
# Regenera la copia pública del repositorio privado y comprueba que no lleva
# nada privado. Nunca toca el repositorio de trabajo ni sube nada: deja el
# resultado en una carpeta nueva y escribe las órdenes «git push» que hay que
# lanzar a mano.
#
# Pasos:
# 1. git clone --no-local de la rama principal del repositorio privado en una
#    carpeta temporal nueva; se quedan solo esa rama y las etiquetas v* que
#    cuelgan de ella (las demás etiquetas y los remotos se borran).
# 2. git filter-repo sobre el clon:
#    - quita las rutas privadas (--invert-paths): odd/, docs/superpowers/,
#      .atl/, .superpowers/, target/ y lo que diga rutas-privadas.txt;
#    - cambia los valores reales por los inventados de la lista privada
#      (--replace-text en los ficheros y --replace-message en los mensajes de
#      commits y etiquetas), y quita las líneas «cherry picked from»;
#    - pone como autor, committer y firmante de etiquetas la identidad pública
#      (un correo …@users.noreply.github.com).
# 3. Comprueba el resultado: solo la rama y las etiquetas decididas, ninguna
#    ruta privada y escanear.sh --historia --privado con cero hallazgos (y el
#    árbol también). Si algo falla, sale con error y deja la carpeta para
#    revisarla.
#
# Modo aplanado (--plano VERSIÓN): en lugar del historial reescrito, la
# copia lleva un solo commit nuevo con el árbol de la rama, el mensaje
# «semillero VERSIÓN», la fecha de hoy a las 00:00 UTC y la etiqueta VERSIÓN.
# Así no sale nada de los commits privados: ni ficheros viejos, ni mensajes,
# ni horas ni zona horaria. Con --sobre REPO (el repositorio público, o un
# clon suyo) el commit cuelga del último de su rama, para que la subida sea
# un avance normal; si la versión ya está ahí o el árbol no cambia, no se
# hace nada.
#
# Excepciones: las sustituciones son ciegas, así que un texto que debe salir
# tal cual aunque lleve un valor de la lista (como la dirección del propio
# repositorio público) va en el fichero de excepciones, uno por línea. Ese
# texto se protege antes de sustituir, se devuelve al final y el escáner no
# lo cuenta como hallazgo. Solo vale para el contenido y los mensajes, no
# para los nombres de fichero.
#
# Uso: regenerar.sh [--origen REPO] [--rama RAMA] [--privado FICHERO]
#                   [--excepciones FICHERO] [--nombre NOMBRE] [--correo CORREO]
#                   [--destino CARPETA] [--plano VERSIÓN [--sobre REPO]]
#   --origen       repositorio privado (por defecto, el de este guion)
#   --rama         rama que se publica (main)
#   --privado      lista de valores (~/.config/semillero-publicacion/privado.txt
#                  o $SEMILLERO_PRIVADO)
#   --excepciones  textos que no se tocan (~/.config/semillero-publicacion/
#                  excepciones.txt si existe, o $SEMILLERO_EXCEPCIONES)
#   --nombre       autor público ($SEMILLERO_PUBLICO_NOMBRE o
#                  git config semillero.publicoNombre)
#   --correo       correo público, …@users.noreply.github.com
#                  ($SEMILLERO_PUBLICO_CORREO o git config semillero.publicoCorreo)
#   --destino      carpeta nueva para el resultado (por defecto, una temporal)
#   --plano        un solo commit con la etiqueta VERSIÓN (vX.Y.Z), ver arriba
#   --sobre        repositorio público del que cuelga el commit aplanado
# La dirección del repositorio público para las órdenes de push sale de
# git config semillero.publico del repositorio privado.
# Sale con 0 si la copia está limpia, 1 si se rechaza y 2 si no se puede hacer.

set -eu

HERRAMIENTA=$(cd "$(dirname "$0")" && pwd)
CONFIG=$HOME/.config/semillero-publicacion

uso()
{
	sed -n '/^# Uso:/,/^# Sale/p' "$0" | sed 's/^# \{0,1\}//' >&2
	exit 2
}

error()
{
	echo "regenerar: $*" >&2
	exit 2
}

origen=
rama=main
privado=${SEMILLERO_PRIVADO:-$CONFIG/privado.txt}
excepciones=${SEMILLERO_EXCEPCIONES:-}
nombre=${SEMILLERO_PUBLICO_NOMBRE:-}
correo=${SEMILLERO_PUBLICO_CORREO:-}
destino=
plano=
sobre=
while [ $# -gt 0 ]; do
	[ $# -ge 2 ] || uso
	case $1 in
	--origen) origen=$2 ;;
	--rama) rama=$2 ;;
	--privado) privado=$2 ;;
	--excepciones) excepciones=$2 ;;
	--nombre) nombre=$2 ;;
	--correo) correo=$2 ;;
	--destino) destino=$2 ;;
	--plano) plano=$2 ;;
	--sobre) sobre=$2 ;;
	*) uso ;;
	esac
	shift 2
done

git filter-repo --version >/dev/null 2>&1 ||
	error "hace falta git filter-repo (https://github.com/newren/git-filter-repo)"
if [ -z "$origen" ]; then
	origen=$(git -C "$HERRAMIENTA" rev-parse --show-toplevel) ||
		error "no encuentro el repositorio privado; usa --origen"
fi
git -C "$origen" rev-parse --verify --quiet "refs/heads/$rama^{commit}" >/dev/null ||
	error "el origen no tiene la rama $rama"
[ -r "$privado" ] || error "no se puede leer la lista privada $privado"
if [ -z "$excepciones" ] && [ -f "$CONFIG/excepciones.txt" ]; then
	excepciones=$CONFIG/excepciones.txt
fi
[ -z "$excepciones" ] || [ -r "$excepciones" ] ||
	error "no se puede leer el fichero de excepciones $excepciones"
[ -n "$nombre" ] || nombre=$(git -C "$origen" config --get semillero.publicoNombre || true)
[ -n "$correo" ] || correo=$(git -C "$origen" config --get semillero.publicoCorreo || true)
# Van dentro de código Python (los callbacks de filter-repo): solo caracteres
# que no necesitan escaparse.
printf '%s\n' "$nombre" | grep -Eqx '[A-Za-z0-9][A-Za-z0-9 ._-]{0,63}' ||
	error "falta el nombre público (--nombre) o lleva caracteres raros"
printf '%s\n' "$correo" | grep -Eqx '[A-Za-z0-9._+-]+@users\.noreply\.github\.com' ||
	error "el correo público (--correo) tiene que ser …@users.noreply.github.com"
if [ -n "$excepciones" ] && grep -q '==>' "$excepciones"; then
	error "una excepción no puede llevar «==>»"
fi
publico=$(git -C "$origen" config --get semillero.publico || true)
if [ -n "$plano" ]; then
	printf '%s\n' "$plano" | grep -Eqx 'v[0-9]+\.[0-9]+\.[0-9]+' ||
		error "la versión de --plano tiene que ser vX.Y.Z"
elif [ -n "$sobre" ]; then
	error "--sobre solo vale con --plano"
fi
if [ -n "$sobre" ]; then
	git ls-remote --exit-code --heads "$sobre" "$rama" >/dev/null 2>&1 ||
		error "no encuentro la rama $rama en $sobre"
	[ -z "$(git ls-remote --tags "$sobre" "refs/tags/$plano")" ] ||
		error "$plano ya está publicada en $sobre"
fi

if [ -n "$destino" ]; then
	[ ! -e "$destino" ] || error "$destino ya existe; elige una carpeta nueva"
	mkdir -p "$destino"
	dir=$(cd "$destino" && pwd)
else
	dir=$(mktemp -d "${TMPDIR:-/tmp}/semillero-publico.XXXXXX")
fi
repo=$dir/repo

rechazar()
{
	echo "regenerar: RECHAZADO: $*" >&2
	echo "regenerar: queda todo en $dir para revisarlo" >&2
	exit 1
}

# 1. Clon nuevo: solo la rama y las etiquetas v* que cuelgan de ella.
git clone --quiet --no-local --single-branch --branch "$rama" "$origen" "$repo"
git -C "$repo" remote remove origin
for t in $(git -C "$repo" tag -l); do
	case $t in
	v*) git -C "$repo" merge-base --is-ancestor "refs/tags/$t^{commit}" "refs/heads/$rama" && continue ;;
	esac
	git -C "$repo" tag -d "$t" >/dev/null
done

# Aplanado: la versión tiene que ser la del paquete (la CI de versiones
# rechaza una etiqueta que no coincide).
if [ -n "$plano" ] && [ -f "$repo/crates/semillero/Cargo.toml" ]; then
	paquete=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$repo/crates/semillero/Cargo.toml" | head -n 1)
	[ "v$paquete" = "$plano" ] ||
		error "$plano no es la versión de crates/semillero/Cargo.toml ($paquete)"
fi

# Aplanado: un commit sin padres con el árbol de la rama, y solo él.
# Identidad y fecha públicas (filter-repo vuelve a poner la identidad).
if [ -n "$plano" ]; then
	FECHA="$(date -u +%Y-%m-%d)T00:00:00+0000"
	export GIT_AUTHOR_NAME="$nombre" GIT_AUTHOR_EMAIL="$correo" GIT_AUTHOR_DATE="$FECHA"
	export GIT_COMMITTER_NAME="$nombre" GIT_COMMITTER_EMAIL="$correo" GIT_COMMITTER_DATE="$FECHA"
	c=$(git -C "$repo" commit-tree -m "semillero $plano" "refs/heads/$rama^{tree}")
	git -C "$repo" update-ref "refs/heads/$rama" "$c"
	for t in $(git -C "$repo" tag -l); do git -C "$repo" tag -d "$t" >/dev/null; done
	git -C "$repo" reflog expire --expire=now --all
	git -C "$repo" gc --quiet --prune=now
fi

# Los ficheros de sustituciones copian la lista privada: no se dejan en la
# carpeta del resultado.
borrar_copias()
{
	rm -f "$dir/proteger.txt" "$dir/devolver.txt" "$dir/lista.txt" \
		"$dir/texto.txt" "$dir/mensajes.txt"
}
trap borrar_copias EXIT

# 2. Ficheros para filter-repo.
# Rutas privadas (siempre estas, más las de rutas-privadas.txt).
{
	printf '%s\n' odd/ docs/superpowers/ .atl/ .superpowers/ target/
	if [ -f "$HERRAMIENTA/rutas-privadas.txt" ]; then
		sed -e 's/[[:space:]]*$//' -e '/^#/d' -e '/^$/d' "$HERRAMIENTA/rutas-privadas.txt"
	fi
} | sort -u >"$dir/rutas.txt"

# Sustituciones: filter-repo aplica primero todos los literales, en orden, y
# después todas las expresiones. Las excepciones se cambian primero por una
# marca (literal) y se devuelven las últimas (expresión), así ninguna regla
# de la lista privada llega a verlas. La marca solo lleva caracteres de
# control (\001, \002 tantas veces como el número de excepción y \003), que
# ninguna regla razonable toca. filter-repo no admite comentarios en este
# fichero: se quitan.
marca()
{
	awk -v n="$1" 'BEGIN { s = sprintf("%c", 1); for (i = 0; i < n; i++) s = s sprintf("%c", 2); printf "%s%c", s, 3 }'
}
: >"$dir/proteger.txt"
: >"$dir/devolver.txt"
if [ -n "$excepciones" ]; then
	i=0
	while IFS= read -r e || [ -n "$e" ]; do
		e=$(printf '%s' "$e" | tr -d '\r')
		case $e in '' | '#'*) continue ;; esac
		i=$((i + 1))
		m=$(marca "$i")
		printf 'literal:%s==>%s\n' "$e" "$m" >>"$dir/proteger.txt"
		# En el reemplazo de una expresión, «\» es especial: se dobla.
		printf 'regex:%s==>%s\n' "$m" "$(printf '%s' "$e" | sed 's/\\/\\\\/g')" >>"$dir/devolver.txt"
	done <"$excepciones"
fi
sed -e 's/\r$//' -e '/^#/d' -e '/^[[:space:]]*$/d' "$privado" >"$dir/lista.txt"
cat "$dir/proteger.txt" "$dir/lista.txt" "$dir/devolver.txt" >"$dir/texto.txt"
{
	cat "$dir/proteger.txt" "$dir/lista.txt"
	printf '%s\n' 'regex:(?m)^\(cherry picked from commit [0-9a-f]+\)\n?==>'
	cat "$dir/devolver.txt"
} >"$dir/mensajes.txt"

# 3. Reescritura. --force: el clon es nuevo y solo nuestro (filter-repo no
# lo da por «recién clonado» después de borrar las etiquetas y el remoto).
if ! (cd "$repo" && git filter-repo --force --replace-refs delete-no-add \
	--invert-paths --paths-from-file "$dir/rutas.txt" \
	--replace-text "$dir/texto.txt" \
	--replace-message "$dir/mensajes.txt" \
	--name-callback "return b\"$nombre\"" \
	--email-callback "return b\"$correo\"") >"$dir/filter-repo.log" 2>&1; then
	borrar_copias
	rechazar "git filter-repo falló (mira $dir/filter-repo.log)"
fi
borrar_copias

# Aplanado: cuelga del público (si se da) y lleva su etiqueta.
if [ -n "$plano" ]; then
	arbol=$(git -C "$repo" rev-parse "refs/heads/$rama^{tree}")
	if [ -n "$sobre" ]; then
		git -C "$repo" fetch --quiet --no-tags "$sobre" "refs/heads/$rama" ||
			error "no se puede traer $rama de $sobre"
		anterior=$(git -C "$repo" rev-parse FETCH_HEAD)
		[ "$(git -C "$repo" rev-parse "$anterior^{tree}")" != "$arbol" ] ||
			error "nada nuevo que publicar: el árbol es el mismo que en $sobre"
		c=$(git -C "$repo" commit-tree -p "$anterior" -m "semillero $plano" "$arbol")
		git -C "$repo" update-ref "refs/heads/$rama" "$c"
	fi
	git -C "$repo" tag "$plano" "refs/heads/$rama"
fi

# 4. Comprobaciones.
otras=$(git -C "$repo" for-each-ref --format='%(refname)' |
	grep -v -e "^refs/heads/$rama\$" -e '^refs/tags/v' || true)
[ -z "$otras" ] || rechazar "quedan referencias que no se publican: $otras"

git -C "$repo" rev-list --objects --all | sed -n 's/^[0-9a-f]* //p' | sort -u >"$dir/nombres.txt"
while IFS= read -r r; do
	case $r in
	regex:* | glob:*) continue ;; # filter-repo ya las aplica; se ven en el escáner
	*/) awk -v p="$r" 'index($0, p) == 1 { e = 1 } END { exit !e }' "$dir/nombres.txt" &&
		rechazar "quedan ficheros bajo la ruta privada $r" ;;
	*) awk -v p="$r" '$0 == p || index($0, p "/") == 1 { e = 1 } END { exit !e }' "$dir/nombres.txt" &&
		rechazar "queda la ruta privada $r" ;;
	esac
done <"$dir/rutas.txt"

permitido=$HERRAMIENTA/permitido.txt
[ ! -f "$repo/tools/publicar/permitido.txt" ] || permitido=$repo/tools/publicar/permitido.txt
set -- --privado "$privado" --permitido "$permitido"
[ -z "$excepciones" ] || set -- "$@" --excepciones "$excepciones"
estado=0
sh "$HERRAMIENTA/escanear.sh" --historia "$@" "$repo" >"$dir/hallazgos-historia.txt" || estado=$?
sh "$HERRAMIENTA/escanear.sh" "$@" "$repo" >"$dir/hallazgos-arbol.txt" || estado=$((estado + $?))
if [ "$estado" -ne 0 ]; then
	rechazar "el escáner encontró algo: $dir/hallazgos-historia.txt y $dir/hallazgos-arbol.txt"
fi

# 5. Gancho pre-push también en la copia, por si se sube desde aquí.
git -C "$repo" config semillero.privado "$privado"
[ -z "$excepciones" ] || git -C "$repo" config semillero.excepciones "$excepciones"
[ -z "$publico" ] || git -C "$repo" config semillero.publico "$publico"
sh "$HERRAMIENTA/instalar-gancho.sh" "$repo" >/dev/null

url=${publico:-URL-DEL-REPOSITORIO-PUBLICO}
echo "regenerar: copia pública limpia en $repo"
echo "Para publicarla (la rama primero y cada etiqueta sola, para que GitHub"
echo "lance el flujo de cada versión):"
echo "  git -C '$repo' push '$url' 'refs/heads/$rama:refs/heads/$rama'"
for t in $(git -C "$repo" tag -l 'v*' | sort -V); do
	echo "  git -C '$repo' push '$url' 'refs/tags/$t'"
done
