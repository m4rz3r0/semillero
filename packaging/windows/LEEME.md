# semillero en Windows

Dos formas: a mano, con el `.zip` de la versión, o con podman y la imagen.
En las dos, la web queda en `http://127.0.0.1:8090` y solo se ve desde este
equipo. Cambia `X.Y.Z` por la versión.

## A mano

1. Descarga de la página de la versión
   (`https://github.com/m4rz3r0/semillero/releases`)
   `semillero-X.Y.Z-x86_64-pc-windows-msvc.zip` y `SHA256SUMS`.
2. Comprueba la descarga en PowerShell. El hash tiene que ser el de su línea
   en `SHA256SUMS`:

   ```powershell
   (Get-FileHash .\semillero-X.Y.Z-x86_64-pc-windows-msvc.zip).Hash.ToLower()
   Select-String x86_64-pc-windows-msvc .\SHA256SUMS
   ```

   Con la [CLI de GitHub](https://github.com/cli/cli) puedes comprobar
   además que salió de la CI del repositorio:
   `gh attestation verify .\semillero-X.Y.Z-x86_64-pc-windows-msvc.zip --repo m4rz3r0/semillero`.
3. Descomprime el `.zip` donde quieras, por ejemplo en `C:\semillero`.
4. En esa carpeta, `.\semillero.exe configurar`: pregunta las direcciones
   y las claves, las comprueba y escribe `semillero.env`, con la contraseña
   de qBittorrent aparte, en `qbt.clave`. Intro deja el valor propuesto.
   Para cambiar algo, vuelve a ejecutarlo.
5. Doble clic en `semillero.cmd`, o desde una consola:
   `C:\semillero\semillero.cmd`. Para pararlo, `Ctrl+C`.

La base de datos queda en `datos\semillero.db`, junto a `semillero.cmd`.
Para actualizar, descomprime la versión nueva en otra carpeta y copia
`semillero.env`, los `.clave` y `datos\` (si cambia la carpeta, vuelve a
ejecutar `semillero configurar`).

Si Windows pregunta por el cortafuegos, no hace falta abrir nada mientras
escuche en `127.0.0.1`.

## Con podman

Con [Podman Desktop](https://github.com/containers/podman-desktop) o la CLI
de podman ya instalada:

```powershell
podman machine init
podman machine start
podman pull ghcr.io/m4rz3r0/semillero:X.Y.Z
podman volume create --ignore semillero-datos
podman run --rm -it --user 0 -v semillero-datos:/data -v "${PWD}:/config" `
  ghcr.io/m4rz3r0/semillero:X.Y.Z configurar --env /config/semillero.env --secretos /data
podman run -d --init --name semillero --restart=unless-stopped `
  -p 127.0.0.1:8090:8090 -v semillero-datos:/data `
  --env-file .\semillero.env ghcr.io/m4rz3r0/semillero:X.Y.Z
```

`semillero configurar` escribe `semillero.env` en la carpeta actual y la
contraseña de qBittorrent en el volumen. En podman, usa el modo con
contraseña. Dentro del contenedor, `127.0.0.1` es el propio contenedor:
como dirección de qBittorrent, la IP del equipo donde corre.

Para comprobar la imagen:
`gh attestation verify oci://ghcr.io/m4rz3r0/semillero:X.Y.Z --repo m4rz3r0/semillero`.
