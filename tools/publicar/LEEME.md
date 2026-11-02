# Publicar sin filtrar datos

El repositorio de trabajo es privado y sigue vivo. El público es una copia
reescrita que se regenera cada vez desde la rama `main`: sin las carpetas
privadas, con los valores reales cambiados por inventados y con autor
`noreply`. Estas herramientas la hacen y demuestran que no lleva nada
privado.

| Fichero | Para qué |
|---|---|
| `escanear.sh` | escáner del árbol o de todo el historial |
| `reglas.awk` | las reglas del escáner |
| `permitido.txt` | lo poco que puede salir aunque una regla lo marque |
| `rutas-privadas.txt` | rutas que nunca salen |
| `regenerar.sh` | hace la copia pública y la comprueba |
| `gancho-pre-push`, `instalar-gancho.sh` | bloquean una subida con hallazgos al repositorio público |
| `ci.sh` | la comprobación de la CI, sin lista privada |

Pruebas: `sh tools/pruebas/publicar.sh` (sin red; las de `regenerar.sh` se
saltan si no hay `git filter-repo`).

## Rangos para ejemplos y pruebas

Usa solo direcciones y nombres reservados para documentación; así cualquier
otro salta a la vista:

- IPv4: `192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`
- IPv6: `2001:db8::/32`
- Dominios: `*.example`, `*.home.arpa` (y `localhost`)
- Correos: `…@correo.example` o tu `…@users.noreply.github.com`
- MAC: `00:00:5e:00:53:00` a `00:00:5e:00:53:ff`

Las direcciones privadas, de bucle local o públicas, las MAC, los correos,
las claves, las cadenas con pinta de token, las rutas de tu disco
(`/home/<usuario>/`, `/Users/<usuario>/`), los dominios de fuera de esa lista
y los binarios son hallazgos salvo que estén en `permitido.txt`. Cada entrada
de ese fichero lleva un motivo; mantenlo corto. Los nombres `.local` y `.app`
no se miran (chocan con nombres de fichero y con código): si usas alguno de
verdad, ponlo en la lista privada.

## La lista privada

Vive fuera del repositorio, en
`~/.config/semillero-publicacion/privado.txt`, y nunca se sube. Tiene el
formato de `git filter-repo --replace-text`, una entrada por línea:

```
valor-real==>valor-inventado
regex:expresión==>valor-inventado
glob:patrón==>valor-inventado
# comentario
```

El escáner usa el lado izquierdo; las expresiones se leen como ERE (de las
de Python se entienden `\d`, `\s`, `\w`, `(?:` y `(?i)` al principio). En los
hallazgos solo aparece el número de línea de la entrada, nunca el valor.

## Regenerar la copia pública

```
git config semillero.publicoNombre 'Tu alias público'
git config semillero.publicoCorreo '123+alias@users.noreply.github.com'
git config semillero.publico 'https://github.com/…/semillero.git'
sh tools/publicar/regenerar.sh
```

Hace un clon nuevo (nunca toca el de trabajo), deja `main` y las etiquetas
`v*` que cuelgan de ella, pasa `git filter-repo` (rutas privadas, lista
privada en ficheros y mensajes, sin «cherry picked from», autor público) y
escanea el resultado entero con la lista privada. Si queda algo, sale con
error y deja la carpeta para revisarla. Si sale limpio, escribe las órdenes
de `git push`: la rama primero y cada etiqueta sola (GitHub no lanza los
flujos de más de tres etiquetas subidas a la vez). No sube nada por sí mismo.

Necesita `git filter-repo`. Con los mismos datos de entrada da los mismos
commits, así que regenerar y volver a subir no reescribe lo ya publicado.

### Aplanado: un commit por versión

```
sh tools/publicar/regenerar.sh --plano v0.3.0                 # la primera vez
sh tools/publicar/regenerar.sh --plano v0.4.0 --sobre "$(git config semillero.publico)"
```

En lugar del historial reescrito, la copia lleva un solo commit con el árbol
de `main`, el mensaje `semillero vX.Y.Z`, la fecha de hoy a las 00:00 UTC y
la etiqueta. No sale nada de los commits privados (ficheros borrados,
mensajes, horas, zona horaria). Con `--sobre` el commit cuelga del último
del repositorio público, así que la subida es un avance normal; si esa
versión ya está publicada o no hay cambios, no hace nada.

### Excepciones

Las sustituciones son ciegas: si la lista cambia tu alias, también cambiaría
la dirección del repositorio público que lo lleva a propósito. Los textos que
deben salir tal cual van en `~/.config/semillero-publicacion/excepciones.txt`,
uno por línea (sin `==>`). `regenerar.sh` los protege antes de sustituir y los
devuelve al final, y el escáner no los cuenta. Solo valen para el contenido y
los mensajes: un nombre de fichero con un valor privado hay que cambiarlo en
el repositorio privado.

## Gancho pre-push

```
sh tools/publicar/instalar-gancho.sh --publico 'https://github.com/…/semillero.git'
```

Al subir a ese remoto, pasa el escáner con la lista privada por todo lo que
aún no está allí (commits, ficheros, mensajes, autores, etiquetas) y bloquea
la subida si encuentra algo. A los demás remotos no les hace nada. Sin la
lista privada a mano, bloquea. `regenerar.sh` lo instala también en la copia
que hace.

## CI

`sh tools/publicar/ci.sh` escanea el árbol y todo el historial con las reglas
genéricas, sin lista privada. En GitHub Actions el checkout necesita
`fetch-depth: 0`; con un clon superficial falla a propósito.
