# Hoja de ruta hacia la 1.0

semillero ya hace su trabajo: clasifica los torrents según las normas del
tracker, avisa antes de cada borrado y solo borra lo que ya cumplió. La 1.0
no trae una idea nueva; es la promesa de que lo que hay es estable: la
configuración, la base de datos y los presets no cambian sin aviso, y lo que
dicen las guías está probado en cada plataforma.

Las versiones intermedias son orientativas. Si algo de aquí te importa,
abre una incidencia y lo hablamos.

## 0.4: lo que ya sabemos que falla

Avisos de la auditoría de la 0.3.0 que no impiden usarlo, pero confunden:

- **Contraseña del servicio equivocada.** Hoy la web dice «qBittorrent no
  responde» y solo el registro dice la causa. La web tiene que decir que la
  contraseña no vale y cómo cambiarla (`semillero configurar`).
- **Ficheros de clave sueltos.** Al pasar del modo con contraseña a la lista
  de confianza, o al quitar Jellyfin, `semillero configurar` deja el fichero
  de clave antiguo. Tiene que ofrecer borrarlo.
- **Guías sin shell POSIX.** Las guías cargan el fichero de configuración con
  `set -a`; en fish o en PowerShell eso no vale. Cada guía tiene que dar la
  orden para su shell.
- **Origen de las versiones.** El actualizador de FreeBSD comprueba que lo
  que recibe está entero (SHA256SUMS), pero no quién lo publicó. Tiene que
  comprobar también la atestación de GitHub de cada versión.

## 0.5: probado donde se instala

Hoy las pruebas pasan en Linux y la compilación en Windows, pero las guías
de FreeBSD, systemd y Windows solo se han seguido a mano.

- **Prueba de punta a punta en la integración continua** con un qBittorrent
  de verdad en un contenedor, no solo con el simulador: entrar, clasificar,
  avisar, cancelar un aviso y borrar en modo real.
- **Una máquina FreeBSD en la integración continua** que siga la guía de
  FreeBSD con el script de arranque `rc.d`.
- **Windows**: arrancar el binario, `semillero configurar` y comprobar que la
  web contesta en `127.0.0.1`.
- **Versiones de qBittorrent**: probar contra las ramas 4.6 y 5.x y decir en
  el README cuáles valen.
- **Actualizar desde la versión anterior**: arrancar la última versión
  publicada con su configuración y su base de datos, cambiar al binario
  nuevo sin tocar nada y comprobar que arranca y no pierde datos.
- **Avisos del actualizador** que quedaron de la 0.4: el motivo de una
  descarga fallida, un fallo de `gh` que no es un rechazo y el segundo
  intento tras fallar la compilación.

## 0.6: más de un tracker

Hoy hay un solo juego de normas para todos los torrents. Quien siembra en
varios trackers privados con normas distintas tiene que elegir las más
estrictas.

- Normas por tracker, elegidas por el anuncio de cada torrent, con un juego
  por defecto para los que no encajan.
- Los presets guardan las normas de cada tracker; un preset de la 0.3 sigue
  cargando como el juego por defecto.
- **Añadir torrents desde semillero**: subir un `.torrent` o pegar un enlace
  magnet, con su categoría, sin pasar por la WebUI de qBittorrent.

## 1.0: estable

- **Contrato de configuración cerrado.** Las variables `SEMILLERO_*` y el
  formato del fichero que escribe `semillero configurar` no cambian hasta la
  2.0. Si alguna se deja de usar, sigue funcionando con un aviso en el
  registro durante al menos una versión.
- **Base de datos.** Cualquier versión 0.3 o posterior se actualiza a la 1.0
  sin pasos a mano; las migraciones tienen pruebas desde cada versión
  publicada.
- **Presets con versión.** El formato lleva su número; la 1.0 carga los
  presets de todas las versiones anteriores.
- **Versionado semántico** desde la 1.0, con un registro de cambios
  ([CHANGELOG.md](CHANGELOG.md)) que dice qué hay que hacer al actualizar.
- **Revisión de seguridad** de la entrada, la sesión y el borrado, y pruebas
  de propiedades en el crate `reglas`: ningún torrent que no haya cumplido
  sale nunca como «Listo para recoger».
- **Accesibilidad**: la web se puede usar entera con teclado y con lector de
  pantalla, y se lee bien en el móvil.

## Fuera de la 1.0

- Otros idiomas en la interfaz (la documentación sí está en español e
  inglés).
- Otros clientes de BitTorrent además de qBittorrent.
- Una página de inicio con otros servicios.
