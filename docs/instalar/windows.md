# Windows

Dos formas: el `.zip` de la versión, a mano (la recomendada), o la imagen
con podman. En las dos, la web queda en `http://127.0.0.1:8090`, solo en
este equipo. Para Windows 10 u 11 de 64 bits (x86_64).

Antes: [Preparar qBittorrent](qbittorrent.md).

Cambia `X.Y.Z` por la versión. Las órdenes son de PowerShell.

## A mano, con el zip

### 1. Descargar y comprobar

```powershell
$V = 'X.Y.Z'
$P = "semillero-$V-x86_64-pc-windows-msvc"
cd $env:USERPROFILE\Downloads
Invoke-WebRequest "https://github.com/m4rz3r0/semillero/releases/download/v$V/$P.zip" -OutFile "$P.zip"
Invoke-WebRequest "https://github.com/m4rz3r0/semillero/releases/download/v$V/SHA256SUMS" -OutFile SHA256SUMS
(Get-FileHash ".\$P.zip").Hash.ToLower()
Select-String x86_64-pc-windows-msvc .\SHA256SUMS
```

Los dos hash tienen que ser iguales. Con la
[CLI de GitHub](https://github.com/cli/cli) compruebas además que el zip
salió de la CI del repositorio:

```powershell
gh attestation verify ".\$P.zip" --repo m4rz3r0/semillero
```

### 2. Descomprimir

```powershell
Expand-Archive ".\$P.zip" -DestinationPath C:\
Rename-Item "C:\$P" C:\semillero
```

Dentro están `semillero.exe`, `semillero.cmd` (lo arranca),
`semillero.env.example`, la web (`site\`) y `LEEME-windows.md`.

### 3. Configurar

```powershell
cd C:\semillero
.\semillero.exe configurar
```

Pregunta dónde escucha, la dirección con la que lo abres en el navegador,
la de qBittorrent y cómo entra en él (ver
[Preparar qBittorrent](qbittorrent.md); con qBittorrent en este Windows,
cualquiera de los dos modos; si no, el modo con contraseña), el proxy,
Jellyfin, el fichero de estado de la VPN y la base de datos (con Intro,
`semillero.cmd` la deja en `datos\`). Intro deja el valor propuesto y las
contraseñas no se ven al escribirlas. Antes de guardar comprueba
qBittorrent (y Jellyfin) y dice qué falla; si algo falla (por ejemplo,
qBittorrent aún no está en marcha), pregunta si guardar de todos modos.

Escribe `semillero.env` en esa carpeta y la contraseña de qBittorrent y la
clave de Jellyfin en `qbt.clave` y `jellyfin.clave`, junto a él. Los
ficheros heredan los permisos de la carpeta: si otros usuarios usan este
Windows, deja la carpeta solo para el tuyo:

```powershell
icacls C:\semillero /inheritance:r /grant:r "${env:USERNAME}:(OI)(CI)F" SYSTEM:(OI)(CI)F Administradores:(OI)(CI)F
```

(`Administrators` en un Windows en inglés.) Para cambiar algo, vuelve a
ejecutarlo: solo cambia lo que cambies. Las demás variables están en
`semillero.env.example` y en la
[tabla del README](../../README.md#configuración).

### 4. Arrancar

Doble clic en `C:\semillero\semillero.cmd`, o desde una consola:

```powershell
C:\semillero\semillero.cmd
```

La ventana tiene que decir `semillero escuchando en http://127.0.0.1:8090`
y `canario de acceso: qBittorrent rechaza una contraseña al azar`. Abre
`http://127.0.0.1:8090` y entra con el usuario y la contraseña de
qBittorrent. Para pararlo, `Ctrl+C` en esa ventana.

Si Windows pregunta por el cortafuegos, no hace falta abrir nada mientras
escuche en `127.0.0.1`.

### Dónde queda cada cosa

| Qué | Dónde |
|---|---|
| Programa y web | `C:\semillero` |
| Configuración | `C:\semillero\semillero.env` |
| Contraseña de qBittorrent, clave de Jellyfin | `C:\semillero\qbt.clave`, `C:\semillero\jellyfin.clave` |
| Base de datos | `C:\semillero\datos\semillero.db` |

### Actualizar

Descomprime la versión nueva en otra carpeta, copia en ella
`semillero.env`, los `.clave` y `datos\` de la anterior y arranca desde la
nueva. Si la carpeta cambia de nombre, vuelve a ejecutar
`semillero configurar` para que apunte a los ficheros nuevos.

## Con podman

Con [Podman Desktop](https://github.com/containers/podman-desktop) o la CLI
de podman ya instalada, en una carpeta para la configuración (por ejemplo
`C:\semillero`):

```powershell
podman machine init
podman machine start
podman pull ghcr.io/m4rz3r0/semillero:X.Y.Z
gh attestation verify oci://ghcr.io/m4rz3r0/semillero:X.Y.Z --repo m4rz3r0/semillero
cd C:\semillero
podman volume create --ignore semillero-datos
podman run --rm -it --user 0 -v semillero-datos:/data -v "${PWD}:/config" `
  ghcr.io/m4rz3r0/semillero:X.Y.Z configurar --env /config/semillero.env --secretos /data
podman run -d --init --name semillero --restart=unless-stopped `
  -p 127.0.0.1:8090:8090 -v semillero-datos:/data `
  --env-file .\semillero.env ghcr.io/m4rz3r0/semillero:X.Y.Z
```

El nombre de la imagen va en minúsculas. `semillero configurar` escribe
`semillero.env` en la carpeta y deja la contraseña de qBittorrent en el
volumen. En la máquina virtual de podman, semillero sale hacia qBittorrent
con una sola dirección: usa el
[modo con contraseña](qbittorrent.md#modo-con-contraseña-recomendado). Como
dirección de qBittorrent, la IP de este Windows en la red local (dentro del
contenedor, `127.0.0.1` es el propio contenedor).
