# Seguridad

## Avisar de un fallo

No abras una incidencia pública. Usa el aviso privado de GitHub: en la
pestaña **Security** del repositorio, **Report a vulnerability**
(*private vulnerability reporting*). Así solo lo ven quienes mantienen el
proyecto hasta que haya arreglo.

Cuenta qué versión usas, cómo se reproduce y qué consigue un atacante. No
mandes datos tuyos de verdad (contraseñas, direcciones, nombres de
torrents): no hacen falta.

## Versiones con arreglos

Solo la última versión publicada. Los arreglos salen en una versión nueva.

## Qué protege semillero

- **Contraseñas.** semillero no guarda ninguna. Al entrar, comprueba tu
  usuario y tu contraseña contra el login de qBittorrent. Al arrancar hace
  una prueba con una contraseña al azar: si qBittorrent la acepta, cualquier
  contraseña valdría, y semillero cierra el acceso hasta que lo arregles.
- **Sesiones.** Cookie `__Host-` con `Secure`, `HttpOnly` y `SameSite`; en
  la base de datos solo se guarda su hash SHA-256.
- **Peticiones.** Toda acción comprueba la sesión, la cabecera `Origin`
  (frente a `SEMILLERO_ORIGEN`) y un token CSRF.
- **Cabeceras.** CSP con nonce, `X-Frame-Options`, `Referrer-Policy` y
  similares. HSTS solo si el origen es `https`.
- **Freno de intentos.** Los fallos de acceso se cuentan por dirección del
  cliente (en IPv6, por red /64), no por usuario: nadie puede bloquear tu
  cuenta desde fuera.
- **Tamaños.** Hay límite en todo lo que se lee (peticiones, respuestas de
  qBittorrent, presets, ficheros) y en lo que se guarda.
- **Borrados.** Activar el borrado real, borrar a mano y aplicar un preset
  con el borrado real activo piden otra vez la contraseña.
- **Lo que se ve desde fuera.** `/salud` solo dice la versión a quien
  pregunta desde el propio equipo, sin proxy por medio. Los registros no
  llevan contraseñas, tokens, usuarios ni direcciones.

## Qué da por hecho

- **Acceso remoto, detrás de un proxy con TLS.** semillero habla HTTP. Por
  defecto solo escucha en `127.0.0.1`. Para entrar desde otro equipo, ponlo
  detrás de un proxy inverso con HTTPS y pon su dirección en
  `SEMILLERO_ORIGEN`.
- **`SEMILLERO_PROXY` es solo ese proxy.** semillero cree la IP del cliente
  que dice `X-Forwarded-For` solo si la conexión viene de esa dirección. Si
  pones otra, cualquiera podría inventarse su IP y saltarse el freno de
  intentos. El proxy tiene que añadir la IP real al final de esa cabecera.
- **qBittorrent bien configurado.** Con el modo con contraseña
  (`SEMILLERO_QBT_CLAVE`), semillero entra con su propia cuenta y no hace
  falta lista blanca. Con el modo lista blanca, la dirección de
  `SEMILLERO_LOGIN_DESDE` no puede estar en ella. Ver
  [Preparar qBittorrent](docs/instalar/qbittorrent.md).
- **El equipo es tuyo.** Quien pueda leer la base de datos o los ficheros
  de las claves (la contraseña de qBittorrent y la clave de Jellyfin) puede
  ver sesiones abiertas y entrar en esos servicios. `semillero configurar`
  guarda cada clave en su fichero, solo para su dueño (0600), y la base de
  datos se crea igual; las guías de instalación los dejan solo para el
  usuario del servicio.

Ninguna de estas defensas depende de que el código sea secreto.
