# A mano, en cualquier sistema

Sin servicio ni contenedor: descargas, descomprimes y lo arrancas desde una
terminal. Sirve para probarlo con tu qBittorrent o para un sistema sin guía
propia. Para dejarlo como servicio, mira la guía de tu sistema en el
[README](../../README.md#instalar).

Antes: [Preparar qBittorrent](qbittorrent.md).

Cambia `X.Y.Z` por la versión. Las órdenes son para `sh`, `bash` o `zsh`;
en fish, escribe `set V X.Y.Z` en lugar de `V=X.Y.Z` (y lo mismo con `P`).
Arrancar semillero es igual en cualquier shell, también en PowerShell:
`semillero --env semillero.env` lee él mismo la configuración.

## Con un paquete de la versión

Hay paquetes para:

| Sistema | Paquete |
|---|---|
| Linux x86_64 | `semillero-X.Y.Z-x86_64-unknown-linux-gnu.tar.gz` |
| Linux aarch64 | `semillero-X.Y.Z-aarch64-unknown-linux-gnu.tar.gz` |
| FreeBSD x86_64 | `semillero-X.Y.Z-x86_64-unknown-freebsd.tar.gz` |
| Windows x86_64 | `semillero-X.Y.Z-x86_64-pc-windows-msvc.zip` (ver [Windows](windows.md)) |

### 1. Descargar y comprobar

En Linux:

```sh
V=X.Y.Z
P=semillero-$V-x86_64-unknown-linux-gnu
curl -fLO "https://github.com/m4rz3r0/semillero/releases/download/v$V/$P.tar.gz"
curl -fLO "https://github.com/m4rz3r0/semillero/releases/download/v$V/SHA256SUMS"
sha256sum -c SHA256SUMS --ignore-missing
```

En FreeBSD:

```sh
V=X.Y.Z
P=semillero-$V-x86_64-unknown-freebsd
fetch "https://github.com/m4rz3r0/semillero/releases/download/v$V/$P.tar.gz"
fetch "https://github.com/m4rz3r0/semillero/releases/download/v$V/SHA256SUMS"
sha256 -c "$(awk -v f="$P.tar.gz" '$2 == f { print $1 }' SHA256SUMS)" "$P.tar.gz"
```

Con la [CLI de GitHub](https://github.com/cli/cli) compruebas además que
salió de la CI del repositorio:

```sh
gh attestation verify "$P.tar.gz" --repo m4rz3r0/semillero
```

### 2. Descomprimir y arrancar

```sh
tar -xzf "$P.tar.gz"
cd "$P"
./semillero configurar
./semillero --env semillero.env
```

`semillero configurar` pregunta las direcciones (la de qBittorrent y cómo
entra en él: ver [Preparar qBittorrent](qbittorrent.md)) y las claves, las
comprueba y escribe `semillero.env` en la carpeta, con la contraseña de
qBittorrent aparte, en `qbt.clave`, solo para ti. Intro deja el valor
propuesto y las contraseñas no se ven al escribirlas. La web (`site/`) está
junto al ejecutable y semillero la encuentra sola.

En la salida tiene que aparecer `semillero escuchando en
http://127.0.0.1:8090` y `canario de acceso: qBittorrent rechaza una
contraseña al azar`. Abre `http://127.0.0.1:8090` en este equipo y entra
con el usuario y la contraseña de qBittorrent. Para pararlo, `Ctrl+C`.

### Cambiar la configuración

Vuelve a ejecutar `./semillero configurar`: propone lo que ya hay y solo
cambia lo que cambies. También puedes editar `semillero.env` a mano (una
variable por línea, `VARIABLE=valor`, sin comillas); las variables están
en la [tabla del README](../../README.md#configuración). Si una variable ya
está puesta en el entorno, manda sobre la del fichero. Si prefieres cargar
el fichero tú, en `sh` vale `set -a; . ./semillero.env; set +a` antes de
`./semillero`.

## Compilado desde el código

Para un sistema sin paquete (o para compilarlo tú). Hace falta Rust
(`rustup`); la versión la fija `rust-toolchain.toml`.

```sh
git clone https://github.com/m4rz3r0/semillero.git
cd semillero
git checkout vX.Y.Z
cargo install --locked cargo-leptos --version 0.3.11
cargo leptos build --release --precompress
./target/release/semillero configurar
echo LEPTOS_SITE_ROOT=target/site >> semillero.env
./target/release/semillero --env semillero.env
```

Lo que hace falta para llevarlo a otra carpeta: `target/release/semillero`
y `target/site/` (renombrada a `site/`, junto al ejecutable).
