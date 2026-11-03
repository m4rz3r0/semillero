# Diseño de semillero

Qué hace semillero, cómo decide y cómo está hecho. Para instalarlo, ver el
[README](../README.md).

## Objetivo

Una web para ver los torrents de qBittorrent con su estado frente a las normas
del tracker, gestionarlos de forma sencilla y **borrar automáticamente** lo que
ya cumplió, sin arriesgar nunca un Hit & Run.

Petición original: «debe dejar configurar reglas de borrado automático, mostrar
los torrents con su estado y permitir una gestión sencilla».

### Criterios de éxito

- Ningún torrent Obligado o En espera se borra jamás, ni a mano ni por regla.
- Ningún borrado automático ocurre sin un aviso visible (24 h por defecto) que se puede cancelar.
- Si se vigilan la VPN y el puerto y fallan, no avanza ninguna cuenta atrás ni se borra nada.
- Al estrenarlo, las reglas solo avisan; el borrado real se activa a mano.
- Tiene identidad visual propia («Vivero», abajo).

## Decisiones tomadas

| Tema | Decisión |
|---|---|
| Qué borra una regla | El torrent y sus ficheros; si hay Jellyfin, le pide reescanear |
| Tipos de regla | Ratio, tiempo sembrando, inactividad y espacio en disco; los cuatro, por categoría |
| Al cumplirse una regla | Aviso de 24 h con botón «No borrar» |
| Arranque | Modo «solo aviso»; un interruptor activa el borrado real |
| VPN o puerto caídos | Se congelan cuentas atrás y borrados |
| Acceso | Cuenta de qBittorrent; semillero no guarda contraseñas |
| Tecnología | Leptos 0.8 sobre axum 0.8, compilado con `cargo-leptos` |
| Aspecto | «Vivero» (abajo) |

## Clasificación según las normas del tracker

Una función pura en el crate `reglas`, con las **normas del tracker**
(`NormasTracker`) como datos. Se guardan en la tabla `normas` (una fila) y se
cambian en la página de reglas o aplicando un preset. Los valores por defecto
son los de un tracker privado habitual:

| Norma | Campo | Por defecto | Rango |
|---|---|---|---|
| Obliga a sembrar desde | `umbral_pct` | 10 % descargado | 0–100 |
| Días de siembra | `siembra_dias` | 7 | 0–365 |
| Ratio mínima | `ratio_min` | 1.0 | 0–100 |
| Plazo máximo | `plazo_largo_dias` | 30 | 0–365, ≥ días de siembra |
| Ratio para parar | `ratio_pausa` | 2.0 | 0–100, ≥ ratio mínima |
| Aviso antes de recoger | `aviso_horas` | 24 | 1–720 |

- **Libre**: `downloaded < umbral % de size`.
- **Obligado**: no es libre y `seeding_time < días de siembra`.
- **En espera**: con los días de siembra, ratio < mínima y antes del plazo máximo.
- **Cumplido**: con los días de siembra y (ratio ≥ mínima o plazo máximo cumplido).
- `total_size ≤ 0` con `downloaded > 0` nunca es Libre; después se clasifica como cualquier otro.

Pausa automática con los días de siembra y la ratio para parar: nunca borra.
Los rangos se comprueban al recibirlos y otra vez en la base de datos (al
guardar y con `CHECK` en la tabla). La migración que crea la tabla la llena
con los valores por defecto: una base de datos ya en uso sigue igual.

## Reglas de borrado

Una tarjeta por categoría (`peliculas`, `series`, sin categoría). Cada regla es
un interruptor y un valor; sin formularios largos:

| Regla | Se cumple cuando | Valor por defecto |
|---|---|---|
| Ratio | `ratio ≥ X` | 2.0 |
| Tiempo sembrando | `seeding_time ≥ N días` | 30 |
| Inactividad | sin subir nada en `N días` | 14 |
| Espacio en disco | libre en el dataset `< X %`; se eligen candidatos del más antiguo al más nuevo hasta volver a pasar de X % | 10 % |

Sobre esas reglas:

- Solo afectan a torrents **Cumplidos o Libres**. Antes de borrar, el servidor
  vuelve a leer el torrent y a clasificarlo.
- Todas empiezan desactivadas.
- La inactividad se mide con el contador de subida del torrent, que semillero guarda una vez por hora.

### Ciclo de un aviso

1. Cada hora, cada torrent que cumple una regla pasa a **Aviso**, con su hora de borrado (ahora + las horas de aviso de las normas) y la regla que lo causó.
2. En la web aparece en «A punto de borrarse», con cuenta atrás y «No borrar».
3. «No borrar» lo exime de esa regla para siempre; se puede deshacer en su ficha.
4. Al vencer:
   - en modo «solo aviso», se apunta «se habría borrado» y no se borra nada;
   - en modo real, se borra.
5. Si deja de cumplir la regla antes de vencer, el aviso desaparece.
6. **Congelación:** si la VPN o el puerto fallan, los avisos pendientes suman el
   tiempo de fallo a su hora de borrado. No vencen hasta cumplir sus horas de
   aviso *efectivas* de funcionamiento correcto.

Cada aviso, cancelación y borrado se apunta en el historial, que se ve en la
web y está guardado en SQLite.

### Presets

Un preset guarda juntas las normas del tracker y las reglas de las tres
categorías, con un nombre y una descripción. En la página de reglas, sección
«Presets»:

- **Guardar como preset**: la configuración de ahora, con nombre y descripción.
- **Subir preset**: un archivo `.toml` de otra persona; se añade a la lista, sin aplicarlo.
- En cada uno: **Aplicar** (con confirmación: sus normas y reglas sustituyen a
  las de ahora; las categorías que no trae quedan apagadas; se apunta en el
  historial), **Descargar** (el archivo, para compartirlo), **Cambiar**
  (nombre, descripción y, si se marca, poner en él la configuración de ahora)
  y **Eliminar** (con confirmación).
- Hay uno incluido, «Por defecto»: normas por defecto y ninguna regla. Se
  aplica y se descarga; no se cambia ni se elimina.

El archivo (formato 1):

```toml
formato = 1
nombre = "Tracker exigente"
descripcion = "Siembra 10 días y ratio 1,5."

[normas]
umbral_pct = 10.0
siembra_dias = 10
ratio_min = 1.5
plazo_largo_dias = 40
ratio_pausa = 3.0
aviso_horas = 24

[categorias.peliculas]
ratio = 2.0
siembra_dias = 30

[categorias.sin_categoria]
disco_libre_pct = 10.0
```

- Las categorías son `peliculas`, `series` y `sin_categoria`; las reglas son
  `ratio`, `siembra_dias`, `inactividad_dias` y `disco_libre_pct`, con los
  rangos de las tarjetas. Una regla que no está, está apagada.
- Se rechaza entero, sin guardar nada y con una frase que dice qué falla: un
  `formato` que no sea 1, cualquier clave de más o de menos, un tipo o un valor
  fuera de rango, un nombre vacío o de más de 60 caracteres, una descripción de
  más de 300, más de 16 categorías, una categoría que semillero no tiene, un
  nombre ya usado o un archivo de más de 64 KiB (se mira antes de leerlo, en el
  navegador y en el servidor).
- Como mucho 50 presets guardados. Se guardan en la tabla `presets` (nombre
  único sin mirar mayúsculas, descripción y el archivo tal cual se descarga) y
  se vuelven a comprobar al leerlos.
- Todas las acciones piden la sesión y el token, como las demás; la descarga
  es un `GET` con la sesión (`/api/descargar_preset?id=N`) que devuelve el
  archivo como adjunto, con un nombre de archivo hecho solo de minúsculas,
  cifras y guiones.

## Interfaz

- **Franja de estado:** VPN, puerto abierto y disco libre por dataset. Solo destaca lo que falla.
- **Pantalla principal:**
  - grupos Obligados, En espera, Cumplidos y Libres;
  - cada torrent muestra nombre, barra de ratio, tiempo sembrado y qué le falta;
  - acciones sencillas: pausar o reanudar; «Quitar» y «Borrar con ficheros» solo en Cumplidos y Libres, con confirmación.
- **A punto de borrarse:** lista con cuenta atrás, regla causante y «No borrar».
- **Reglas:** el interruptor global «Solo aviso / Borrado real», las normas del tracker, las tarjetas por categoría y los presets.
- **Historial.**
- Lenguaje claro, sin nombres de la API de qBittorrent.
- Solo en español.

### Identidad «Vivero»

Grupos «Recién sembrados», «Creciendo», «Listos para recoger» y «Sin
arraigar», sin tarjetas ni bordes de color.

| | semillero |
|---|---|
| Acento | Verde musgo `#4f9d69`; brote, trigo y remolacha |
| Fondo claro | Salvia claro `#eef2ea` |
| Títulos | Alegreya |
| Texto | Alegreya Sans |
| Cifras | Alegreya Sans, cifras tabulares |

Técnica:

- variables CSS para acento, velo, borde y texto, con tema claro y oscuro (`data-tema`);
- componentes sencillos: avisos, botones y diálogo de confirmación;
- CSS escrito a mano, servido con `style-src 'self'`;
- las fuentes, alojadas en el propio servidor.

## Arquitectura

Espacio de trabajo Cargo (Leptos 0.8, axum 0.8, `cargo-leptos`, la Rust
fijada en `rust-toolchain.toml`):

- `crates/reglas`: normas del tracker, clasificación, evaluación de reglas y archivos de preset. Puro, sin E/S, compila también a wasm.
- `crates/qbt`:
  - cliente de la API v2 de qBittorrent 5.x;
  - dos modos: con contraseña (entra con su propia cuenta, de
    `SEMILLERO_QBT_CLAVE`, guarda la cookie y repite una vez tras un 403) o
    lista blanca (habla desde una IP de la lista y comprueba contraseñas
    desde otra que no lo está, `SEMILLERO_LOGIN_DESDE`); las contraseñas de
    quien entra se comprueban siempre sin la sesión de semillero;
  - valida los hashes;
  - operaciones de parar, reanudar y borrar.
- `crates/jellyfin`: tras borrar, pide a Jellyfin reescanear la biblioteca
  (`POST /Library/Refresh`); Jellyfin quita lo que ya no está en el disco. Sin
  `SEMILLERO_JELLYFIN` no hace nada.
- `crates/qbt-sim`: qBittorrent falso con axum para las pruebas (204/401 en
  el login, como 5.2) y para la demo (`cargo run -p qbt-sim --example demo`).
- `crates/semillero`:
  - SSR con hidratación;
  - SQLite (sqlx) para sesiones, normas del tracker, reglas, presets, avisos, exenciones, historial y muestras de subida;
  - tarea horaria;
  - lectura del fichero de estado de la VPN y del puerto (`SEMILLERO_ESTADO`), si lo hay;
  - `semillero configurar`: pregunta direcciones y claves, las comprueba y escribe el fichero `VARIABLE=valor` y los secretos aparte (0600).

### Seguridad

- **Login:**
  - las credenciales se comprueban contra qBittorrent;
  - la sesión se guarda en SQLite con id SHA-256 y cookie `__Host-sm_session`;
  - hay un freno de intentos fallidos por dirección del cliente (en IPv6, por red /64).
- **Borrados:** borrar a mano y activar «Borrado real» piden reconfirmar la contraseña.
- **Peticiones:**
  - CSRF estricto (Origin y token);
  - CSP con nonce;
  - un solo guardián para todo `/api`;
  - límite de tamaño de las peticiones.
- **Ejecución:** escucha en `127.0.0.1:8090` por defecto. Para entrar desde
  otro equipo, detrás de un proxy inverso con HTTPS (`SEMILLERO_ORIGEN`,
  `SEMILLERO_PROXY`). Más en [SECURITY.md](../SECURITY.md).

## Compilación y despliegue

- Cada versión es una etiqueta `vX.Y.Z` con la misma versión en
  `crates/semillero/Cargo.toml` (la que dice `/salud` a las peticiones
  locales).
- La CI (`.github/workflows/release.yml`) compila con cada etiqueta los
  paquetes de Linux (x86_64, aarch64), Windows (x86_64) y FreeBSD (x86_64),
  la imagen de contenedor y `SHA256SUMS`, con atestaciones de procedencia.
  Paquete: el binario, `site/`, `COMMIT`, la licencia y lo propio de cada
  sistema (`packaging/empaquetar.sh`).
- Formas de instalarlo: [docs/instalar/](instalar/qbittorrent.md).
- En FreeBSD hay además un actualizador (`deploy/freebsd/actualizar.sh`), un
  cron de root cada 15 minutos en el host:
  1. lee las etiquetas con `git ls-remote` desde el host (sin clonar ni
     ejecutar nada del repositorio; por ssh, con una clave de despliegue de
     solo lectura y la clave del servidor git fijada) y se queda con la
     `vX.Y.Z` más alta y su commit;
  2. la toma de la release de Forgejo de la etiqueta, que publica un runner
     de Forgejo Actions en un PC (`.forgejo/workflows/freebsd.yml` y
     `deploy/cross/`), con un token de solo lectura que va a curl por la
     entrada estándar, comprobando sha256, ficheros normales, el commit de
     la etiqueta y que sea un ejecutable de FreeBSD; o, si no, de la release
     de GitHub, con su atestación de procedencia;
  3. el servidor no compila nunca: si ninguna está o cuadra, espera y lo
     vuelve a mirar en cada pasada, sin contarlo como fallo;
  4. la deja en `/usr/local/semillero/releases/X.Y.Z` en el host, montado en
     solo lectura (nullfs) en la jail del servicio;
  5. para el servicio, copia la base de datos, cambia `current` de golpe y
     arranca;
  6. espera a que `/salud` diga `"ok":true` con la versión nueva y a que el que
     escucha sea su binario;
  7. si no, vuelve a la anterior con la base de datos de antes. Tras dos fallos
     seguidos, la etiqueta queda como fallida. Se guardan las tres últimas.

  Se instala con `deploy/freebsd/install.sh`; los pasos están en
  [deploy/README.md](../deploy/README.md).

## Pruebas

- `reglas`: clasificación, cada tipo de regla, el
  orden por antigüedad en la regla de disco, la congelación y que nunca se elige un Obligado ni un En espera.
- `qbt` contra `qbt-sim`: login 204/401, cookie SID, hash no válido, reintento.
- `semillero`, de extremo a extremo con `qbt-sim`:
  - el aviso aparece;
  - «No borrar» lo cancela;
  - en «solo aviso» no se borra nada;
  - en «borrado real» se borra al vencer;
  - con la VPN caída no vence;
  - el login y el CSRF rechazan lo que deben.

## Fuera de alcance

- Página de inicio de servicios.
- Otros idiomas en la interfaz.
