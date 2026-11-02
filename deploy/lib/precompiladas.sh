# shellcheck shell=sh
#
# Versiones de semillero para FreeBSD: las compila el PC de compilación
# (deploy/cross/) y las deja en el buzón del servidor, o las compila el propio
# servidor en su jail de compilación si no llegan a tiempo. Se carga con «.»;
# solo funciones, sin efectos al cargarse. Las usan el actualizador
# (deploy/freebsd/actualizar.sh, en el host como root), el receptor del buzón
# (deploy/freebsd/recibir-precompilada.sh, como el usuario del buzón), los
# guiones del PC (deploy/cross/) y las pruebas (deploy/pruebas/).
#
# El paquete de la versión X.Y.Z (etiqueta vX.Y.Z):
# semillero-X.Y.Z-x86_64-unknown-freebsd.tar.gz, con una única carpeta del
# mismo nombre (sin .tar.gz) que trae semillero (el binario), site/ (la web
# de cargo-leptos), COMMIT (el commit completo de la etiqueta) y LICENSE si
# la hay; y SHA256SUMS, una línea «<sha256>  <archivo>» como la escribe
# sha256sum. SHA256SUMS llega el último: con él, la versión está completa.

PRECOMPILADA_OBJETIVO=x86_64-unknown-freebsd

# ¿Es $1 una versión X.Y.Z? Solo cifras y dos puntos, sin partes vacías y
# como mucho 40 caracteres (es un componente de ruta en el buzón y en
# releases/).
version_valida()
{
	case $1 in
	'' | *[!0-9.]* | *..* | *. | .*) return 1 ;;
	esac
	[ "$(printf '%s' "$1" | tr -cd . | wc -c | tr -d ' ')" -eq 2 ] && [ "${#1}" -le 40 ]
}

# Nombre del paquete de la versión $1.
archivo_precompilada()
{
	echo "semillero-$1-$PRECOMPILADA_OBJETIVO.tar.gz"
}

# Las etiquetas vX.Y.Z de la entrada estándar (una por línea), sin las demás.
solo_versiones()
{
	grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' || true
}

# La más reciente de las etiquetas que llegan por la entrada estándar: la
# última vX.Y.Z según sort -V, la misma que elige traer_ultima_etiqueta
# (deploy/lib/git.sh) en el servidor.
ultima_version()
{
	solo_versiones | sort -V | tail -n 1
}

# La versión (version = "…") del [package] de un Cargo.toml que llega por la
# entrada estándar.
version_cargo()
{
	sed -n '/^\[package\]/,/^\[/s/^version[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1
}

# ¿Es $2 un fichero que se acepta en el buzón para la versión $1?
fichero_de_buzon()
{
	case "$2" in
	"$(archivo_precompilada "$1")" | SHA256SUMS) return 0 ;;
	esac
	return 1
}

# sha256 de $1 en hexadecimal (sha256sum en Linux y en FreeBSD 14+; si no,
# sha256 -q de FreeBSD).
suma_sha256()
{
	if command -v sha256sum >/dev/null 2>&1; then
		sha256sum <"$1" | cut -d' ' -f1
	else
		sha256 -q <"$1"
	fi
}

# ¿Es $1 un ejecutable de FreeBSD x86-64 (según file)?
binario_freebsd()
{
	case $(file -b "$1" 2>/dev/null) in
	ELF\ 64-bit\ LSB\ *executable,\ x86-64,*FreeBSD*) return 0 ;;
	esac
	return 1
}

# ¿Trae la web $1 (target/site de cargo-leptos) hash.txt y los ficheros con
# hash de semillero que nombra (pkg/semillero.<hash>.{js,wasm,css})? Escribe
# lo que falta en la salida de error.
web_completa()
{
	if [ ! -f "$1/hash.txt" ]; then
		echo "falta site/hash.txt" >&2
		return 1
	fi
	for _ext in js wasm css; do
		_h=$(sed -n "s/^$_ext: *//p" "$1/hash.txt" | head -n 1)
		case $_h in
		'' | *[!A-Za-z0-9_-]*)
			echo "site/hash.txt no da el hash de $_ext" >&2
			return 1
			;;
		esac
		if [ ! -f "$1/pkg/semillero.$_h.$_ext" ]; then
			echo "falta site/pkg/semillero.$_h.$_ext" >&2
			return 1
		fi
	done
}

# Qué hacer con la etiqueta $2 según el estado $1 (fichero «etiqueta
# segundos» de cuándo se vio por primera vez sin precompilada), la hora $3
# (segundos), la espera $4 (minutos) y si ya ha llegado al buzón ($5: «si» o
# «no»). Escribe instalar, esperar o compilar. La primera vez que ve una
# etiqueta sin precompilada apunta la hora en $1.
decidir_precompilada()
{
	_estado=$1
	_etiqueta=$2
	_ahora=$3
	_espera=$4
	if [ "$5" = si ]; then
		echo instalar
		return 0
	fi
	case $_espera in
	'' | *[!0-9]*) _espera=0 ;;
	esac
	_vista=
	if [ -f "$_estado" ] && [ "$(cut -d' ' -f1 "$_estado")" = "$_etiqueta" ]; then
		_vista=$(cut -d' ' -f2 "$_estado" | tr -cd '0-9')
	fi
	if [ -z "$_vista" ]; then
		echo "$_etiqueta $_ahora" >"$_estado"
		_vista=$_ahora
	fi
	if [ "$((_ahora - _vista))" -lt "$((_espera * 60))" ]; then
		echo esperar
	else
		echo compilar
	fi
}

# ¿Ha llegado ya al buzón $1 la versión $2 completa? Lo está cuando están el
# paquete y SHA256SUMS (el PC lo sube el último). Solo cuentan los ficheros
# normales, nunca los enlaces.
precompilada_llegada()
{
	for _f in "$(archivo_precompilada "$2")" SHA256SUMS; do
		[ -f "$1/$2/$_f" ] && [ ! -L "$1/$2/$_f" ] || return 1
	done
	return 0
}

# Comprueba la lista del paquete $1 antes de desempaquetarlo: todo bajo $2/
# (se admite un «./» delante), sin rutas absolutas ni «..», y solo ficheros y
# carpetas normales (ni enlaces simbólicos ni duros). $3 es un fichero de
# trabajo.
lista_segura()
{
	tar -tzf "$1" >"$3" 2>/dev/null || return 1
	[ -s "$3" ] || return 1
	while IFS= read -r _n; do
		_n=${_n#./}
		_n=${_n%/}
		case $_n in
		"$2") ;;
		"$2"/*)
			case "/$_n/" in
			*/../* | *//*) return 1 ;;
			esac
			;;
		*) return 1 ;;
		esac
	done <"$3"
	tar -tvzf "$1" >"$3" 2>/dev/null || return 1
	while IFS= read -r _l; do
		case $_l in
		[d-]*) ;;
		*) return 1 ;;
		esac
		case $_l in
		*' -> '* | *' link to '*) return 1 ;;
		esac
	done <"$3"
}

# Comprueba la versión $2 del buzón $1 y la deja desempaquetada en $4 (que no
# debe existir), con semillero, site/ y COMMIT y nada más. $3 es el commit
# completo de la etiqueta en el clon del servidor. Escribe el motivo en la
# salida de error y sale con 1 si algo no cuadra; $4 no queda creada en ese
# caso.
#
# Quien escribe en el buzón es otro usuario: antes de leer nada se copia a
# una carpeta propia (con un tiempo máximo, por si hubiera una FIFO en lugar
# de un fichero) y todo se comprueba sobre la copia. Nada de lo que trae se
# ejecuta.
preparar_precompilada()
{
	_buzon=$1
	_ver=$2
	_commit=$3
	_dst=$4
	_arch=$(archivo_precompilada "$_ver")
	_raiz=${_arch%.tar.gz}
	if ! version_valida "$_ver"; then
		echo "versión no válida" >&2
		return 1
	fi
	case $_commit in
	'' | *[!0-9a-f]*)
		echo "commit no válido" >&2
		return 1
		;;
	esac
	if [ "${#_commit}" -ne 40 ]; then
		echo "commit no válido" >&2
		return 1
	fi
	_trabajo=$(mktemp -d) || return 1
	for _f in "$_arch" SHA256SUMS; do
		if [ ! -f "$_buzon/$_ver/$_f" ] || [ -L "$_buzon/$_ver/$_f" ] ||
			! timeout 300 cp "$_buzon/$_ver/$_f" "$_trabajo/$_f" 2>/dev/null ||
			[ ! -f "$_trabajo/$_f" ] || [ -L "$_trabajo/$_f" ]; then
			echo "no se puede copiar $_f del buzón" >&2
			rm -rf "$_trabajo"
			return 1
		fi
	done
	# SHA256SUMS: exactamente una línea, la del paquete, con 64 cifras
	# hexadecimales en minúscula.
	_re=$(printf '%s' "$_arch" | sed 's/\./\\./g')
	_esperada=$(sed -n "s/^\([0-9a-f]\{64\}\)  $_re\$/\1/p" "$_trabajo/SHA256SUMS")
	if [ "$(wc -l <"$_trabajo/SHA256SUMS" | tr -d ' ')" -ne 1 ] || [ "${#_esperada}" -ne 64 ]; then
		echo "SHA256SUMS no es una sola línea «<sha256>  $_arch»" >&2
		rm -rf "$_trabajo"
		return 1
	fi
	if [ "$(suma_sha256 "$_trabajo/$_arch")" != "$_esperada" ]; then
		echo "el sha256 de $_arch no coincide con SHA256SUMS" >&2
		rm -rf "$_trabajo"
		return 1
	fi
	if ! lista_segura "$_trabajo/$_arch" "$_raiz" "$_trabajo/lista"; then
		echo "el paquete trae algo que no es fichero ni directorio normal, una ruta absoluta o con «..», o algo fuera de $_raiz/" >&2
		rm -rf "$_trabajo"
		return 1
	fi
	mkdir "$_trabajo/x" || return 1
	# Como root, tar conservaría dueños y modos del archivo (setuid incluido).
	if ! tar -xzf "$_trabajo/$_arch" -C "$_trabajo/x" --no-same-owner --no-same-permissions 2>/dev/null; then
		echo "no se puede desempaquetar $_arch" >&2
		rm -rf "$_trabajo"
		return 1
	fi
	# Lo mismo, ya en disco.
	if [ -n "$(find "$_trabajo/x" ! -type f ! -type d)" ] ||
		[ -n "$(find "$_trabajo/x" -type f -links +1)" ]; then
		echo "el paquete trae algo que no es fichero ni directorio, o enlaces duros" >&2
		rm -rf "$_trabajo"
		return 1
	fi
	if [ "$(find "$_trabajo/x" -mindepth 1 -maxdepth 1 | wc -l)" -ne 1 ] ||
		[ ! -d "$_trabajo/x/$_raiz" ]; then
		echo "el paquete no trae solo la carpeta $_raiz" >&2
		rm -rf "$_trabajo"
		return 1
	fi
	_d=$_trabajo/x/$_raiz
	if [ ! -f "$_d/semillero" ] || [ ! -d "$_d/site" ] || [ ! -f "$_d/COMMIT" ]; then
		echo "al paquete le faltan semillero, site/ o COMMIT" >&2
		rm -rf "$_trabajo"
		return 1
	fi
	if [ "$(head -n 1 "$_d/COMMIT")" != "$_commit" ]; then
		echo "el paquete es de otro commit ($(head -n 1 "$_d/COMMIT" | tr -cd '0-9a-f' | cut -c1-12)), no del de v$_ver" >&2
		rm -rf "$_trabajo"
		return 1
	fi
	if ! binario_freebsd "$_d/semillero"; then
		echo "semillero no es un ejecutable de FreeBSD x86-64 ($(file -b "$_d/semillero" 2>/dev/null | cut -c1-60))" >&2
		rm -rf "$_trabajo"
		return 1
	fi
	if ! mkdir "$_dst" || ! mv "$_d/semillero" "$_d/site" "$_d/COMMIT" "$_dst/"; then
		echo "no se puede preparar $_dst" >&2
		rm -rf "$_trabajo" "$_dst"
		return 1
	fi
	rm -rf "$_trabajo"
}
