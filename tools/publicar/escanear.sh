#!/bin/sh
# Escáner de datos que no deben salir en el repositorio público.
#
# Dos modos:
# - árbol (por defecto): los ficheros de una copia de trabajo (los que git
#   sigue más los nuevos no ignorados; fuera de git, todos los ficheros) y
#   sus nombres;
# - --historia: todos los blobs de todos los commits y todas las etiquetas
#   (o de lo que digan --revs), los commits en crudo (mensaje, autor,
#   committer y sus correos), las etiquetas anotadas (mensaje y quien las
#   firma), los nombres de fichero y los nombres de las referencias.
#
# Reglas genéricas (no necesitan nada privado; las usa la CI):
#   ipv4-privada ipv4-publica ipv6 mac correo clave-privada clave-ssh token
#   ruta-personal dominio binario metadatos-png metadatos-jpeg
# Quedan fuera los rangos de documentación (192.0.2.0/24, 198.51.100.0/24,
# 203.0.113.0/24, 2001:db8::/32, *.example, *.home.arpa, localhost, MAC
# 00:00:5e:00:53:xx) y lo que diga la lista de permitidos (ver LEEME.md).
#
# Con --privado FICHERO, además, cada entrada de esa lista (el lado izquierdo
# de un fichero de git filter-repo --replace-text: «literal==>x»,
# «regex:expr==>x», «glob:patrón==>x»; las líneas con # son comentarios) es
# un hallazgo «privado» allí donde aparezca, también dentro de binarios.
# --excepciones FICHERO lista textos (uno por línea) que se dejan pasar
# aunque contengan un valor privado, como la dirección del repositorio
# público.
#
# Nunca escribe el valor encontrado: solo dónde, la regla y una muestra
# tapada (los dos primeros caracteres y la longitud; de la lista privada,
# solo el número de línea de la entrada).
#
# Uso: escanear.sh [--historia] [--revs 'REV…'] [--privado FICHERO]
#                  [--excepciones FICHERO] [--permitido FICHERO] [CARPETA]
# Sale con 0 sin hallazgos, 1 con hallazgos y 2 si no puede escanear.

set -euf
LC_ALL=C
export LC_ALL

HERRAMIENTA=$(cd "$(dirname "$0")" && pwd)

uso()
{
	sed -n '/^# Uso:/,/^# Sale/p' "$0" | sed 's/^# \{0,1\}//' >&2
	exit 2
}

error()
{
	echo "escanear: $*" >&2
	exit 2
}

modo=arbol
revs=--all
privado=
excepciones=
permitido=$HERRAMIENTA/permitido.txt
carpeta=.
while [ $# -gt 0 ]; do
	case $1 in
	--historia) modo=historia ;;
	--revs)
		[ $# -ge 2 ] || uso
		revs=$2
		shift
		;;
	--privado)
		[ $# -ge 2 ] || uso
		privado=$2
		shift
		;;
	--excepciones)
		[ $# -ge 2 ] || uso
		excepciones=$2
		shift
		;;
	--permitido)
		[ $# -ge 2 ] || uso
		permitido=$2
		shift
		;;
	-h | --ayuda) uso ;;
	-*) uso ;;
	*)
		[ $# -eq 1 ] || uso
		carpeta=$1
		;;
	esac
	shift
done

[ -d "$carpeta" ] || error "no existe la carpeta $carpeta"
for f in "$privado" "$excepciones" "$permitido"; do
	[ -z "$f" ] || [ -r "$f" ] || error "no se puede leer $f"
done
command -v git >/dev/null 2>&1 || error "hace falta git"

tmp=$(mktemp -d "${TMPDIR:-/tmp}/escanear.XXXXXX")
trap 'rm -rf "$tmp"' EXIT
trap 'exit 2' INT TERM HUP
mkdir "$tmp/u"
indice=$tmp/indice
binarios=$tmp/binarios
rutas=$tmp/rutas
: >"$indice"
: >"$binarios"
: >"$rutas"
n=0
TAB=$(printf '\t')

# Una unidad que revisa el motor: modo (texto, privado, rutas o binarios),
# fichero y etiqueta (por dónde se informa).
unidad()
{
	printf '%s\t%s\t%s\n' "$1" "$2" "$3" >>"$indice"
}

# Patrones de los binarios permitidos (tipo «binario» de la lista).
globs_binarios=
if [ -n "$permitido" ]; then
	globs_binarios=$(awk '$1 == "binario" { print $2 }' "$permitido")
fi

binario_permitido()
{
	for g in $globs_binarios; do
		# shellcheck disable=SC2254 # el patrón es un glob a propósito
		case $1 in
		$g) return 0 ;;
		esac
	done
	return 1
}

# Imprime un byte por línea en hexadecimal.
bytes()
{
	od -An -v -tx1 "$1" | tr -s ' \n' '\n\n' | sed '/^$/d'
}

# Trozos de texto de un PNG (tEXt, iTXt, zTXt, eXIf, tIME).
metadatos_png()
{
	bytes "$1" | awk '
	BEGIN { for (i = 0; i < 256; i++) h[sprintf("%02x", i)] = i }
	NR <= 8 { next }
	st == 0 { len = len * 256 + h[$1]; if (++k == 4) { st = 1; k = 0; t = "" } next }
	st == 1 {
		t = t sprintf("%c", h[$1])
		if (++k == 4) {
			k = 0
			if (t ~ /^(tEXt|iTXt|zTXt|eXIf|tIME)$/) print t
			if (t == "IEND") exit
			resto = len + 4; st = 2
		}
		next
	}
	st == 2 { if (--resto == 0) { st = 0; len = 0 } }'
}

# Segmentos de metadatos de un JPEG (APP1 Exif/XMP, APP13 IPTC, COM).
metadatos_jpeg()
{
	bytes "$1" | awk '
	BEGIN { for (i = 0; i < 256; i++) h[sprintf("%02x", i)] = i }
	NR <= 2 { next }
	st == 0 { if ($1 == "ff") st = 1; next }
	st == 1 {
		m = $1
		if (m == "ff") next
		if (m == "da" || m == "d9") exit
		if ((m >= "d0" && m <= "d8") || m == "01") { st = 0; next }
		if (m == "e1") print "APP1"
		else if (m == "ed") print "APP13"
		else if (m == "fe") print "COM"
		st = 2; k = 0; len = 0
		next
	}
	st == 2 { len = len * 256 + h[$1]; if (++k == 2) { resto = len - 2; st = (resto > 0) ? 3 : 0 } next }
	st == 3 { if (--resto == 0) st = 0 }'
}

# Revisa un fichero binario: tipo, metadatos y, con lista privada, el texto
# que lleva dentro. $1 fichero, $2 etiqueta, $3 ruta (para los permitidos).
binario()
{
	magia=$(od -An -tx1 -N12 "$1" | tr -d ' \n')
	case $magia in
	89504e47*) tipo=png ;;
	ffd8ff*) tipo=jpeg ;;
	25504446*) tipo=pdf ;;
	47494638*) tipo=gif ;;
	52494646????????57454250*) tipo=webp ;;
	504b0304*) tipo=zip ;;
	1f8b*) tipo=gzip ;;
	7f454c46*) tipo=elf ;;
	774f4632*) tipo=woff2 ;;
	774f4646*) tipo=woff ;;
	53514c69*) tipo=sqlite ;;
	00000100*) tipo=ico ;;
	*) tipo=desconocido ;;
	esac
	if ! binario_permitido "$3"; then
		printf '%s\tbinario\t%s\n' "$2" "$tipo" >>"$binarios"
	fi
	case $tipo in
	png)
		metadatos_png "$1" | sort -u | while read -r t; do
			printf '%s\tmetadatos-png\t%s\n' "$2" "$t" >>"$binarios"
		done
		;;
	jpeg)
		metadatos_jpeg "$1" | sort -u | while read -r t; do
			printf '%s\tmetadatos-jpeg\t%s\n' "$2" "$t" >>"$binarios"
		done
		;;
	esac
	if [ -n "$privado" ]; then
		n=$((n + 1))
		tr -c '[:print:]' '\n' <"$1" | awk 'length($0) >= 4' >"$tmp/u/$n"
		unidad privado "$tmp/u/$n" "$2 (texto dentro del binario)"
	fi
}

# Un fichero cualquiera: vacío no se mira, texto al motor, binario aparte.
fichero()
{
	[ -s "$1" ] || return 0
	if grep -qI '' "$1"; then
		unidad texto "$1" "$2"
	else
		binario "$1" "$2" "$3"
	fi
}

if [ "$modo" = arbol ]; then
	cd "$carpeta"
	if git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
		git ls-files -z --cached --others --exclude-standard | tr '\0' '\n'
	else
		find . -type d -name .git -prune -o -type f -print | sed 's#^\./##'
	fi | sort -u >"$tmp/lista"
	while IFS= read -r f; do
		[ -n "$f" ] || continue
		printf '%s\n' "$f" >>"$rutas"
		if [ -L "$f" ]; then
			# El destino de un enlace también es texto que sale.
			readlink "$f" >>"$rutas" || true
		elif [ -f "$f" ]; then
			fichero "$f" "$f" "$f"
		fi
	done <"$tmp/lista"
else
	git -C "$carpeta" rev-parse --git-dir >/dev/null 2>&1 ||
		error "$carpeta no es un repositorio git"
	cd "$carpeta"
	# shellcheck disable=SC2086 # --revs es una lista de revisiones a propósito
	git rev-list --objects $revs >"$tmp/objetos" ||
		error "git rev-list no pudo recorrer $revs"
	cut -d' ' -f1 "$tmp/objetos" |
		git cat-file --batch-check='%(objectname) %(objecttype)' >"$tmp/tipos"
	# sha, tipo y ruta (la primera con la que aparece) de cada objeto.
	awk 'NR == FNR { tipo[$1] = $2; next }
	{
		sha = $1; ruta = $0; sub(/^[0-9a-f]+ ?/, "", ruta)
		if (!(sha in visto)) { visto[sha] = 1; printf "%s\t%s\t%s\n", sha, tipo[sha], ruta }
	}' "$tmp/tipos" "$tmp/objetos" >"$tmp/todo"
	cut -f3 "$tmp/todo" | sed '/^$/d' | sort -u >"$rutas"
	if [ "$revs" = --all ]; then
		git for-each-ref --format='%(refname)' >"$tmp/refs"
		unidad texto "$tmp/refs" "referencias"
	fi
	while IFS="$TAB" read -r sha tipo ruta; do
		corto=$(printf '%.10s' "$sha")
		case $tipo in
		blob)
			n=$((n + 1))
			git cat-file blob "$sha" >"$tmp/u/$n"
			fichero "$tmp/u/$n" "$ruta (blob $corto)" "$ruta"
			;;
		commit)
			n=$((n + 1))
			git cat-file commit "$sha" >"$tmp/u/$n"
			unidad texto "$tmp/u/$n" "commit $corto"
			;;
		tag)
			n=$((n + 1))
			git cat-file tag "$sha" >"$tmp/u/$n"
			unidad texto "$tmp/u/$n" "etiqueta $corto"
			;;
		esac
	done <"$tmp/todo"
fi
unidad rutas "$rutas" "nombre de fichero"
unidad binarios "$binarios" ""

set +e
awk -v indice="$indice" -v privado="$privado" -v excepciones="$excepciones" \
	-v permitido="$permitido" -f "$HERRAMIENTA/reglas.awk"
estado=$?
set -e
case $estado in
0 | 1) exit "$estado" ;;
*) error "el motor de reglas falló (salida $estado)" ;;
esac
