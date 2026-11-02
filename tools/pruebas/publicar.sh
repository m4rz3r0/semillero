#!/bin/sh
# Pruebas de tools/publicar/ (escáner, gancho pre-push, regenerar.sh y la
# comprobación de la CI) en Linux, sin red y sin tocar nada del equipo: todo
# vive en una carpeta temporal fuera del repositorio, con su propio HOME y
# sin la configuración global de git.
#
# Los valores «reales» que se plantan son inventados y se construyen al
# correr (troceados con variables), para que este fichero no dé hallazgos
# cuando el escáner revisa el propio repositorio.
#
# 0. sh -n de todos los guiones.
# 1. Escáner, árbol: un repositorio limpio (rangos de documentación,
#    *.example, *.home.arpa, correos noreply, bucle local permitido) da cero;
#    cada valor plantado (IPv4 privadas y pública, IPv6, MAC, correo, clave
#    privada, clave SSH, token, rutas /home y /Users, dominio, literal y
#    expresión de la lista privada, binario, metadatos PNG y JPEG, valor
#    privado dentro de un binario y en un nombre de fichero) sale con su
#    regla; las excepciones; la lista de permitidos (sin motivo: error).
# 2. Escáner, historial: valores que solo están en un commit viejo, en un
#    mensaje, en el mensaje de una etiqueta, en el correo del autor y en el
#    nombre de una rama.
# 3. Ninguna salida contiene un valor plantado (solo muestras tapadas).
# 4. Gancho pre-push: bloquea al subir algo con hallazgos al remoto público,
#    deja pasar lo limpio y cualquier subida a otro remoto; el instalador no
#    pisa otro gancho sin --forzar.
# 5. ci.sh: limpio da 0, sucio da 1, clon superficial da 2.
# 6. regenerar.sh de principio a fin (si hay git filter-repo; si no, se salta
#    y lo dice): rama y etiquetas, rutas privadas, sustituciones, mensajes,
#    autores, excepciones, gancho y órdenes de push; y que rechaza una copia
#    con hallazgos.
#
# Uso: tools/pruebas/publicar.sh (desde cualquier sitio; necesita git, awk,
# od, grep y sed). Con SEMILLERO_PRUEBA_CONSERVAR=1 no borra la carpeta
# temporal; si algo falla, tampoco.

set -eu

RAIZ=$(cd "$(dirname "$0")/../.." && pwd)
PUB=$RAIZ/tools/publicar
ESC=$PUB/escanear.sh
T=$(mktemp -d "${TMPDIR:-/tmp}/prueba-publicar.XXXXXX")
echo "prueba-publicar: carpeta temporal $T"

# Git aislado: sin configuración global ni del sistema, HOME propio.
export HOME="$T/home"
mkdir -p "$HOME"
export GIT_CONFIG_NOSYSTEM=1
export GIT_CONFIG_GLOBAL=/dev/null
export GIT_AUTHOR_NAME=Prueba GIT_COMMITTER_NAME=Prueba
export GIT_AUTHOR_EMAIL=1+prueba@users.noreply.github.com
export GIT_COMMITTER_EMAIL=1+prueba@users.noreply.github.com
unset GIT_DIR GIT_WORK_TREE 2>/dev/null || true

fallos=0
n_ok=0
bien()
{
	n_ok=$((n_ok + 1))
}
mal()
{
	echo "FALLO: $*" >&2
	fallos=$((fallos + 1))
}

# Valores plantados, troceados.
P=.
C=:
A=@
IP10="10${P}20${P}30${P}40"
IP172="172${P}16${P}5${P}9"
IP192="192${P}168${P}7${P}1"
IPPUB="93${P}184${P}216${P}34"
IP6="fd12${C}3456${C}${C}7"
MAC="02${C}42${C}ac${C}11${C}00${C}02"
CORREO="ana${A}correo-privado${P}es"
CLAVE="-----BEGIN OPENSSH PRIV""ATE KEY-----"
SSHPUB="ssh-ed25519 AA""AAC3NzaC1lZDI1NTE5AAAAIEjemplo"
VALOR="a1B2c3D4e5F6""g7H8i9J0kLmN"
TOKEN="api_t""oken = \"$VALOR\""
CASA="/ho""me/ana/proyecto/"
MAC_OS="/Us""ers/ana/proyecto/"
DOMINIO="servidor${P}mi-casa${P}es"
ALIAS="zorro""plateado"
PRIV_RE="casa-""4821"
PLANTADOS="$IP10 $IP172 $IP192 $IPPUB $IP6 $MAC $CORREO $VALOR $CASA $MAC_OS $DOMINIO $ALIAS $PRIV_RE"

# Lista privada, permitidos y excepciones de prueba.
cat >"$T/privado.txt" <<EOF
# lista de prueba (valores inventados)
$ALIAS==>autor
regex:casa-[0-9]+==>casa-x
$CORREO==>alguien${A}example
EOF
cat >"$T/permitido.txt" <<EOF
# permitidos de prueba
ip      127.0.0.0/8   bucle local
dominio github.com    alojamiento
binario img/*.png     capturas de prueba
EOF
printf '# nada permitido\n' >"$T/permitido-vacio.txt"
printf 'ip 127.0.0.0/8\n' >"$T/permitido-sin-motivo.txt"
URL_PUBLICA="https://github${P}com/$ALIAS/semillero"
printf '%s\n' "$URL_PUBLICA" >"$T/excepciones.txt"

nuevo_repo()
{
	git init -q -b main "$1"
	git -C "$1" config commit.gpgsign false
	git -C "$1" config tag.gpgsign false
}

confirmar()
{
	git -C "$1" add -A
	git -C "$1" commit -q -m "$2"
}

# Ejecuta el escáner; deja la salida en $T/salida y el código en $rc.
escanear()
{
	rc=0
	sh "$ESC" "$@" >"$T/salida" 2>"$T/errores" || rc=$?
	cat "$T/salida" "$T/errores" >>"$T/todas-las-salidas"
}

# espera DESCRIPCIÓN TEXTO: la salida tiene una línea que contiene TEXTO.
espera()
{
	if grep -qF -- "$2" "$T/salida"; then bien; else
		mal "$1: falta «$2» en la salida"
		sed 's/^/    /' "$T/salida" >&2
	fi
}

no_espera()
{
	if grep -qF -- "$2" "$T/salida"; then
		mal "$1: sobra «$2» en la salida"
	else bien; fi
}

espera_rc()
{
	if [ "$rc" -eq "$2" ]; then bien; else
		mal "$1: salida $rc, se esperaba $2"
		sed 's/^/    /' "$T/salida" "$T/errores" >&2
	fi
}

# PNG mínimo; con $2, un trozo tEXt «Author» con ese texto.
png()
{
	{
		printf '\211PNG\r\n\032\n'
		printf '\000\000\000\015IHDR\000\000\000\001\000\000\000\001\010\002\000\000\000\000\000\000\000'
		if [ -n "${2:-}" ]; then
			n=$((7 + ${#2}))
			printf '\000\000\000'
			printf "\\$(printf '%03o' "$n")"
			printf 'tEXtAuthor\000%s\000\000\000\000' "$2"
		fi
		printf '\000\000\000\000IEND\256B`\202'
	} >"$1"
}

# JPEG mínimo; con $2 = exif, un segmento APP1 Exif.
jpeg()
{
	{
		printf '\377\330\377\340\000\020JFIF\000\001\001\000\000\001\000\001\000\000'
		[ "${2:-}" != exif ] || printf '\377\341\000\010Exif\000\000'
		printf '\377\331'
	} >"$1"
}

echo "0. sintaxis"
for f in "$PUB"/*.sh "$PUB/gancho-pre-push" "$RAIZ/tools/pruebas/publicar.sh"; do
	sh -n "$f" && bien || mal "sh -n $f"
done

echo "1. escáner: árbol"
L=$T/limpio
nuevo_repo "$L"
cat >"$L/ejemplo.conf" <<EOF
servidor = 192.0.2.10
pasarela = 198.51.100.1
publica = 203.0.113.99
v6 = 2001:db8::1
local = 127.0.0.1
mac = 00:00:5e:00:53:01
correo = persona${A}correo.example
autor = 1+ana${A}users.noreply.github.com
web = https://web.example/ y nas.home.arpa, localhost
codigo = https://github.com/ejemplo/semillero
version = 1.2.3 y hora 12:30:45
EOF
confirmar "$L" "limpio"
escanear --permitido "$T/permitido.txt" "$L"
espera_rc "repositorio limpio, árbol" 0
escanear --historia --permitido "$T/permitido.txt" "$L"
espera_rc "repositorio limpio, historial" 0
escanear --permitido "$T/permitido-vacio.txt" "$L"
espera_rc "bucle local sin permitir" 1
espera "bucle local sin permitir" "ejemplo.conf:5: ipv4-privada: 12"
escanear --permitido "$T/permitido-sin-motivo.txt" "$L"
espera_rc "permitido sin motivo" 2

S=$T/sucio
nuevo_repo "$S"
planta()
{
	printf 'línea sin nada\n%s\n' "$2" >"$S/$1"
}
planta ip10.txt "origen = $IP10"
planta ip172.txt "red $IP172/24"
planta ip192.txt "http://$IP192:8080/"
planta ippub.txt "dns $IPPUB."
planta ip6.txt "v6 = $IP6"
planta mac.txt "mac $MAC"
planta correo.txt "contacto: $CORREO"
planta clave.txt "$CLAVE"
planta ssh.txt "$SSHPUB ana"
planta token.txt "$TOKEN"
planta casa.txt "ruta $CASA"
planta macos.txt "ruta $MAC_OS"
planta dominio.txt "https://$DOMINIO/estado"
planta alias.txt "hecho por $ALIAS"
planta regex.txt "equipo $PRIV_RE"
planta excepcion.txt "código en $URL_PUBLICA"
mkdir "$S/img" "$S/doc"
png "$S/doc/captura.png"
png "$S/img/limpia.png"
png "$S/img/meta.png" "$ALIAS"
jpeg "$S/img/foto.jpg" exif
jpeg "$S/img/sin-exif.jpg"
printf 'nada\n' >"$S/notas-$ALIAS.txt"
confirmar "$S" "sucio"
escanear --permitido "$T/permitido.txt" --privado "$T/privado.txt" \
	--excepciones "$T/excepciones.txt" "$S"
espera_rc "repositorio sucio" 1
espera "IPv4 10/8" "ip10.txt:2: ipv4-privada: "
espera "IPv4 172.16/12" "ip172.txt:2: ipv4-privada: "
espera "IPv4 192.168/16" "ip192.txt:2: ipv4-privada: "
espera "IPv4 pública" "ippub.txt:2: ipv4-publica: "
espera "IPv6" "ip6.txt:2: ipv6: "
espera "MAC" "mac.txt:2: mac: "
espera "correo" "correo.txt:2: correo: "
espera "correo de la lista privada" "correo.txt:2: privado: entrada 4"
espera "clave privada" "clave.txt:2: clave-privada: "
espera "clave SSH" "ssh.txt:2: clave-ssh: "
espera "token" "token.txt:2: token: "
espera "ruta /home" "casa.txt:2: ruta-personal: "
espera "ruta /Users" "macos.txt:2: ruta-personal: "
espera "dominio" "dominio.txt:2: dominio: "
espera "literal privado" "alias.txt:2: privado: entrada 2"
espera "expresión privada" "regex.txt:2: privado: entrada 3"
no_espera "excepción" "excepcion.txt"
espera "binario no permitido" "doc/captura.png: binario: png"
no_espera "binario permitido" "img/limpia.png"
no_espera "binario permitido con metadatos" "img/meta.png: binario"
espera "metadatos PNG" "img/meta.png: metadatos-png: tEXt"
espera "valor privado dentro de un binario" "img/meta.png (texto dentro del binario):"
espera "metadatos JPEG" "img/foto.jpg: metadatos-jpeg: APP1"
no_espera "JPEG sin Exif" "sin-exif.jpg: metadatos-jpeg"
espera "valor privado en un nombre de fichero" "nombre de fichero «notas-‹privado#2›.txt»: privado: entrada 2"
for f in ip10 ip172 ip192 ippub ip6 mac correo clave ssh token casa macos dominio; do
	escanear --permitido "$T/permitido.txt" "$S"
	grep -q "^$f.txt:2: " "$T/salida" || mal "sin lista privada no sale $f.txt"
done
no_espera "sin lista privada, nada privado" ": privado: "
no_espera "línea sin nada" ":1: "

echo "2. escáner: historial"
H=$T/historial
nuevo_repo "$H"
printf 'origen = %s\n' "$IP10" >"$H/a.txt"
confirmar "$H" "primero"
printf 'origen = 192.0.2.1\n' >"$H/a.txt"
confirmar "$H" "quita la dirección"
printf 'b\n' >"$H/b.txt"
confirmar "$H" "arreglo de $ALIAS"
git -C "$H" tag -a v1.0.0 -m "versión, avisos a $CORREO"
printf 'c\n' >"$H/c.txt"
git -C "$H" add c.txt
GIT_AUTHOR_EMAIL=$CORREO git -C "$H" commit -q -m "otro"
git -C "$H" branch "rama-$PRIV_RE"
escanear --permitido "$T/permitido.txt" --privado "$T/privado.txt" "$H"
espera_rc "historial: el árbol está limpio" 0
escanear --historia --permitido "$T/permitido.txt" --privado "$T/privado.txt" "$H"
espera_rc "historial sucio" 1
espera "valor solo en un commit viejo" "a.txt (blob "
espera "valor solo en un commit viejo (regla)" "ipv4-privada"
grep "^commit " "$T/salida" | grep -q ": privado: entrada 2" || mal "valor solo en un mensaje de commit"
grep "^etiqueta " "$T/salida" | grep -q ": correo: " || mal "valor solo en el mensaje de una etiqueta"
grep "^commit " "$T/salida" | grep -q ": correo: " || mal "valor solo en el correo del autor"
grep -q "^referencias:[0-9]*: privado: entrada 3" "$T/salida" || mal "valor solo en el nombre de una rama"
escanear --historia --revs "main~1 ^main~2" --permitido "$T/permitido.txt" --privado "$T/privado.txt" "$H"
espera "--revs solo mira esos commits" "privado: entrada 2"
no_espera "--revs no mira el commit viejo" "a.txt"

echo "3. ninguna salida enseña un valor plantado"
for v in $PLANTADOS; do
	if grep -qF -- "$v" "$T/todas-las-salidas"; then
		mal "la salida enseña un valor plantado (empieza por $(printf '%.2s' "$v"))"
	else bien; fi
done

echo "4. gancho pre-push"
git init -q --bare "$T/publico.git"
git init -q --bare "$T/otro.git"
W=$T/trabajo
nuevo_repo "$W"
printf 'limpio\n' >"$W/a.txt"
confirmar "$W" "limpio"
printf '#!/bin/sh\nexit 0\n' >"$W/.git/hooks/pre-push"
rc=0
sh "$PUB/instalar-gancho.sh" --publico "$T/publico.git" --privado "$T/privado.txt" "$W" >/dev/null 2>&1 || rc=$?
espera_rc "el instalador no pisa otro gancho" 2
rc=0
sh "$PUB/instalar-gancho.sh" --forzar --publico "$T/publico.git" --privado "$T/privado.txt" \
	"$W" >/dev/null 2>&1 || rc=$?
espera_rc "instalador con --forzar" 0
cmp -s "$W/.git/hooks/pre-push" "$PUB/gancho-pre-push" && bien || mal "el gancho no quedó instalado"
subir()
{
	rc=0
	git -C "$W" push -q "$@" >"$T/salida" 2>&1 || rc=$?
	cat "$T/salida" >>"$T/todas-las-salidas"
}
subir "$T/publico.git" main
espera_rc "subida limpia al público" 0
printf 'hecho por %s\n' "$ALIAS" >"$W/b.txt"
confirmar "$W" "con valor privado"
subir "$T/publico.git" main
espera_rc "subida con un valor privado al público" 1
espera "el gancho bloquea" "BLOQUEADO"
espera "el gancho dice dónde" "b.txt (blob "
subir "$T/otro.git" main
espera_rc "subida a otro remoto" 0
git -C "$W" reset -q --hard HEAD~1
git -C "$W" commit -q --allow-empty -m "mensaje con $PRIV_RE"
subir "$T/publico.git" main
espera_rc "mensaje con un valor privado al público" 1
git -C "$W" reset -q --hard HEAD~1
git -C "$W" tag -a v0.1.0 -m "etiqueta para $ALIAS"
subir "$T/publico.git" refs/tags/v0.1.0
espera_rc "etiqueta con un valor privado al público" 1
git -C "$W" tag -d v0.1.0 >/dev/null
git -C "$W" tag -a v0.1.0 -m "primera versión"
subir "$T/publico.git" refs/tags/v0.1.0
espera_rc "etiqueta limpia al público" 0
git -C "$W" config semillero.privado "$T/no-existe.txt"
git -C "$W" commit -q --allow-empty -m "limpio otra vez"
subir "$T/publico.git" main
espera_rc "sin lista privada no se sube al público" 1
git -C "$W" config --unset semillero.publico
subir "$T/publico.git" main
espera_rc "sin semillero.publico el gancho no mira nada" 0

echo "5. ci.sh"
CI=$T/ci
nuevo_repo "$CI"
mkdir -p "$CI/tools"
cp -R "$PUB" "$CI/tools/publicar"
cp "$T/permitido.txt" "$CI/tools/publicar/permitido.txt"
printf 'servidor 192.0.2.10\n' >"$CI/a.txt"
confirmar "$CI" "limpio"
rc=0
sh "$CI/tools/publicar/ci.sh" >"$T/salida" 2>&1 || rc=$?
espera_rc "ci.sh limpio" 0
printf 'servidor %s\n' "$IP192" >"$CI/a.txt"
confirmar "$CI" "sucio"
printf 'servidor 192.0.2.10\n' >"$CI/a.txt"
confirmar "$CI" "limpio otra vez"
rc=0
sh "$CI/tools/publicar/ci.sh" >"$T/salida" 2>&1 || rc=$?
espera_rc "ci.sh con algo en el historial" 1
git clone -q --depth 1 "file://$CI" "$T/ci-superficial"
rc=0
sh "$T/ci-superficial/tools/publicar/ci.sh" >"$T/salida" 2>&1 || rc=$?
espera_rc "ci.sh en un clon superficial" 2

echo "6. regenerar.sh"
if ! git filter-repo --version >/dev/null 2>&1; then
	echo "SALTADO: no hay git filter-repo; las pruebas de regenerar.sh no se ejecutan" >&2
else
	R=$T/privado-repo
	nuevo_repo "$R"
	mkdir -p "$R/odd/tasks" "$R/docs/superpowers" "$R/src"
	printf 'red de casa %s\n' "$IP10" >"$R/odd/tasks/plan.md"
	printf 'plan de %s\n' "$ALIAS" >"$R/docs/superpowers/notas.md"
	cat >"$R/README.md" <<EOF
Semillero, de $ALIAS.
Código: $URL_PUBLICA
Ejemplo: 192.0.2.10, equipo $PRIV_RE.
EOF
	printf 'fn main() {}\n' >"$R/src/main.rs"
	confirmar "$R" "inicio por $ALIAS"
	git -C "$R" tag -a v0.1.0 -m "versión 0.1.0 de $ALIAS"
	git -C "$R" tag prueba-interna
	printf 'más\n' >>"$R/src/main.rs"
	git -C "$R" add -A
	GIT_AUTHOR_EMAIL=$CORREO GIT_COMMITTER_EMAIL=$CORREO git -C "$R" commit -q \
		-m "arreglo" -m "(cherry picked from commit 0123456789abcdef0123456789abcdef01234567)"
	git -C "$R" checkout -q -b experimento
	printf 'experimento\n' >"$R/exp.txt"
	confirmar "$R" "experimento"
	git -C "$R" tag -a v9.9.9 -m "solo en la rama experimento"
	git -C "$R" checkout -q main
	git -C "$R" config semillero.publico "$T/publico-regenerado.git"

	rc=0
	sh "$PUB/regenerar.sh" --origen "$R" --privado "$T/privado.txt" \
		--excepciones "$T/excepciones.txt" --destino "$T/regen-sin-correo" \
		--nombre "Autor Publico" >"$T/salida" 2>&1 || rc=$?
	espera_rc "regenerar sin correo público" 2

	rc=0
	sh "$PUB/regenerar.sh" --origen "$R" --privado "$T/privado.txt" \
		--excepciones "$T/excepciones.txt" --destino "$T/regen" \
		--nombre "Autor Publico" --correo "1+autor@users.noreply.github.com" \
		>"$T/salida" 2>&1 || rc=$?
	cat "$T/salida" >>"$T/todas-las-salidas"
	espera_rc "regenerar.sh" 0
	G=$T/regen/repo
	refs=$(git -C "$G" for-each-ref --format='%(refname)' | tr '\n' ' ')
	[ "$refs" = "refs/heads/main refs/tags/v0.1.0 " ] && bien ||
		mal "regenerar: referencias «$refs»"
	git -C "$G" rev-list --objects --all | grep -qE ' (odd|docs/superpowers)(/|$)' &&
		mal "regenerar: quedan rutas privadas" || bien
	todo=$(git -C "$G" log --all --format='%an <%ae> %cn <%ce>%n%B' -p)
	todo="$todo$(git -C "$G" for-each-ref --format='%(taggername) %(taggeremail) %(contents)' refs/tags)"
	printf '%s\n' "$todo" | grep -F "$ALIAS" | grep -vqF "$URL_PUBLICA" &&
		mal "regenerar: queda el literal privado fuera de la excepción" || bien
	printf '%s\n' "$todo" | grep -qF "$URL_PUBLICA" && bien ||
		mal "regenerar: la excepción no sobrevivió"
	printf '%s\n' "$todo" | grep -qF "$PRIV_RE" && mal "regenerar: queda la expresión privada" || bien
	printf '%s\n' "$todo" | grep -qF "casa-x" && bien || mal "regenerar: no se aplicó la expresión"
	printf '%s\n' "$todo" | grep -qF "$CORREO" && mal "regenerar: queda el correo privado" || bien
	printf '%s\n' "$todo" | grep -q "cherry picked" && mal "regenerar: queda «cherry picked from»" || bien
	ids=$(git -C "$G" log --all --format='%an <%ae>|%cn <%ce>' | sort -u | tr '\n' ' ')
	[ "$ids" = "Autor Publico <1+autor@users.noreply.github.com>|Autor Publico <1+autor@users.noreply.github.com> " ] &&
		bien || mal "regenerar: autores «$ids»"
	[ "$(git -C "$G" for-each-ref --format='%(taggeremail)' refs/tags)" = "<1+autor@users.noreply.github.com>" ] &&
		bien || mal "regenerar: firmante de la etiqueta"
	cmp -s "$G/.git/hooks/pre-push" "$PUB/gancho-pre-push" && bien || mal "regenerar: sin gancho en la copia"
	grep -rqF "$ALIAS" "$T/regen" --exclude-dir=repo &&
		mal "regenerar: deja una copia de la lista privada junto al resultado" || bien
	espera "regenerar: orden de push de la rama" "push '$T/publico-regenerado.git' 'refs/heads/main:refs/heads/main'"
	espera "regenerar: orden de push de la etiqueta" "push '$T/publico-regenerado.git' 'refs/tags/v0.1.0'"
	no_espera "regenerar: nada de la otra rama" "v9.9.9"
	git -C "$R" log --all --format=%B | grep -qF "$ALIAS" && bien ||
		mal "regenerar tocó el repositorio privado"
	# La copia regenerada se puede subir al público con su gancho.
	git init -q --bare "$T/publico-regenerado.git"
	rc=0
	git -C "$G" push -q "$T/publico-regenerado.git" refs/heads/main:refs/heads/main >"$T/salida" 2>&1 || rc=$?
	espera_rc "subir la copia regenerada con el gancho" 0

	# Modo aplanado: un solo commit por versión, sin el historial privado,
	# con fecha UTC a las 00:00 y encima del público anterior con --sobre.
	ID_PUB="Autor Publico <1+autor@users.noreply.github.com>"
	regen_plano()
	{
		rc=0
		sh "$PUB/regenerar.sh" --origen "$R" --privado "$T/privado.txt" \
			--excepciones "$T/excepciones.txt" --nombre "Autor Publico" \
			--correo "1+autor@users.noreply.github.com" "$@" >"$T/salida" 2>&1 || rc=$?
		cat "$T/salida" >>"$T/todas-las-salidas"
	}
	regen_plano --plano 0.2 --destino "$T/plano-mal"
	espera_rc "plano con versión mal escrita" 2
	mkdir -p "$R/crates/semillero"
	printf '[package]\nname = "semillero"\nversion = "0.2.0"\n' >"$R/crates/semillero/Cargo.toml"
	confirmar "$R" "versión 0.2.0"
	regen_plano --plano v0.9.0 --destino "$T/plano-otra"
	espera_rc "plano con una versión distinta de la de Cargo.toml" 2
	espera "plano: dice la versión de Cargo.toml" "0.2.0"
	regen_plano --plano v0.2.0 --destino "$T/plano1"
	espera_rc "regenerar --plano" 0
	P=$T/plano1/repo
	refs=$(git -C "$P" for-each-ref --format='%(refname)' | tr '\n' ' ')
	[ "$refs" = "refs/heads/main refs/tags/v0.2.0 " ] && bien || mal "plano: referencias «$refs»"
	[ "$(git -C "$P" rev-list --count --all)" = 1 ] && bien || mal "plano: más de un commit"
	[ "$(git -C "$P" log -1 --format=%B | sed '/^$/d')" = "semillero v0.2.0" ] && bien ||
		mal "plano: mensaje «$(git -C "$P" log -1 --format=%B)»"
	fechas=$(git -C "$P" log -1 --format='%ai|%ci')
	case $fechas in
	*" 00:00:00 +0000|"*" 00:00:00 +0000") bien ;;
	*) mal "plano: fechas «$fechas»" ;;
	esac
	[ "$(git -C "$P" log -1 --format='%an <%ae>|%cn <%ce>')" = "$ID_PUB|$ID_PUB" ] && bien ||
		mal "plano: autor"
	todo=$(git -C "$P" log --all --format='%B' -p)
	printf '%s\n' "$todo" | grep -qF "arreglo" && mal "plano: quedan mensajes privados" || bien
	printf '%s\n' "$todo" | grep -F "$ALIAS" | grep -vqF "$URL_PUBLICA" &&
		mal "plano: queda el literal privado" || bien
	git -C "$P" rev-list --objects --all | grep -qE ' (odd|docs/superpowers)(/|$)' &&
		mal "plano: quedan rutas privadas" || bien
	git -C "$P" cat-file -t "$(git -C "$R" rev-parse main~1)" >/dev/null 2>&1 &&
		mal "plano: quedan objetos del historial privado" || bien
	git init -q --bare "$T/publico-plano.git"
	git -C "$P" push -q "$T/publico-plano.git" refs/heads/main:refs/heads/main refs/tags/v0.2.0 >/dev/null 2>&1
	primero=$(git -C "$P" rev-parse main)
	# Sin cambios desde la versión publicada: nada que publicar.
	regen_plano --plano v0.3.0 --sobre "$T/publico-plano.git" --destino "$T/plano-igual"
	espera_rc "plano sin cambios" 2
	# Una versión que ya está en el público no se repite.
	printf 'nuevo\n' >"$R/src/nuevo.rs"
	sed -i.bak 's/0\.2\.0/0.3.0/' "$R/crates/semillero/Cargo.toml" && rm -f "$R/crates/semillero/Cargo.toml.bak"
	confirmar "$R" "nuevo por $ALIAS"
	regen_plano --plano v0.2.0 --sobre "$T/publico-plano.git" --destino "$T/plano-repe"
	espera_rc "plano con versión ya publicada" 2
	regen_plano --plano v0.3.0 --sobre "$T/publico-plano.git" --destino "$T/plano2"
	espera_rc "regenerar --plano --sobre" 0
	P=$T/plano2/repo
	[ "$(git -C "$P" rev-parse main~1)" = "$primero" ] && bien || mal "plano --sobre: no cuelga del público"
	[ "$(git -C "$P" rev-list --count main)" = 2 ] && bien || mal "plano --sobre: commits"
	git -C "$P" log -1 --format=%B | grep -qF "$ALIAS" && mal "plano --sobre: mensaje privado" || bien
	espera "plano: orden de push de la etiqueta nueva" "'refs/tags/v0.3.0'"
	no_espera "plano: no vuelve a subir la etiqueta vieja" "'refs/tags/v0.2.0'"
	rc=0
	git -C "$P" push -q "$T/publico-plano.git" refs/heads/main:refs/heads/main >/dev/null 2>&1 || rc=$?
	espera_rc "plano --sobre: avance rápido en el público" 0

	# Algo que la lista no cubre: se rechaza y queda la carpeta.
	printf 'pasarela %s\n' "$IP172" >"$R/red.txt"
	confirmar "$R" "red"
	rc=0
	sh "$PUB/regenerar.sh" --origen "$R" --privado "$T/privado.txt" \
		--excepciones "$T/excepciones.txt" --destino "$T/regen-sucio" \
		--nombre "Autor Publico" --correo "1+autor@users.noreply.github.com" \
		>"$T/salida" 2>&1 || rc=$?
	cat "$T/salida" >>"$T/todas-las-salidas"
	espera_rc "regenerar con hallazgos" 1
	espera "regenerar con hallazgos lo dice" "RECHAZADO"
	grep -q "red.txt.*ipv4-privada" "$T/regen-sucio/hallazgos-historia.txt" && bien ||
		mal "regenerar con hallazgos: no deja el informe"
	[ -d "$T/regen-sucio/repo/.git" ] && bien || mal "regenerar con hallazgos: no deja la carpeta"
	for v in $PLANTADOS; do
		grep -qF -- "$v" "$T/todas-las-salidas" &&
			mal "regenerar enseña un valor plantado (empieza por $(printf '%.2s' "$v"))" || bien
	done
fi

if [ "$fallos" -ne 0 ]; then
	echo "prueba-publicar: $fallos fallos, $n_ok bien; se queda $T" >&2
	exit 1
fi
echo "prueba-publicar: ok ($n_ok comprobaciones)"
if [ "${SEMILLERO_PRUEBA_CONSERVAR:-0}" = 1 ]; then
	echo "prueba-publicar: se queda $T"
else
	rm -rf "$T"
fi
