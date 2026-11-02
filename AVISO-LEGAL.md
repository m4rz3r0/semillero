# Aviso legal

## Marcas

qBittorrent y Jellyfin son marcas de sus dueños. semillero es un proyecto
independiente: no tiene relación con ellos, no lo patrocinan ni lo revisan.
Se nombran solo para decir con qué programas funciona.

## Borrar es borrar

En modo «Borrado real», semillero borra torrents **y sus ficheros del
disco**. Lo hace a mano, cuando tú lo pides, o solo, cuando se cumple una
regla y vence su aviso. Un fichero borrado no vuelve: semillero no tiene
papelera.

- Al estrenarlo está en «Solo aviso»: avisa, pero no borra nada.
- Activar el borrado real pide tu contraseña.
- Revisa las reglas y haz copias de lo que no quieras perder.

## Tu tracker y la ley

Las normas del tracker que trae semillero son un ejemplo, no las de ningún
tracker concreto. Tú decides qué normas pones y tú respondes de cumplir las
de tu tracker y las leyes de tu país. semillero no comprueba qué
descargas ni si puedes compartirlo.

## Sin garantía

semillero se publica con la licencia MIT, «tal cual», sin ninguna garantía.
Los autores no responden de los daños que cause su uso, como la pérdida de
ficheros o la expulsión de un tracker. El texto completo está en
[LICENSE](LICENSE).

## Lo que va dentro

- El código de semillero: licencia MIT ([LICENSE](LICENSE)).
- Las fuentes Alegreya y Alegreya Sans: licencia SIL Open Font License
  ([Alegreya](crates/semillero/public/fuentes/OFL-Alegreya.txt),
  [Alegreya Sans](crates/semillero/public/fuentes/OFL-AlegreyaSans.txt)).
- Las bibliotecas de Rust de las que depende, cada una con su licencia
  (`cargo deny check licenses` comprueba que son compatibles con MIT).
