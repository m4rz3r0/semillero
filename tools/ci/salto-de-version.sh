#!/bin/sh
# Prueba del salto de versión: una instalación de la versión anterior pasa a
# la nueva sin tocar nada. Arranca la versión publicada anterior como la deja
# la guía (docs/instalar/linux-systemd.md: el paquete en una carpeta, la base
# de datos y la clave de Jellyfin en la carpeta de trabajo, la configuración
# en un fichero de variables), le mete datos, la para, cambia el binario por
# el nuevo con el mismo entorno y los mismos ficheros y comprueba que arranca
# (/salud), que las migraciones se aplicaron y que siguen todas las filas.
#
# Existe porque la 0.3.0 cambió sin migración dónde se busca por defecto la
# clave de Jellyfin (SEMILLERO_JELLYFIN_CLAVE sin poner) y una instalación
# de la 0.2 dejó de arrancar sin que lo viera ninguna prueba. Aquí la clave
# está donde la busca por defecto la versión ANTIGUA y SEMILLERO_JELLYFIN_CLAVE
# no se pone: si la nueva la busca en otro sitio sin mirar el de antes, no
# arranca y la prueba falla. Si la encuentra en la ruta de antes avisa en el
# registro; eso vale (es lo que hace la 0.4.1 con la ruta de la 0.2).
#
# No hace falta qBittorrent ni Jellyfin: los dos apuntan a puertos de
# 127.0.0.1 donde no escucha nadie. Las filas se meten con sqlite3, porque la
# API pide entrar y entrar pide qBittorrent.
#
# Uso: tools/ci/salto-de-version.sh [ANTIGUA [BINARIO]]
#
# ANTIGUA  vacía o «-»: la release de GitHub más alta de $GITHUB_REPOSITORY
#          (por defecto m4rz3r0/semillero) con versión menor que la de
#          crates/semillero/Cargo.toml (así, repetida tras etiquetar, sigue
#          comparando con la anterior). X.Y.Z o vX.Y.Z: esa release. Una ruta:
#          un paquete ya descargado semillero-X.Y.Z-x86_64-unknown-linux-gnu
#          (.tar.gz, o la carpeta ya desempaquetada); si hay un SHA256SUMS
#          junto al .tar.gz, se comprueba.
# BINARIO  el semillero nuevo; sin él, se compila este repositorio (cargo
#          build --release ... --features ssr) y se exige que /salud diga la
#          versión de Cargo.toml.
#
# Con SALTO_ATESTACION=1 se comprueba también la atestación de lo que se
# descarga (gh attestation verify). Para descargar hace falta gh (en la CI,
# con GH_TOKEN). Las versiones anteriores a la 0.3.0 buscaban la clave en
# /usr/local/etc/semillero/jellyfin.clave: probar desde una de ellas pide
# poder escribir ahí (root, un contenedor o un espacio de nombres).
#
# Necesita curl, sqlite3, python3, tar y sha256sum. Todo vive en una carpeta
# temporal fuera del repositorio, que se borra al terminar bien.

set -eu

RAIZ=$(cd "$(dirname "$0")/../.." && pwd)
OBJ=x86_64-unknown-linux-gnu
REPO=${GITHUB_REPOSITORY:-m4rz3r0/semillero}
CLAVE_0_2=/usr/local/etc/semillero/jellyfin.clave
# Tablas con datos de la prueba; las que no tenga la versión antigua se saltan.
TABLAS="ajustes reglas avisos exenciones historial normas presets normas_tracker trackers_vistos"
# Dos torrents de mentira (el hash v1 de qBittorrent: 40 hexadecimales).
HASH_A=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
HASH_B=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb

error()
{
	echo "salto-de-version: $*" >&2
	exit 1
}

for orden in curl sqlite3 python3 tar sha256sum; do
	command -v "$orden" >/dev/null 2>&1 || error "falta $orden"
done
if [ "$#" -gt 2 ]; then
	sed -n '/^# Uso: /s/^# Uso: //p' "$0" >&2
	exit 2
fi
antigua=${1:--}
binario=${2:-}

es_version()
{
	printf '%s\n' "$1" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'
}

# menor A B: A < B, comparando X.Y.Z número a número.
menor()
{
	[ "$1" != "$2" ] &&
		[ "$(printf '%s\n%s\n' "$1" "$2" | sort -t. -k1,1n -k2,2n -k3,3n | head -n 1)" = "$1" ]
}

nueva=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$RAIZ/crates/semillero/Cargo.toml" | head -n 1)
es_version "$nueva" || error "no se lee la versión de crates/semillero/Cargo.toml"

T=$(mktemp -d)
pid=
clave_puesta=
fallo=1
limpiar()
{
	if [ -n "$pid" ]; then
		kill "$pid" 2>/dev/null || true
		wait "$pid" 2>/dev/null || true
	fi
	if [ -n "$clave_puesta" ]; then rm -f "$clave_puesta"; fi
	if [ "$fallo" -eq 0 ]; then
		rm -rf "$T"
	else
		echo "salto-de-version: FALLO; carpeta de la prueba: $T" >&2
	fi
}
trap limpiar EXIT
trap 'exit 1' HUP INT TERM

# --- La versión antigua -------------------------------------------------

descargar()
{
	# descargar X.Y.Z: deja el paquete en $T/descarga, comprobado.
	v=$1
	command -v gh >/dev/null 2>&1 || error "falta gh para descargar la $v"
	paquete=semillero-$v-$OBJ.tar.gz
	mkdir "$T/descarga"
	gh release download "v$v" --repo "$REPO" --dir "$T/descarga" \
		--pattern "$paquete" --pattern SHA256SUMS ||
		error "no se puede descargar $paquete ni SHA256SUMS de la v$v de $REPO"
	comprobar_suma "$T/descarga/$paquete"
	if [ "${SALTO_ATESTACION:-0}" = 1 ]; then
		gh attestation verify "$T/descarga/$paquete" --repo "$REPO" >/dev/null ||
			error "la atestación de $paquete no cuadra con $REPO"
		echo "atestación de $paquete: bien"
	fi
	paquete=$T/descarga/$paquete
}

comprobar_suma()
{
	dir=$(dirname "$1")
	nombre=$(basename "$1")
	linea=$(awk -v n="$nombre" '$2 == n' "$dir/SHA256SUMS")
	[ -n "$linea" ] || error "$nombre no está en SHA256SUMS"
	(cd "$dir" && printf '%s\n' "$linea" | sha256sum -c - >/dev/null) ||
		error "la suma de $nombre no cuadra con SHA256SUMS"
	echo "suma de $nombre: bien"
}

case $antigua in
-)
	command -v gh >/dev/null 2>&1 || error "falta gh para buscar la versión anterior"
	antigua=$(gh release list --repo "$REPO" --exclude-drafts --exclude-pre-releases \
		--limit 100 --json tagName -q '.[].tagName' |
		sed -n 's/^v\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)$/\1/p' |
		while read -r v; do
			if menor "$v" "$nueva"; then echo "$v"; fi
		done | sort -t. -k1,1n -k2,2n -k3,3n | tail -n 1)
	[ -n "$antigua" ] || error "no hay ninguna release de $REPO anterior a la $nueva"
	descargar "$antigua"
	;;
v[0-9]* | [0-9]*)
	antigua=${antigua#v}
	es_version "$antigua" || error "«$1» no es una versión X.Y.Z"
	descargar "$antigua"
	;;
*)
	[ -e "$antigua" ] || error "no existe $antigua"
	paquete=$(cd "$(dirname "$antigua")" && pwd)/$(basename "$antigua")
	antigua=$(basename "$paquete" | sed -n "s/^semillero-\([0-9.]*\)-$OBJ\(\.tar\.gz\)\{0,1\}\$/\1/p")
	es_version "$antigua" || error "el nombre del paquete no es semillero-X.Y.Z-${OBJ}[.tar.gz]"
	if [ -f "$paquete" ] && [ -f "$(dirname "$paquete")/SHA256SUMS" ]; then
		comprobar_suma "$paquete"
	fi
	;;
esac
# Instalación como la de la guía: el paquete en $T/opt (/opt/semillero) y
# el estado en $T/estado (la carpeta de trabajo, /var/lib/semillero).
mkdir "$T/opt" "$T/estado"
chmod 700 "$T/estado"
if [ -d "$paquete" ]; then
	cp -R "$paquete" "$T/opt/antigua"
else
	mkdir "$T/desempaquetado"
	tar -xzf "$paquete" -C "$T/desempaquetado"
	[ -d "$T/desempaquetado/semillero-$antigua-$OBJ" ] ||
		error "el paquete no trae la carpeta semillero-$antigua-$OBJ"
	mv "$T/desempaquetado/semillero-$antigua-$OBJ" "$T/opt/antigua"
fi
[ -x "$T/opt/antigua/semillero" ] || error "el paquete no trae el binario semillero"
mkdir -p "$T/opt/antigua/site"

# --- La nueva -----------------------------------------------------------

comprobar_nueva=
if [ -z "$binario" ]; then
	echo "compilando la $nueva"
	# Como la release: el nombre de la web, que cargo-leptos fija al compilar.
	(cd "$RAIZ" && LEPTOS_OUTPUT_NAME=${LEPTOS_OUTPUT_NAME:-semillero} &&
		export LEPTOS_OUTPUT_NAME && cargo build --release -p semillero --bin semillero --locked --features ssr)
	binario=${CARGO_TARGET_DIR:-$RAIZ/target}/release/semillero
	comprobar_nueva=$nueva
fi
[ -x "$binario" ] || error "no existe el binario nuevo $binario"
mkdir -p "$T/opt/nueva/site"
cp "$binario" "$T/opt/nueva/semillero"
echo "de la $antigua a la ${comprobar_nueva:-nueva de $binario}"

# --- Configuración ------------------------------------------------------

# Puertos libres de 127.0.0.1: el de semillero y dos donde no escucha nadie
# (qBittorrent y Jellyfin de mentira).
puertos=$(python3 -c '
import socket
ss = [socket.socket() for _ in range(3)]
for s in ss:
    s.bind(("127.0.0.1", 0))
print(" ".join(str(s.getsockname()[1]) for s in ss))
')
# shellcheck disable=SC2086 # tres números separados por espacios
set -- $puertos
puerto=$1
puerto_qbt=$2
puerto_jellyfin=$3

# Donde busca la clave de Jellyfin por defecto la versión antigua.
if menor "$antigua" 0.3.0; then
	# Hasta la 0.2: ruta fija, y la base de datos por defecto era
	# /var/db/semillero/semillero.db, que se cambia por la de la carpeta de
	# trabajo (la clave por defecto de las nuevas va junto a ella).
	clave=$CLAVE_0_2
	db_env="SEMILLERO_DB=$T/estado/semillero.db"
	[ ! -e "$clave" ] || error "ya existe $clave; no se toca"
	if ! { mkdir -p "$(dirname "$clave")" && (umask 077 && : >"$clave"); } 2>/dev/null; then
		error "la $antigua busca la clave en $clave y no se puede escribir ahí (root, un contenedor o unshare -rm)"
	fi
	clave_puesta=$clave
else
	# Desde la 0.3.0: jellyfin.clave junto a la base de datos, semillero.db
	# en la carpeta de trabajo (sin SEMILLERO_DB, como en la guía).
	clave=$T/estado/jellyfin.clave
	db_env=
fi
(umask 077 && echo "clave-de-prueba-del-salto" >"$clave")
db=$T/estado/semillero.db

# Lo que pondría quien instala: escuchar en este equipo, su qBittorrent y su
# Jellyfin. SEMILLERO_JELLYFIN_CLAVE no se pone (el caso que se prueba).
{
	echo "SEMILLERO_LISTEN=127.0.0.1:$puerto"
	echo "SEMILLERO_QBT=http://127.0.0.1:$puerto_qbt"
	echo "SEMILLERO_JELLYFIN=http://127.0.0.1:$puerto_jellyfin"
	echo "SEMILLERO_LOG=info"
	if [ -n "$db_env" ]; then echo "$db_env"; fi
} >"$T/semillero.env"
chmod 600 "$T/semillero.env"

# arrancar VERSIÓN REGISTRO: arranca $T/opt/VERSIÓN/semillero con solo el
# entorno del fichero (como EnvironmentFile de systemd; con variables y no
# con --env, que solo existe desde la 0.4.0) y espera a /salud.
arrancar()
{
	# shellcheck disable=SC2016 # $1 y $2 son del sh de dentro
	(cd "$T/estado" && exec env -i PATH="$PATH" HOME="$T/estado" \
		LEPTOS_SITE_ROOT="$T/opt/$1/site" \
		sh -c 'set -a && . "$1" && set +a && exec "$2"' sh \
		"$T/semillero.env" "$T/opt/$1/semillero") >"$2" 2>&1 &
	pid=$!
	i=0
	while :; do
		if ! kill -0 "$pid" 2>/dev/null; then
			wait "$pid" && rc=0 || rc=$?
			pid=
			echo "salto-de-version: la $1 no arranca (código $rc); su salida:" >&2
			cat "$2" >&2
			exit 1
		fi
		if salud=$(curl -fsS --max-time 2 "http://127.0.0.1:$puerto/salud" 2>/dev/null); then
			break
		fi
		i=$((i + 1))
		if [ "$i" -ge 60 ]; then
			echo "salto-de-version: la $1 no contesta en /salud; su salida:" >&2
			cat "$2" >&2
			exit 1
		fi
		sleep 1
	done
}

parar()
{
	kill "$pid"
	wait "$pid" || true
	pid=
}

hay_tabla()
{
	[ "$(sqlite3 -batch "$db" "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = '$1'")" = 1 ]
}

# foto CARPETA: las filas de cada tabla, una por línea (en JSON, que no
# parte en líneas el TOML de un preset) y sin la llave del final: si una
# migración añade columnas, van al final y la fila de antes es el principio
# de la de después.
foto()
{
	mkdir "$1"
	for t in $TABLAS; do
		if hay_tabla "$t"; then
			sqlite3 -batch -json "$db" "SELECT * FROM $t" |
				sed 's/^\[//; s/\]$//; s/,$//; s/}$//' | grep -v '^$' | sort >"$1/$t" || true
		fi
	done
}

# --- La antigua, con datos ----------------------------------------------

arrancar antigua "$T/antigua.log"
case $salud in
*"\"version\":\"$antigua\""*) echo "la $antigua arranca: $salud" ;;
*) error "/salud de la antigua no dice la $antigua: $salud" ;;
esac
[ -f "$db" ] || error "la $antigua no ha creado $db"

ahora=$(date +%s)
{
	echo "PRAGMA busy_timeout = 5000;"
	echo "INSERT OR REPLACE INTO ajustes (clave, valor) VALUES ('modo', 'solo_aviso');"
	echo "INSERT INTO reglas (categoria, ratio, siembra_dias, inactividad_dias, disco_libre_pct)
		VALUES ('series', 2.5, 10, NULL, NULL), ('', NULL, NULL, 30, 5.0);"
	echo "INSERT INTO avisos (hash, regla, creado_en, vence_en)
		VALUES ('$HASH_A', 'ratio', $ahora, $((ahora + 86400)));"
	echo "INSERT INTO exenciones (hash, regla, creada)
		VALUES ('$HASH_B', 'siembra', $ahora);"
	echo "INSERT INTO historial (ts, hash, nombre, evento, regla, detalle)
		VALUES ($ahora, '$HASH_A', 'Prueba del salto', 'Aviso', 'ratio', 'ratio 2,50 de 2,50');"
	if hay_tabla normas; then
		echo "UPDATE normas SET siembra_dias = 3, aviso_horas = 12 WHERE id = 1;"
	fi
	if hay_tabla normas_tracker; then
		echo "INSERT INTO normas_tracker (host, umbral_pct, siembra_dias, ratio_min,
			plazo_largo_dias, ratio_pausa, aviso_horas)
			VALUES ('tracker.example', 20.0, 3, 0.5, 12, 4.0, 6);"
	fi
	if hay_tabla trackers_vistos; then
		echo "INSERT INTO trackers_vistos (hash, host, visto) VALUES ('$HASH_A', 'tracker.example', $ahora);"
	fi
	if hay_tabla presets; then
		echo "INSERT INTO presets (nombre, descripcion, archivo, cambiado) VALUES (
			'Prueba del salto', 'Siembra 3 días y ratio 0,5.', 'formato = 1
nombre = \"Prueba del salto\"
descripcion = \"Siembra 3 días y ratio 0,5.\"

[normas]
umbral_pct = 10.0
siembra_dias = 3
ratio_min = 0.5
plazo_largo_dias = 30
ratio_pausa = 2.0
aviso_horas = 12

[categorias.series]
ratio = 2.5
', $ahora);"
	fi
} | sqlite3 -batch "$db" >/dev/null || error "no se pueden meter los datos en la base de datos de la $antigua"
parar
foto "$T/antes"
migraciones_antes=$(sqlite3 -batch "$db" "SELECT count(*) FROM _sqlx_migrations WHERE success = 1")
echo "datos de la $antigua: $(cat "$T"/antes/* | wc -l | tr -d ' ') filas en $(cd "$T/antes" && echo *)"

# --- La nueva, con lo mismo ---------------------------------------------

arrancar nueva "$T/nueva.log"
echo "la nueva arranca con la configuración y los ficheros de la $antigua: $salud"
if [ -n "$comprobar_nueva" ]; then
	case $salud in
	*"\"version\":\"$comprobar_nueva\""*) ;;
	*) error "/salud de la nueva no dice la $comprobar_nueva: $salud" ;;
	esac
fi
# Con Jellyfin configurado, la nueva solo arranca si ha leído una clave; la
# única que hay es la de la antigua. Si la ha leído de la ruta de antes, lo
# dice; vale.
esc=$(printf '\033')
if grep -q 'donde estaba en la' "$T/nueva.log"; then
	echo "aviso de la nueva (se acepta): $(grep 'donde estaba en la' "$T/nueva.log" | head -n 1 | sed "s/$esc\[[0-9;]*m//g")"
fi
version_nueva=$(printf '%s\n' "$salud" | sed -n 's/.*"version":"\([^"]*\)".*/\1/p')
parar

malas=0
esperadas=$(find "$RAIZ/crates/semillero/migrations" -name '*.sql' | wc -l | tr -d ' ')
migraciones=$(sqlite3 -batch "$db" "SELECT count(*) FROM _sqlx_migrations WHERE success = 1")
if [ "$migraciones" -lt "$migraciones_antes" ] || [ "$migraciones" -ne "$esperadas" ]; then
	echo "salto-de-version: migraciones aplicadas: $migraciones_antes antes, $migraciones después; en el repositorio hay $esperadas" >&2
	malas=1
else
	echo "migraciones: $migraciones_antes antes, $migraciones después"
fi
foto "$T/despues"
for f in "$T"/antes/*; do
	t=$(basename "$f")
	if [ ! -f "$T/despues/$t" ]; then
		echo "salto-de-version: la tabla $t ya no está" >&2
		malas=1
		continue
	fi
	# Cada fila de antes sigue igual (o con columnas nuevas detrás): tras ella
	# se acaba la fila o va «,», así que un valor que solo crece por detrás no
	# cuela.
	faltan=$(awk 'NR == FNR { antes[$0]; next }
		{ for (f in antes) { c = substr($0, length(f) + 1, 1); if (index($0, f) == 1 && (c == "" || c == ",")) delete antes[f] } }
		END { for (f in antes) print f }' "$f" "$T/despues/$t")
	if [ -n "$faltan" ]; then
		echo "salto-de-version: en $t faltan o han cambiado estas filas:" >&2
		printf '%s\n' "$faltan" >&2
		malas=1
	fi
done
if [ "$malas" -ne 0 ]; then
	echo "salto-de-version: salida de la nueva:" >&2
	cat "$T/nueva.log" >&2
	exit 1
fi
echo "bien: la $version_nueva arranca sobre la instalación de la $antigua y conserva sus datos"
fallo=0
