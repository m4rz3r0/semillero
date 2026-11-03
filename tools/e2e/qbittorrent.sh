#!/bin/sh
# qBittorrent de usar y tirar para las pruebas `real` (prueba de punta a
# punta con un qBittorrent de verdad): la imagen oficial `qbittorrent-nox`
# en un contenedor, con la WebUI solo en 127.0.0.1 (en el mismo puerto
# dentro y fuera: qBittorrent rechaza un Host con otro puerto que el suyo)
# y una contraseña al azar
# puesta de antemano en su configuración (su hash PBKDF2, como lo guarda
# qBittorrent), así que no hay que leerla del registro.
#
# Uso:
#   qbittorrent.sh arrancar IMAGEN DIRECTORIO [PUERTO]
#   qbittorrent.sh parar
#
# `arrancar` deja en DIRECTORIO la configuración (config/), los datos que
# siembra (datos/, que en el contenedor es /downloads) y la contraseña
# (clave), espera a que la WebUI acepte la contraseña y escribe en la salida
# las variables de las pruebas (`NOMBRE=valor`, una por línea, como las
# quiere $GITHUB_ENV). `parar` enseña el registro del contenedor y lo quita.
#
# Usa docker; con CONTENEDOR=podman, podman. Necesita curl y python3.

set -eu

NOMBRE=semillero-e2e-qbt
CONTENEDOR=${CONTENEDOR:-docker}

fallo() {
    echo "qbittorrent.sh: $*" >&2
    exit 1
}

# PBKDF2-SHA512, 100 000 vueltas, sal de 16 bytes y clave de 64, en el
# formato de qBittorrent (4.2 y siguientes).
hash_pbkdf2() {
    python3 -I -c '
import base64, hashlib, os, sys
sal = os.urandom(16)
clave = hashlib.pbkdf2_hmac("sha512", sys.argv[1].encode(), sal, 100000, 64)
print("@ByteArray(%s:%s)" % (base64.b64encode(sal).decode(), base64.b64encode(clave).decode()))
' "$1"
}

# La WebUI acepta la contraseña: 204 desde la 5.2; antes, 200 con «Ok.».
entra() {
    codigo=$(curl -sS -o "$3" -w '%{http_code}' --data-urlencode username=admin \
        --data-urlencode "password=$2" "$1/api/v2/auth/login" 2>/dev/null) || return 1
    [ "$codigo" = 204 ] || { [ "$codigo" = 200 ] && [ "$(cat "$3")" = "Ok." ]; }
}

arrancar() {
    imagen=$1
    dir=$2
    puerto=${3:-18080}
    mkdir -p "$dir/config/qBittorrent/config" "$dir/datos"
    # El contenedor corre con otro usuario: tiene que poder borrar lo que
    # la prueba deja en datos/.
    chmod 0777 "$dir/datos"
    clave=$(od -An -N18 -tx1 /dev/urandom | tr -d ' \n')
    (umask 077 && printf '%s\n' "$clave" >"$dir/clave")
    hash=$(hash_pbkdf2 "$clave")
    # La 4.6 lee la WebUI en [Preferences] y la 5.3 en [WebUI]: van las
    # dos. Sin DHT, LSD ni PeX: los torrents de la prueba no salen a
    # ninguna red. Con la red del equipo (para llegar al tracker de mentira
    # de la prueba en 127.0.0.1), la WebUI y BitTorrent solo escuchan ahí.
    cat >"$dir/config/qBittorrent/config/qBittorrent.conf" <<EOF
[BitTorrent]
Session\\DefaultSavePath=/downloads
Session\\TempPathEnabled=false
Session\\DHTEnabled=false
Session\\LSDEnabled=false
Session\\PeXEnabled=false
Session\\Port=6881
Session\\Interface=lo
Session\\InterfaceName=lo
Session\\InterfaceAddress=127.0.0.1

[LegalNotice]
Accepted=true

[Meta]
MigrationVersion=9999

[Network]
PortForwardingEnabled=false

[Preferences]
Connection\\UPnP=false
WebUI\\Port=$puerto
WebUI\\Address=127.0.0.1
WebUI\\Username=admin
WebUI\\Password_PBKDF2="$hash"
WebUI\\LocalHostAuth=true
WebUI\\AuthSubnetWhitelistEnabled=false
WebUI\\MaxAuthenticationFailCount=0

[WebUI]
Port=$puerto
Address=127.0.0.1
Username=admin
Password_PBKDF2="$hash"
LocalHostAuth=true
AuthSubnetWhitelistEnabled=false
MaxAuthenticationFailCount=0
EOF
    "$CONTENEDOR" run -d --name "$NOMBRE" \
        -e QBT_LEGAL_NOTICE=confirm \
        -e "QBT_WEBUI_PORT=$puerto" \
        --network host \
        -v "$dir/config:/config:Z" \
        -v "$dir/datos:/downloads:Z" \
        "$imagen" >/dev/null
    url="http://127.0.0.1:$puerto"
    i=0
    until entra "$url" "$clave" "$dir/login"; do
        i=$((i + 1))
        if [ "$i" -ge 60 ]; then
            "$CONTENEDOR" logs "$NOMBRE" >&2 || true
            fallo "la WebUI de $url no acepta la contraseña tras 60 s"
        fi
        sleep 1
    done
    echo "SEMILLERO_QBT_REAL=$url"
    echo "SEMILLERO_QBT_REAL_CLAVE=$dir/clave"
    echo "SEMILLERO_QBT_REAL_DATOS=$dir/datos"
}

parar() {
    "$CONTENEDOR" logs "$NOMBRE" 2>&1 || true
    "$CONTENEDOR" rm -f "$NOMBRE" >/dev/null 2>&1 || true
}

case "${1:-}" in
arrancar)
    [ $# -ge 3 ] || fallo "uso: qbittorrent.sh arrancar IMAGEN DIRECTORIO [PUERTO]"
    arrancar "$2" "$3" "${4:-}"
    ;;
parar) parar ;;
*) fallo "uso: qbittorrent.sh arrancar IMAGEN DIRECTORIO [PUERTO] | parar" ;;
esac
