#!/bin/sh
# Pruebas de las versiones precompiladas para FreeBSD (deploy/lib/,
# deploy/freebsd/recibir-precompilada.sh y deploy/cross/), sin servidor, sin
# PC de compilación y sin tocar el sistema: todo bajo una carpeta temporal.
# La parte del actualizador está en deploy/pruebas/actualizar.sh.
#
# 1. Nombres: version_valida, archivo_precompilada, fichero_de_buzon,
#    version_cargo.
# 2. ultima_version (PC) elige la misma etiqueta que traer_ultima_etiqueta y
#    ultima_etiqueta_remota (deploy/lib/git.sh, servidor) en un clon de
#    verdad, y solo vX.Y.Z.
# 3. decidir_precompilada: esperar la primera vez y mientras no pase la
#    espera, compilar después, instalar si ya llegó; una etiqueta nueva vuelve
#    a empezar la cuenta.
# 4. preparar_precompilada acepta un paquete bueno (deja solo semillero,
#    site/ y COMMIT) y rechaza, sin dejar nada: sha256 que no cuadra,
#    SHA256SUMS de dos líneas, otro commit, un commit no válido, un binario
#    de Linux, carpeta con otro nombre o algo más al lado, un enlace
#    simbólico o duro dentro, sin site/, sin COMMIT y un enlace en el buzón.
#    precompilada_llegada no da por llegada una versión sin SHA256SUMS.
# 5. recibir-precompilada.sh guarda lo admitido y rechaza versiones y
#    nombres no válidos, órdenes con más argumentos, ficheros vacíos o
#    demasiado grandes (sin dejar restos); conserva 3 versiones.
# 6. Lo que corre fuera del servidor se niega antes de compilar nada:
#    lanzar.sh sin configuración, con algo que no es una etiqueta vX.Y.Z, con
#    la copia de trabajo sucia, con una etiqueta que no es la versión de
#    Cargo.toml o sin PRECOMPILADAS_BUZON_SSH; con un PC que no responde,
#    sale con 3 y lo dice. compilar-freebsd.sh --release con una rama
#    (aunque se llame como una versión) o con otra versión en Cargo.toml, y
#    --prueba con --entregar. entregar.sh con una versión de --prueba, sin
#    clave o con un SHA256SUMS que no cuadra.
# 7. compilar-freebsd.sh de verdad con cargo, cross, rustup y podman de
#    mentira: el paquete (una carpeta con semillero, site/, COMMIT y
#    LICENSE; hash.txt dentro de site/), SHA256SUMS, que dos compilaciones
#    del mismo commit dan el mismo archivo, y la entrega con un ssh de
#    mentira que llama al receptor de verdad: el paquete primero, SHA256SUMS
#    el último, .entregada, y el actualizador (preparar_precompilada) lo
#    acepta. --ultima no repite lo entregado.
#
# Uso: deploy/pruebas/precompiladas.sh (desde cualquier sitio; necesita bash,
# git, tar, gzip, file, sha256sum, rsync y flock).

set -eu

DEPLOY=$(cd "$(dirname "$0")/.." && pwd)
LIB=$DEPLOY/lib/precompiladas.sh
RECIBIR=$DEPLOY/freebsd/recibir-precompilada.sh
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
trap 'exit 143' INT TERM

HOME=$TMP/casa
mkdir -p "$HOME"
GIT_CONFIG_NOSYSTEM=1
GIT_AUTHOR_NAME=prueba
GIT_AUTHOR_EMAIL=prueba
GIT_COMMITTER_NAME=prueba
GIT_COMMITTER_EMAIL=prueba
export HOME GIT_CONFIG_NOSYSTEM GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL

for orden in bash git tar gzip file sha256sum rsync flock; do
	command -v "$orden" >/dev/null 2>&1 || {
		echo "FALLO: falta $orden" >&2
		exit 1
	}
done

fallos=0
fallo()
{
	echo "FALLO: $*" >&2
	fallos=$((fallos + 1))
}

# shellcheck source=../lib/precompiladas.sh
. "$LIB"
# shellcheck source=../lib/git.sh
. "$DEPLOY/lib/git.sh"
OBJ=x86_64-unknown-freebsd

# Cabecera ELF mínima de x86-64 con el ABI $1 (9: FreeBSD, 0: Linux/SYSV);
# file la reconoce igual que un binario de verdad.
elf()
{
	printf '\177ELF\002\001\001'
	printf "\\$(printf '%03o' "$1")"
	printf '\000\000\000\000\000\000\000\000\002\000\076\000\001\000\000\000'
	head -c 40 /dev/zero
}
elf 9 >"$TMP/elf-freebsd"

# Construye en $1 (carpeta del buzón para la versión $2) un paquete con
# COMMIT $3. Variantes ($4): buena, linux, otro-nombre, extra, enlace, duro,
# sin-site, sin-commit, sha-malo, dos-lineas.
hacer_paquete()
{
	_dir=$1
	_ver=$2
	_commit=$3
	_variante=${4:-buena}
	_arch=$(archivo_precompilada "$_ver")
	_raiz=${_arch%.tar.gz}
	_t=$TMP/hacer
	rm -rf "$_t"
	if [ "$_variante" = otro-nombre ]; then _raiz=semillero-otra; fi
	mkdir -p "$_t/$_raiz/site/pkg" "$_dir"
	if [ "$_variante" = linux ]; then
		elf 0 >"$_t/$_raiz/semillero"
	else
		elf 9 >"$_t/$_raiz/semillero"
	fi
	echo 'js: abc' >"$_t/$_raiz/site/hash.txt"
	echo js >"$_t/$_raiz/site/pkg/semillero.abc.js"
	echo "$_commit" >"$_t/$_raiz/COMMIT"
	echo licencia >"$_t/$_raiz/LICENSE"
	case $_variante in
	extra) echo x >"$_t/suelto" ;;
	enlace) ln -s /etc/passwd "$_t/$_raiz/site/pkg/enlace" ;;
	duro) ln "$_t/$_raiz/LICENSE" "$_t/$_raiz/site/duro" ;;
	sin-site) rm -rf "$_t/$_raiz/site" ;;
	sin-commit) rm -f "$_t/$_raiz/COMMIT" ;;
	esac
	(cd "$_t" && tar -czf "$_dir/$_arch" -- *)
	(cd "$_dir" && sha256sum -- "$_arch" >SHA256SUMS)
	case $_variante in
	sha-malo) echo basura >>"$_dir/$_arch" ;;
	dos-lineas) echo "$(printf '%064d' 0)  otro.tar.gz" >>"$_dir/SHA256SUMS" ;;
	esac
}

# --- 1. nombres ---------------------------------------------------------------
[ "$(archivo_precompilada 1.2.3)" = semillero-1.2.3-x86_64-unknown-freebsd.tar.gz ] ||
	fallo "archivo_precompilada 1.2.3: $(archivo_precompilada 1.2.3)"
for v in 1.2.3 0.10.0 10.20.30; do
	version_valida "$v" || fallo "version_valida rechaza $v"
done
largo=1.1.$(printf '%040d' 1)
for v in '' v1.2.3 1.2 1.2.3.40000 1.2.3.x 1..3 .1.2 1.2. '1.2.3 ' prueba-abc 1.2.3-rc1 ../1.2 "$largo" '1.2
3'; do
	if version_valida "$v"; then fallo "version_valida acepta «$v»"; fi
done
for f in semillero-1.2.3-x86_64-unknown-freebsd.tar.gz SHA256SUMS; do
	fichero_de_buzon 1.2.3 "$f" || fallo "fichero_de_buzon rechaza $f"
done
for f in semillero-1.2.4-x86_64-unknown-freebsd.tar.gz semillero-1.2.3-x86_64-unknown-linux-gnu.tar.gz \
	semillero ../SHA256SUMS .SHA256SUMS SHA256SUMS.minisig; do
	if fichero_de_buzon 1.2.3 "$f"; then fallo "fichero_de_buzon acepta $f"; fi
done
[ "$(printf '[workspace]\nversion = "9"\n[package]\nname = "x"\nversion = "1.2.3"\n[dependencies]\nversion = "8"\n' | version_cargo)" = 1.2.3 ] ||
	fallo "version_cargo no lee la versión de [package]"

# --- 2. la misma última versión que el servidor ---------------------------------
git init -q -b main "$TMP/versiones"
echo uno >"$TMP/versiones/f"
git -C "$TMP/versiones" add f
git -C "$TMP/versiones" commit -q -m uno
for t in v0.9.9 v0.9.10 v0.10.0 v1.0.0 v1.0.0-rc1 v2-prueba otra; do
	git -C "$TMP/versiones" tag "$t"
done
git clone -q "$TMP/versiones" "$TMP/versiones-clon"
servidor=$(traer_ultima_etiqueta "$TMP/versiones-clon")
remota=$(ultima_etiqueta_remota "$TMP/versiones")
pc=$(git -C "$TMP/versiones" tag -l | ultima_version)
[ "$pc" = v1.0.0 ] || fallo "ultima_version da $pc, no v1.0.0"
[ "$pc" = "$servidor" ] && [ "$pc" = "$remota" ] ||
	fallo "ultima_version da $pc, el servidor elige $servidor y ultima_etiqueta_remota $remota"
[ "$(git -C "$TMP/versiones-clon" rev-parse HEAD)" = "$(git -C "$TMP/versiones" rev-parse 'v1.0.0^{commit}')" ] ||
	fallo "traer_ultima_etiqueta no deja el clon en la etiqueta"
[ "$(printf 'v0.9.9\nv0.9.10\n' | ultima_version)" = v0.9.10 ] || fallo "ultima_version no ordena v0.9.10 después de v0.9.9"
[ -z "$(printf 'otra\nv1\n' | ultima_version)" ] || fallo "ultima_version da algo sin etiquetas vX.Y.Z"

# --- 3. cuándo esperar ----------------------------------------------------------
E=$TMP/estado
[ "$(decidir_precompilada "$E" v1.0.0 1000 20 no)" = esperar ] || fallo "primera vez: no espera"
[ "$(cat "$E")" = "v1.0.0 1000" ] || fallo "primera vez: no apunta la hora ($(cat "$E"))"
[ "$(decidir_precompilada "$E" v1.0.0 2199 20 no)" = esperar ] || fallo "a los 19 min 59 s: no espera"
[ "$(decidir_precompilada "$E" v1.0.0 2200 20 no)" = compilar ] || fallo "a los 20 min: no compila"
[ "$(cat "$E")" = "v1.0.0 1000" ] || fallo "la hora de la primera vez cambia"
[ "$(decidir_precompilada "$E" v1.0.0 9999 20 si)" = instalar ] || fallo "llegada: no instala"
[ "$(decidir_precompilada "$E" v1.0.1 5000 20 no)" = esperar ] || fallo "etiqueta nueva: no vuelve a esperar"
[ "$(cat "$E")" = "v1.0.1 5000" ] || fallo "etiqueta nueva: no apunta su hora"
[ "$(decidir_precompilada "$E" v1.0.2 5000 0 no)" = compilar ] || fallo "espera 0: no compila enseguida"
[ "$(decidir_precompilada "$E" v1.0.2 5000 0 si)" = instalar ] || fallo "espera 0 y llegada: no instala"

# --- 4. comprobar un paquete ----------------------------------------------------
B=$TMP/buzon
# Commits de 40 cifras hex, generados (sin literales largos en el repositorio).
C=$(printf "%040d" 0 | tr 0 a)
OTRO=$(printf "%040d" 0 | tr 0 b)
V=1.2.3

precompilada_llegada "$B" "$V" && fallo "llegada sin nada en el buzón"
hacer_paquete "$B/$V" "$V" "$C"
rm -f "$B/$V/SHA256SUMS"
precompilada_llegada "$B" "$V" && fallo "llegada sin SHA256SUMS"
hacer_paquete "$B/$V" "$V" "$C"
precompilada_llegada "$B" "$V" || fallo "no da por llegada una versión completa"

if preparar_precompilada "$B" "$V" "$C" "$TMP/bueno" 2>"$TMP/err"; then
	[ "$(find "$TMP/bueno" -mindepth 1 -maxdepth 1 -printf '%f\n' | sort | tr '\n' ' ')" = "COMMIT semillero site " ] ||
		fallo "el paquete bueno no queda solo con semillero, site/ y COMMIT"
	[ "$(cat "$TMP/bueno/COMMIT")" = "$C" ] || fallo "COMMIT no es el de la etiqueta"
else
	fallo "rechaza el paquete bueno: $(cat "$TMP/err")"
fi

# rechazar NOMBRE VARIANTE COMMIT_ESPERADO MOTIVO_ESPERADO
rechazar()
{
	rm -rf "${B:?}/$V" "$TMP/malo"
	hacer_paquete "$B/$V" "$V" "$C" "$2"
	if preparar_precompilada "$B" "$V" "$3" "$TMP/malo" 2>"$TMP/err"; then
		fallo "$1: lo acepta"
	elif ! grep -q "$4" "$TMP/err"; then
		fallo "$1: el motivo no dice «$4»: $(cat "$TMP/err")"
	fi
	[ ! -e "$TMP/malo" ] || fallo "$1: deja la carpeta de destino"
}
rechazar "sha256 alterado" sha-malo "$C" sha256
rechazar "SHA256SUMS de dos líneas" dos-lineas "$C" "una sola línea"
rechazar "otro commit" buena "$OTRO" "otro commit"
rechazar "commit no válido" buena "abc" "commit no válido"
rechazar "binario de Linux" linux "$C" FreeBSD
rechazar "carpeta con otro nombre" otro-nombre "$C" "fuera de"
rechazar "algo al lado de la carpeta" extra "$C" "fuera de"
rechazar "enlace simbólico dentro" enlace "$C" "normal"
rechazar "enlace duro dentro" duro "$C" "normal"
rechazar "sin site/" sin-site "$C" "faltan"
rechazar "sin COMMIT" sin-commit "$C" "faltan"

rm -rf "${B:?}/$V"
hacer_paquete "$TMP/fuera" "$V" "$C"
mkdir -p "$B/$V"
ln -s "$TMP/fuera/$(archivo_precompilada "$V")" "$B/$V/$(archivo_precompilada "$V")"
cp "$TMP/fuera/SHA256SUMS" "$B/$V/"
precompilada_llegada "$B" "$V" && fallo "da por llegada un paquete que es un enlace"
if preparar_precompilada "$B" "$V" "$C" "$TMP/malo" 2>/dev/null; then
	fallo "acepta un paquete del buzón que es un enlace"
fi
rm -rf "${B:?}/$V" "$TMP/malo"

# --- 5. el receptor del buzón ---------------------------------------------------
R=$TMP/receptor
mkdir -p "$R"
recibir()
{
	SSH_ORIGINAL_COMMAND=$1 SEMILLERO_BUZON=$R SEMILLERO_BUZON_LIB=$LIB SEMILLERO_BUZON_MAXIMO=${MAXIMO:-1000} \
		sh "$RECIBIR" >"$TMP/salida" 2>&1
}
ARCH=$(archivo_precompilada "$V")
echo contenido | recibir "recibir $V $ARCH" || fallo "el receptor rechaza $ARCH: $(cat "$TMP/salida")"
[ "$(cat "$R/$V/$ARCH" 2>/dev/null)" = contenido ] || fallo "el receptor no guarda $ARCH"
echo sumas | recibir "recibir $V SHA256SUMS" || fallo "el receptor rechaza SHA256SUMS"
for orden in "recibir ../x SHA256SUMS" "recibir v$V SHA256SUMS" "recibir $V semillero" \
	"recibir $V ../SHA256SUMS" "recibir $V SHA256SUMS.minisig" "recibir $V SHA256SUMS extra" \
	"borrar $V SHA256SUMS" "recibir $V" "recibir 1.0.0;id SHA256SUMS" "recibir \$(id) SHA256SUMS" \
	"recibir * *" "recibir prueba-abc SHA256SUMS"; do
	if echo x | recibir "$orden"; then fallo "el receptor acepta «$orden»"; fi
done
if printf '' | recibir "recibir 2.0.0 SHA256SUMS"; then fallo "el receptor acepta un fichero vacío"; fi
if head -c 2000 /dev/zero | recibir "recibir 2.0.0 SHA256SUMS"; then
	fallo "el receptor acepta un fichero de más de SEMILLERO_BUZON_MAXIMO"
fi
[ -z "$(find "$R" -name '.*' -type f)" ] || fallo "el receptor deja temporales"
[ ! -e "$R/2.0.0/SHA256SUMS" ] || fallo "el receptor guarda un fichero rechazado"
[ ! -e "$R/x" ] && [ ! -e "$TMP/x" ] || fallo "el receptor escribe fuera de su versión"
for v in 3.0.0 3.0.1 3.0.2 3.0.3; do
	sleep 1
	echo s | recibir "recibir $v SHA256SUMS" || fallo "el receptor rechaza $v"
done
quedan=$(find "$R" -mindepth 1 -maxdepth 1 -type d -name '3.*' | wc -l)
[ "$quedan" -eq 3 ] && [ ! -e "$R/3.0.0" ] && [ ! -e "$R/$V" ] ||
	fallo "el receptor no deja solo las 3 versiones más recientes ($(ls "$R" | tr '\n' ' '))"

# --- 6. negativas fuera del servidor ---------------------------------------------
L=$TMP/lanzar
mkdir -p "$L/crates/semillero"
cp -R "$DEPLOY" "$L/deploy"
echo licencia >"$L/LICENSE"
printf '[package]\nname = "semillero"\nversion = "1.0.0"\n' >"$L/crates/semillero/Cargo.toml"
printf '[[package]]\nname = "wasm-bindgen"\nversion = "0.2.100"\n' >"$L/Cargo.lock"
git init -q -b main "$L"
git -C "$L" add -A
git -C "$L" commit -q -m v1.0.0
git -C "$L" tag -a -m v1.0.0 v1.0.0
git -C "$L" tag v1.1.0
git -C "$L" branch v2.0.0
# Destinos SSH de mentira (la arroba aparte: no son correos).
ARROBA=@
PC_FALSO=nadie${ARROBA}pc.invalid
BUZON_FALSO=smentrega${ARROBA}servidor.invalid
printf '. "$CONFIG_EJEMPLO"\nPC_COMPILAR_SSH=%s\nPRECOMPILADAS_BUZON_SSH=%s\n' "$PC_FALSO" "$BUZON_FALSO" >"$TMP/lanzar.conf"
lanzar()
{
	SEMILLERO_CONFIG=$TMP/lanzar.conf bash "$L/deploy/cross/lanzar.sh" "$@" >"$TMP/salida" 2>&1
}
# negar MOTIVO_ESPERADO ARGUMENTOS…
negar()
{
	_motivo=$1
	shift
	if lanzar "$@"; then
		fallo "lanzar.sh $*: no se niega"
	elif ! grep -q "$_motivo" "$TMP/salida"; then
		fallo "lanzar.sh $*: el motivo no dice «$_motivo»: $(cat "$TMP/salida")"
	fi
}
negar "no es una etiqueta" v2.0.0
negar "no es una versión" main
negar "no es una versión" v1.0
negar "no es una etiqueta" v9.9.9
negar "no coincide con la versión" v1.1.0
negar "uso:" v1.0.0 otra
echo cambio >>"$L/LICENSE"
negar "copia sucia" v1.0.0
git -C "$L" checkout -q -- LICENSE
if SEMILLERO_CONFIG=$TMP/no-existe bash "$L/deploy/cross/lanzar.sh" v1.0.0 >"$TMP/salida" 2>&1 ||
	! grep -q "config.example" "$TMP/salida"; then
	fallo "lanzar.sh sin configuración: $(cat "$TMP/salida")"
fi
st=0
lanzar v1.0.0 || st=$?
[ "$st" -eq 3 ] && grep -q "no responde" "$TMP/salida" ||
	fallo "lanzar.sh con el PC apagado: sale con $st: $(cat "$TMP/salida")"
printf '. "$CONFIG_EJEMPLO"\nPC_COMPILAR_SSH=%s\nPRECOMPILADAS_BUZON_SSH=\n' "$PC_FALSO" >"$TMP/lanzar.conf"
negar PRECOMPILADAS_BUZON_SSH v1.0.0

# compilar-freebsd.sh con cargo, cross, rustup y podman de mentira. En las
# negativas no llega a compilar: se niega antes.
F=$TMP/falsos
mkdir -p "$F"
cat >"$F/cargo" <<EOF
#!/bin/sh
echo "cargo \$*" >>"$TMP/cargo.log"
case "\$*" in
"leptos --version") echo "cargo-leptos 0.3.11" ;;
"leptos build --release --precompress")
	[ "\$LEPTOS_BIN_TARGET_TRIPLE" = $OBJ ] && [ "\$LEPTOS_BIN_CARGO_COMMAND" = cross ] || exit 1
	mkdir -p target/$OBJ/release target/$OBJ/site target/site/pkg
	cat "$TMP/elf-freebsd" >target/$OBJ/release/semillero
	printf 'js: h1\nwasm: h2\ncss: h3\n' >target/$OBJ/site/hash.txt
	echo js >target/site/pkg/semillero.h1.js
	echo wasm >target/site/pkg/semillero.h2.wasm
	echo css >target/site/pkg/semillero.h3.css
	: >target/site/pkg/.gitkeep
	;;
*) exit 1 ;;
esac
EOF
printf '#!/bin/sh\nexit 0\n' >"$F/rustup"
printf '#!/bin/sh\nexit 0\n' >"$F/cross"
printf '#!/bin/sh\nexit 0\n' >"$F/podman"
# ssh del PC al buzón: llama al receptor de verdad, como la orden forzada.
cat >"$F/ssh" <<EOF
#!/bin/sh
while :; do
	case \$1 in
	-o | -i) shift 2 ;;
	-*) shift ;;
	*) break ;;
	esac
done
shift
echo "\$*" >>"$TMP/ssh.log"
SSH_ORIGINAL_COMMAND="\$*" SEMILLERO_BUZON=$TMP/buzon-pc SEMILLERO_BUZON_LIB=$LIB exec sh "$RECIBIR"
EOF
chmod 755 "$F"/*
mkdir -p "$TMP/buzon-pc"
: >"$TMP/clave-falsa"
git -C "$L" bundle create -q "$TMP/l.bundle" --all 2>/dev/null
compilar()
{
	SEMILLERO_CROSS_DIR=$TMP/semillero-cross SEMILLERO_BUZON_CLAVE=$TMP/clave-falsa PATH="$F:$PATH" \
		bash "$DEPLOY/cross/compilar-freebsd.sh" "$@" >"$TMP/salida" 2>&1
}
if compilar --release v2.0.0 --bundle "$TMP/l.bundle"; then
	fallo "compilar-freebsd.sh --release con una rama: no se niega"
elif ! grep -q "no es una etiqueta" "$TMP/salida"; then
	fallo "compilar-freebsd.sh --release con una rama: $(cat "$TMP/salida")"
fi
if compilar --release main --bundle "$TMP/l.bundle"; then
	fallo "compilar-freebsd.sh --release main: no se niega"
fi
if compilar --release v1.1.0 --bundle "$TMP/l.bundle" || ! grep -q "no coincide" "$TMP/salida"; then
	fallo "compilar-freebsd.sh --release con otra versión en Cargo.toml: $(cat "$TMP/salida")"
fi
if compilar --prueba --entregar "$BUZON_FALSO"; then
	fallo "compilar-freebsd.sh --prueba --entregar: no se niega"
fi
[ ! -e "$TMP/cargo.log" ] || fallo "compilar-freebsd.sh compila antes de negarse"
entregar()
{
	SEMILLERO_BUZON_CLAVE=${CLAVE:-$TMP/clave-falsa} PATH="$F:$PATH" \
		bash "$DEPLOY/cross/entregar.sh" --destino "$BUZON_FALSO" "$1" >"$TMP/salida" 2>&1
}
mkdir -p "$TMP/prueba-abc"
if entregar "$TMP/prueba-abc" || ! grep -q "no es una versión" "$TMP/salida"; then
	fallo "entregar.sh con una versión de --prueba: $(cat "$TMP/salida")"
fi
hacer_paquete "$TMP/salida-mala/1.2.3" 1.2.3 "$C" sha-malo
if entregar "$TMP/salida-mala/1.2.3" || ! grep -q "no cuadra" "$TMP/salida"; then
	fallo "entregar.sh con SHA256SUMS que no cuadra: $(cat "$TMP/salida")"
fi
if CLAVE=$TMP/no-existe entregar "$TMP/salida-mala/1.2.3" || ! grep -q "falta la clave" "$TMP/salida"; then
	fallo "entregar.sh sin clave: $(cat "$TMP/salida")"
fi
[ ! -e "$TMP/ssh.log" ] || fallo "entregar.sh se conecta antes de negarse"

# --- 7. compilar, empaquetar y entregar ---------------------------------------------
COMMIT_L=$(git -C "$L" rev-parse 'v1.0.0^{commit}')
if ! compilar --release v1.0.0 --bundle "$TMP/l.bundle" --entregar "$BUZON_FALSO"; then
	fallo "compilar-freebsd.sh --release v1.0.0: $(cat "$TMP/salida")"
fi
S=$TMP/semillero-cross/salida/1.0.0
A=$(archivo_precompilada 1.0.0)
[ "$(tar -tzf "$S/$A" 2>/dev/null | sort | tr '\n' ' ')" = "semillero-1.0.0-$OBJ/ semillero-1.0.0-$OBJ/COMMIT semillero-1.0.0-$OBJ/LICENSE semillero-1.0.0-$OBJ/semillero semillero-1.0.0-$OBJ/site/ semillero-1.0.0-$OBJ/site/hash.txt semillero-1.0.0-$OBJ/site/pkg/ semillero-1.0.0-$OBJ/site/pkg/semillero.h1.js semillero-1.0.0-$OBJ/site/pkg/semillero.h2.wasm semillero-1.0.0-$OBJ/site/pkg/semillero.h3.css " ] ||
	fallo "el paquete no trae lo esperado: $(tar -tzf "$S/$A" 2>&1 | tr '\n' ' ')"
tar -tvzf "$S/$A" 2>/dev/null | grep -q '^-rwxr-xr-x 0/0 .*/semillero$' || fallo "el binario no va 755 y de 0:0"
[ "$(cat "$S/SHA256SUMS" 2>/dev/null)" = "$(cd "$S" && sha256sum "$A")" ] || fallo "SHA256SUMS no es la línea de sha256sum"
[ -f "$S/.entregada" ] || fallo "no marca .entregada"
[ "$(cat "$TMP/ssh.log" 2>/dev/null | tr '\n' ' ')" = "recibir 1.0.0 $A recibir 1.0.0 SHA256SUMS " ] ||
	fallo "la entrega no sube el paquete primero y SHA256SUMS el último: $(cat "$TMP/ssh.log")"
cmp -s "$S/$A" "$TMP/buzon-pc/1.0.0/$A" && cmp -s "$S/SHA256SUMS" "$TMP/buzon-pc/1.0.0/SHA256SUMS" ||
	fallo "lo del buzón no es lo compilado"
if preparar_precompilada "$TMP/buzon-pc" 1.0.0 "$COMMIT_L" "$TMP/instalada" 2>"$TMP/err"; then
	[ -f "$TMP/instalada/site/hash.txt" ] || fallo "hash.txt no va dentro de site/"
else
	fallo "el actualizador rechaza lo que entrega el PC: $(cat "$TMP/err")"
fi
grep -q 'leptos build --release --precompress' "$TMP/cargo.log" || fallo "no compila con cargo leptos build --release --precompress"
# Reproducible: el mismo commit da el mismo archivo.
cp "$S/$A" "$TMP/primero.tar.gz"
sleep 1
compilar --release v1.0.0 --bundle "$TMP/l.bundle" || fallo "segunda compilación: $(cat "$TMP/salida")"
cmp -s "$TMP/primero.tar.gz" "$S/$A" || fallo "dos compilaciones del mismo commit dan archivos distintos"
# --ultima: la última etiqueta (v1.0.0, sin la v1.1.0 de prueba) ya está
# entregada y no se repite.
git -C "$L" tag -d v1.1.0 >/dev/null
# (la segunda compilación, sin --entregar, rehízo salida/1.0.0 sin la marca)
: >"$S/.entregada"
: >"$TMP/cargo.log"
compilar --ultima --repo "$L" --entregar "$BUZON_FALSO" || fallo "--ultima: $(cat "$TMP/salida")"
grep -q 'v1.0.0 ya está compilada y entregada' "$TMP/salida" || fallo "--ultima no ve lo entregado: $(cat "$TMP/salida")"
grep -q 'leptos build' "$TMP/cargo.log" && fallo "--ultima vuelve a compilar lo entregado"

# --- sintaxis ------------------------------------------------------------------
for f in "$LIB" "$DEPLOY/lib/git.sh" "$DEPLOY/lib/config.sh" "$RECIBIR" "$DEPLOY/freebsd/actualizar.sh"; do
	sh -n "$f" || fallo "sh -n ${f#"$DEPLOY"/}"
done
for f in "$DEPLOY"/cross/*.sh; do
	bash -n "$f" || fallo "bash -n ${f#"$DEPLOY"/}"
done

if [ "$fallos" -ne 0 ]; then
	echo "$fallos fallos" >&2
	exit 1
fi
echo "precompiladas: todo bien"
