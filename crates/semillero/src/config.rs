//! Configuración del servidor, por variables de entorno. Los valores por
//! defecto sirven para probarlo en el propio equipo, en cualquier sistema:
//!
//! | Variable | Qué es | Por defecto |
//! |---|---|---|
//! | `SEMILLERO_LISTEN` | dirección donde escuchar (`IP:puerto`, o solo la IP y entonces `SEMILLERO_PORT`) | `127.0.0.1:8090` |
//! | `SEMILLERO_PORT` | puerto, si `SEMILLERO_LISTEN` es solo una IP | `8090` |
//! | `SEMILLERO_QBT` | API web de qBittorrent | `http://127.0.0.1:8080` |
//! | `SEMILLERO_QBT_CLAVE` | fichero con la contraseña de la WebUI de qBittorrent (modo con contraseña, ver abajo) | (ninguno: modo lista blanca) |
//! | `SEMILLERO_QBT_USUARIO` | usuario de la WebUI de qBittorrent en el modo con contraseña | `admin` |
//! | `SEMILLERO_LOGIN_DESDE` | IP de origen al comprobar contraseñas; no puede estar en la lista blanca de qBittorrent | `127.0.0.1` en modo lista blanca; en modo con contraseña, la que elija el sistema |
//! | `SEMILLERO_DB` | base de datos SQLite | `semillero.db` (en el directorio de trabajo) |
//! | `SEMILLERO_ESTADO` | fichero de estado de la VPN y del puerto (ver [`crate::estado_red`]) | (ninguno: no se vigilan) |
//! | `SEMILLERO_ORIGEN` | orígenes públicos admitidos en los `POST`, separados por comas | `http://127.0.0.1:<puerto>,http://localhost:<puerto>` |
//! | `SEMILLERO_PROXY` | IP del proxy inverso: solo de ella se cree la IP del cliente en `X-Forwarded-For` | (ninguno: vale la IP de la conexión) |
//! | `SEMILLERO_LOG` | nivel de registro (sintaxis de `EnvFilter`) | `info` |
//! | `SEMILLERO_JELLYFIN` | dirección de Jellyfin; sin ella no se avisa a ninguna mediateca | (sin Jellyfin) |
//! | `SEMILLERO_JELLYFIN_CLAVE` | fichero con la clave de la API de Jellyfin | `jellyfin.clave` junto a la base de datos |
//! | `LEPTOS_SITE_ROOT` | directorio con la web (WASM, JS y CSS) | `site` junto al ejecutable |
//!
//! qBittorrent se usa de dos maneras:
//!
//! - **Lista blanca** (sin `SEMILLERO_QBT_CLAVE`): la IP de semillero está
//!   en la lista blanca de qBittorrent y la API va sin sesión. Las
//!   contraseñas se comprueban desde `SEMILLERO_LOGIN_DESDE`, que no puede
//!   estar en la lista; por eso qBittorrent tiene que estar en este equipo.
//! - **Con contraseña** (`SEMILLERO_QBT_CLAVE`): semillero entra en
//!   qBittorrent con su propia cuenta y no hace falta lista blanca. Sirve en
//!   podman, en Windows y con qBittorrent en otra máquina.
//!
//! El freno de intentos de acceso va por la IP real del cliente. El proxy
//! inverso la añade al final de `X-Forwarded-For` (y debe descartar lo que
//! traiga un cliente), así que se toma la última entrada de esa cabecera, y
//! solo si la conexión viene de `SEMILLERO_PROXY`; si viene de cualquier
//! otra IP, o no hay proxy configurado, la cabecera se ignora y vale la IP
//! de la conexión.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    pub escucha: SocketAddr,
    pub qbt: String,
    /// IP de origen de las comprobaciones de contraseña, si se eligió (ver
    /// [`Config::login_desde_efectivo`]).
    pub login_desde: Option<IpAddr>,
    /// Modo con contraseña: la cuenta de semillero en qBittorrent.
    pub cuenta_qbt: Option<ConfCuentaQbt>,
    pub db: PathBuf,
    /// Fichero de estado de la VPN y del puerto, si lo hay.
    pub estado_red: Option<PathBuf>,
    pub origenes: Vec<String>,
    /// Proxy de confianza (ver arriba), si lo hay.
    pub proxy: Option<IpAddr>,
    /// Jellyfin, si lo hay.
    pub jellyfin: Option<ConfJellyfin>,
}

/// Dónde está Jellyfin y el fichero con su clave (la clave no va en una
/// variable de entorno, que se ve en la lista de procesos).
#[derive(Clone, Debug, PartialEq)]
pub struct ConfJellyfin {
    pub url: String,
    pub clave: PathBuf,
}

/// Usuario de qBittorrent y el fichero con su contraseña (modo con
/// contraseña). Como la clave de Jellyfin, la contraseña no va en una
/// variable de entorno.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfCuentaQbt {
    pub usuario: String,
    pub clave: PathBuf,
}

const PUERTO: u16 = 8090;
/// Más que esto no es una contraseña ni una clave de API.
const MAX_SECRETO_BYTES: u64 = 1024;

/// Lee la variable `k` con `leer`, sin espacios alrededor; vacía es como
/// si no estuviera.
fn variable(leer: &impl Fn(&str) -> Option<String>, k: &str) -> Option<String> {
    leer(k)
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
}

impl Config {
    pub fn desde_entorno() -> Result<Self, String> {
        Self::desde(|k| std::env::var(k).ok())
    }

    /// Como [`Config::desde_entorno`], leyendo las variables con `leer`.
    pub fn desde(leer: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let var = |k: &str| variable(&leer, k);
        let puerto = match var("SEMILLERO_PORT") {
            Some(p) => p
                .parse::<u16>()
                .map_err(|_| format!("SEMILLERO_PORT no válido: {p:?}"))?,
            None => PUERTO,
        };
        let escucha = match var("SEMILLERO_LISTEN") {
            None => SocketAddr::new(IpAddr::from([127, 0, 0, 1]), puerto),
            // Solo la IP, y el puerto aparte.
            Some(v) => match v.parse::<IpAddr>() {
                Ok(ip) => SocketAddr::new(ip, puerto),
                Err(_) => v
                    .parse()
                    .map_err(|_| format!("SEMILLERO_LISTEN no válido: {v:?}"))?,
            },
        };
        let login_desde = match var("SEMILLERO_LOGIN_DESDE") {
            Some(v) => Some(
                v.parse()
                    .map_err(|_| format!("SEMILLERO_LOGIN_DESDE no válido: {v:?}"))?,
            ),
            None => None,
        };
        let cuenta_qbt = var("SEMILLERO_QBT_CLAVE").map(|clave| ConfCuentaQbt {
            usuario: var("SEMILLERO_QBT_USUARIO").unwrap_or_else(|| "admin".into()),
            clave: clave.into(),
        });
        let proxy = match var("SEMILLERO_PROXY") {
            Some(v) => Some(
                v.parse()
                    .map_err(|_| format!("SEMILLERO_PROXY no válido: {v:?}"))?,
            ),
            None => None,
        };
        let p = escucha.port();
        let origenes: Vec<String> = var("SEMILLERO_ORIGEN")
            .unwrap_or_else(|| format!("http://127.0.0.1:{p},http://localhost:{p}"))
            .split(',')
            .map(|o| o.trim().trim_end_matches('/').to_owned())
            .filter(|o| !o.is_empty())
            .collect();
        let db: PathBuf = var("SEMILLERO_DB")
            .unwrap_or_else(|| "semillero.db".into())
            .into();
        let jellyfin = var("SEMILLERO_JELLYFIN").map(|url| ConfJellyfin {
            url: url.trim_end_matches('/').to_owned(),
            clave: var("SEMILLERO_JELLYFIN_CLAVE")
                .map(PathBuf::from)
                .unwrap_or_else(|| junto_a(&db, "jellyfin.clave")),
        });
        Ok(Self {
            escucha,
            qbt: var("SEMILLERO_QBT").unwrap_or_else(|| "http://127.0.0.1:8080".into()),
            login_desde,
            cuenta_qbt,
            db,
            estado_red: var("SEMILLERO_ESTADO").map(PathBuf::from),
            origenes,
            proxy,
            jellyfin,
        })
    }
}

impl Config {
    /// IP de origen de las comprobaciones de contraseña: la de
    /// `SEMILLERO_LOGIN_DESDE`; si no se dio, 127.0.0.1 en modo lista blanca
    /// y ninguna (la que elija el sistema) en modo con contraseña, que así
    /// funciona con qBittorrent en otra máquina.
    pub fn login_desde_efectivo(&self) -> Option<IpAddr> {
        match (self.login_desde, &self.cuenta_qbt) {
            (Some(ip), _) => Some(ip),
            (None, Some(_)) => None,
            (None, None) => Some(IpAddr::from([127, 0, 0, 1])),
        }
    }

    /// Aviso (para `semillero configurar`) si puede que las contraseñas no
    /// se puedan comprobar: en modo lista blanca, con qBittorrent fuera de
    /// este equipo y sin `SEMILLERO_LOGIN_DESDE`, salen desde 127.0.0.1. En
    /// una jail de FreeBSD eso es la IP de la jail y puede valer, así que el
    /// servidor no lo dice al arrancar.
    pub fn aviso_login(&self) -> Option<String> {
        if self.cuenta_qbt.is_some() || self.login_desde.is_some() || es_local(&self.qbt) {
            return None;
        }
        Some(format!(
            "qBittorrent ({}) no está en este equipo y no hay SEMILLERO_QBT_CLAVE ni \
             SEMILLERO_LOGIN_DESDE: las contraseñas se comprobarán desde 127.0.0.1, que \
             solo vale si qBittorrent la ve (en una jail de FreeBSD, la IP de la jail). \
             Si no, usa el modo con contraseña",
            self.qbt
        ))
    }
}

/// Si la dirección `url` (`http://máquina:puerto/…`) es de este equipo.
pub(crate) fn es_local(url: &str) -> bool {
    let resto = url.split_once("://").map_or(url, |(_, r)| r);
    let autoridad = resto.split(['/', '?', '#']).next().unwrap_or_default();
    let autoridad = autoridad.rsplit_once('@').map_or(autoridad, |(_, a)| a);
    let maquina = match autoridad.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => autoridad.split(':').next().unwrap_or_default(),
    };
    maquina.eq_ignore_ascii_case("localhost")
        || maquina.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Lee un secreto (contraseña o clave de una API) del fichero `ruta`, con
/// un límite de tamaño y sin el salto de línea final. `que` lo nombra en los
/// errores, que nunca llevan su contenido.
pub fn leer_secreto(ruta: &Path, que: &str) -> Result<String, String> {
    use std::io::Read;
    let mut texto = String::new();
    std::fs::File::open(ruta)
        .and_then(|f| f.take(MAX_SECRETO_BYTES + 1).read_to_string(&mut texto))
        .map_err(|e| format!("no se puede leer {que} ({}): {e}", ruta.display()))?;
    if texto.len() as u64 > MAX_SECRETO_BYTES {
        return Err(format!(
            "el fichero de {que} ({}) es demasiado grande para ser una clave",
            ruta.display()
        ));
    }
    let texto = texto.strip_suffix('\n').unwrap_or(&texto);
    let texto = texto.strip_suffix('\r').unwrap_or(texto);
    if texto.trim().is_empty() {
        return Err(format!(
            "el fichero de {que} ({}) está vacío",
            ruta.display()
        ));
    }
    Ok(texto.to_owned())
}

/// Más que esto no es un fichero de configuración.
const MAX_FICHERO_ENV_BYTES: u64 = 64 * 1024;

/// Lee el fichero de configuración `ruta` (`semillero --env`): el que
/// escribe `semillero configurar`, una variable por línea,
/// `VARIABLE=valor`, sin comillas; las líneas vacías y las que empiezan por
/// `#` no cuentan, y si una variable sale dos veces vale la última, como en
/// systemd. Una línea que no es así para la lectura; el error dice el
/// fichero y la línea, nunca lo que pone.
pub fn leer_fichero_env(ruta: &Path) -> Result<HashMap<String, String>, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(ruta)
        .and_then(|f| f.take(MAX_FICHERO_ENV_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("no se puede leer {}: {e}", ruta.display()))?;
    if bytes.len() as u64 > MAX_FICHERO_ENV_BYTES {
        return Err(format!("{} es demasiado grande", ruta.display()));
    }
    let texto =
        String::from_utf8(bytes).map_err(|_| format!("{} no es texto UTF-8", ruta.display()))?;
    let mut vars = HashMap::new();
    for (n, linea) in texto.lines().enumerate() {
        let linea = linea.strip_suffix('\r').unwrap_or(linea);
        if linea.trim().is_empty() || linea.trim_start().starts_with('#') {
            continue;
        }
        let nombre_valido = |k: &str| {
            !k.is_empty()
                && !k.starts_with(|c: char| c.is_ascii_digit())
                && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        };
        match linea.split_once('=') {
            Some((k, v)) if nombre_valido(k) => {
                vars.insert(k.to_owned(), v.to_owned());
            }
            _ => {
                return Err(format!(
                    "{}, línea {}: no es VARIABLE=valor",
                    ruta.display(),
                    n + 1
                ));
            }
        }
    }
    Ok(vars)
}

/// Las variables de `entorno` y, las que no estén ahí, las de `fichero`
/// (ver [`leer_fichero_env`]): lo que ya está puesto manda.
pub fn con_fichero(
    entorno: impl Fn(&str) -> Option<String>,
    fichero: HashMap<String, String>,
) -> impl Fn(&str) -> Option<String> {
    move |k| entorno(k).or_else(|| fichero.get(k).cloned())
}

/// La contraseña de qBittorrent del modo con contraseña. Los espacios
/// cuentan; solo se quita el salto de línea del final.
pub fn leer_contrasena_qbt(cuenta: &ConfCuentaQbt) -> Result<String, String> {
    leer_secreto(&cuenta.clave, "la contraseña de qBittorrent")
}

/// Si otros usuarios del sistema pueden leer el fichero `ruta` (permisos de
/// grupo u otros). Un secreto debería ser solo de su dueño.
#[cfg(unix)]
pub fn legible_por_otros(ruta: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(ruta).is_ok_and(|m| m.permissions().mode() & 0o077 != 0)
}

/// `nombre` en el mismo directorio que `fichero`.
fn junto_a(fichero: &Path, nombre: &str) -> PathBuf {
    fichero
        .parent()
        .map(|d| d.join(nombre))
        .unwrap_or_else(|| PathBuf::from(nombre))
}

/// Directorio de la web: `LEPTOS_SITE_ROOT` si está; si no, `site` junto
/// al ejecutable `ejecutable` (o en el directorio de trabajo si no se sabe
/// dónde está).
pub fn raiz_web(leer: impl Fn(&str) -> Option<String>, ejecutable: Option<&Path>) -> PathBuf {
    if let Some(r) = variable(&leer, "LEPTOS_SITE_ROOT") {
        return r.into();
    }
    match ejecutable {
        Some(exe) => junto_a(exe, "site"),
        None => PathBuf::from("site"),
    }
}
