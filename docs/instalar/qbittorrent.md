# Preparar qBittorrent

Vale para todas las formas de instalar. Hace falta qBittorrent 5 con la
interfaz web activa (*Herramientas → Opciones → Interfaz web*).

semillero no guarda las contraseñas de quien entra: las comprueba contra
qBittorrent. Para leer los torrents, pararlos y borrarlos, habla con
qBittorrent de una de estas dos maneras:

| Modo | Cómo entra semillero en qBittorrent | Cuándo |
|---|---|---|
| **Con contraseña** (recomendado) | con su propio usuario y contraseña, guardada en un fichero (`SEMILLERO_QBT_CLAVE`) | siempre vale: mismo equipo, otro equipo, podman, Windows |
| **Lista blanca** | sin contraseña, desde una IP de la lista blanca de qBittorrent | semillero y qBittorrent en el mismo equipo, o una jail con dos IP |

En los dos, semillero comprueba al arrancar que qBittorrent rechaza una
contraseña al azar. Si la acepta, cualquier contraseña valdría: cierra el
acceso y lo dice en el registro:

```text
qBittorrent ha aceptado un usuario y una contraseña al azar desde …
```

Si todo va bien, el registro dice:

```text
canario de acceso: qBittorrent rechaza una contraseña al azar
```

## Modo con contraseña (recomendado)

1. En *Opciones → Interfaz web*:
   - deja qBittorrent escuchando donde semillero llegue (`*`, o la IP del
     equipo), puerto `8080`;
   - **desmarca** «Saltar la autenticación para clientes en localhost»
     (*Bypass authentication for clients on localhost*);
   - **desmarca** «Saltar la autenticación para clientes en las subredes de
     la lista blanca» (*… in whitelisted IP subnets*), o deja fuera de la
     lista la IP desde la que llega semillero.
2. En la configuración de semillero, la dirección de qBittorrent, el
   usuario de la interfaz web y el fichero con su contraseña:

   ```sh
   SEMILLERO_QBT=http://192.0.2.10:8080
   SEMILLERO_QBT_USUARIO=admin
   SEMILLERO_QBT_CLAVE=/ruta/a/qbt.clave
   ```

   El fichero tiene solo la contraseña (el salto de línea del final no
   cuenta) y tiene que poder leerlo el usuario con el que corre semillero, y
   nadie más (`chmod 600`). Si otros pueden leerlo, el registro lo avisa.

semillero entra con esa cuenta cuando qBittorrent se lo pide y guarda la
sesión. Las contraseñas de quien entra en la web se comprueban aparte, sin
esa sesión, así que una contraseña mala sigue siendo mala.

## Modo lista blanca

semillero no guarda ninguna contraseña, pero tiene que hablar con
qBittorrent desde dos direcciones que qBittorrent vea distintas:

| Para qué | Desde dónde | En qBittorrent |
|---|---|---|
| Leer torrents, pararlos, borrarlos | la dirección que el sistema elige para llegar a `SEMILLERO_QBT` | **en** la lista blanca (entra sin contraseña) |
| Comprobar tu contraseña al entrar | `SEMILLERO_LOGIN_DESDE` (por defecto `127.0.0.1`) | **fuera** de la lista blanca |

Por eso qBittorrent tiene que estar en el mismo equipo (o en una jail con
dos IP). Si no lo está y falta `SEMILLERO_LOGIN_DESDE`, `semillero
configurar` lo avisa. Detrás de la NAT de un contenedor las dos salen por la misma
dirección: ahí, el modo con contraseña.

### En el mismo equipo

semillero y qBittorrent en el mismo Linux o Windows. En el ejemplo, el
equipo tiene la IP `192.0.2.10` en la red local.

1. En *Opciones → Interfaz web*:
   - deja qBittorrent escuchando en todas las direcciones (`*`) o en
     `192.0.2.10`, puerto `8080`;
   - **desmarca** «Saltar la autenticación para clientes en localhost»;
   - **marca** «Saltar la autenticación para clientes en las subredes de la
     lista blanca» y pon solo `192.0.2.10/32`.
2. En la configuración de semillero:

   ```sh
   SEMILLERO_QBT=http://192.0.2.10:8080
   ```

   `SEMILLERO_LOGIN_DESDE` se queda en `127.0.0.1`.

Así semillero lee los torrents desde `192.0.2.10` (en la lista blanca) y
comprueba contraseñas desde `127.0.0.1` (fuera de ella).

No uses `SEMILLERO_QBT=http://127.0.0.1:8080`: las dos cosas saldrían de
`127.0.0.1` y no se puede tener dentro y fuera de la lista a la vez.

### En una jail con dos IP

Una jail de FreeBSD con dos IP, `192.0.2.11` y `192.0.2.12`, y
`ip4.saddrsel = 0` para que siempre salga por la primera:

- lista blanca de qBittorrent: `192.0.2.11/32`;
- `SEMILLERO_QBT=http://<IP de qBittorrent>:8080`;
- `SEMILLERO_LOGIN_DESDE=192.0.2.12`.

## Categorías

Las reglas de borrado van por categoría: las categorías `peliculas` y
`series` de qBittorrent y los torrents sin categoría. Si usas otros
nombres, esos torrents se ven y se clasifican, pero ninguna regla los borra.

## Bloqueos

qBittorrent bloquea una IP tras varios fallos de contraseña seguidos
(*Opciones → Interfaz web → Bloquear cliente tras fallos consecutivos*).
semillero gasta un fallo en cada arranque con la prueba al azar, y cada
contraseña mala que alguien escriba en la web es otro. En el modo con
contraseña, mientras dure el bloqueo semillero tampoco puede leer los
torrents. Si reinicias muchas veces seguidas y ves que no deja entrar,
espera a que pase el bloqueo.
