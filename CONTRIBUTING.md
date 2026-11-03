# Contribuir

Gracias por echar una mano. Antes de un cambio grande, abre una incidencia
y lo hablamos.

## Lo que hace falta

- Rust con `rustup`. La versión está fijada en `rust-toolchain.toml`;
  `rustup` la instala sola, con el objetivo `wasm32-unknown-unknown`.
- [cargo-leptos](https://github.com/leptos-rs/cargo-leptos), la versión de
  `CARGO_LEPTOS_VERSION` en `.github/workflows/release.yml`:
  `cargo install --locked cargo-leptos --version 0.3.11`.
- Para las pruebas de los guiones: `sh`, `git`, `shellcheck`, `zip`, `unzip`
  y `git filter-repo`.

## Compilar y probar

Lo mismo que la CI (`.github/workflows/ci.yml`). Tiene que pasar todo antes
de pedir que se acepte un cambio:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --features semillero/ssr -- -D warnings
cargo test --workspace --locked --features semillero/ssr
cargo clippy -p semillero --lib --locked \
  --target wasm32-unknown-unknown --no-default-features --features hydrate \
  -- -D warnings

for p in deploy/pruebas/*.sh; do sh "$p"; done
for p in tools/pruebas/*.sh; do sh "$p"; done
for p in packaging/pruebas/*.sh; do sh "$p"; done
for f in packaging/*.sh packaging/pruebas/*.sh; do sh -n "$f"; done
shellcheck -x packaging/*.sh packaging/pruebas/*.sh

sh tools/publicar/ci.sh
cargo deny check
```

La CI además comprueba que compila en Windows:
`cargo check -p semillero --bin semillero --locked --no-default-features --features ssr`.

Para verlo en el navegador sin qBittorrent, sigue
[Probarlo sin qBittorrent](README.md#probarlo-sin-qbittorrent).

## Cada cambio, con su prueba

- Una prueba que falla antes del cambio y pasa después.
- Las reglas de `crates/reglas` tienen además pruebas de propiedades
  (`crates/reglas/tests/propiedades.rs`, con `proptest`): torrents y normas
  al azar en los que ninguno sale «Listo para recoger» sin haber cumplido,
  cada uno se juzga con las normas de su tracker y nada se recoge antes de
  sus horas de aviso. Si cambias las reglas, tienen que seguir pasando.
- Si cambias una variable, una opción o un mensaje, cambia también la
  documentación (`README.md`, `README.en.md`, `docs/`).

## Nada de datos de verdad

Ejemplos, pruebas y documentación usan solo direcciones y nombres
reservados para documentación:

- IPv4: `192.0.2.0/24`, `198.51.100.0/24`, `203.0.113.0/24`
- IPv6: `2001:db8::/32`
- Dominios: `*.example`, `*.home.arpa`
- Correos: `…@correo.example`

Ni tus IP, ni tu dominio, ni nombres de equipos, trackers o proveedores.
`sh tools/publicar/escanear.sh` busca lo que no debería salir; la CI lo pasa
por el árbol y por todo el historial. Usa en tus commits el correo
`…@users.noreply.github.com` de GitHub.

Si trabajas en una copia privada y publicas en otra pública, instala el
gancho `pre-push`: bloquea la subida al repositorio público si encuentra
algo de tu lista privada. Cómo se hace y cómo es esa lista, en
[tools/publicar/LEEME.md](tools/publicar/LEEME.md):

```sh
sh tools/publicar/instalar-gancho.sh --publico 'https://github.com/m4rz3r0/semillero.git'
```

## Commits

- En español, cortos, con el área delante: `reglas: …`, `semillero: …`,
  `deploy: …`, `empaquetado: …`, `ci: …`, `docs: …`.
- Un cambio coherente por commit, con sus pruebas y su documentación.
- Sin datos personales en el mensaje.

## Licencia

Lo que aportes se publica con la licencia MIT del proyecto
([LICENSE](LICENSE)).
