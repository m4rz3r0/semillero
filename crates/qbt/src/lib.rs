//! Cliente mínimo de la API web v2 de qBittorrent 5.x, de dos maneras:
//!
//! - **Lista blanca** ([`Qbt::new`]): semillero habla con qBittorrent desde
//!   una IP que está en su lista blanca («saltar autenticación» para esa
//!   subred) y no necesita sesión.
//! - **Con contraseña** ([`Qbt::con_cuenta`]): si la API contesta 403,
//!   entra con la [`Cuenta`] de semillero, guarda la cookie de sesión y
//!   repite la petición una vez. Sirve con o sin lista blanca.
//!
//! La comprobación de credenciales NO puede salir desde una IP de la lista
//! blanca: qBittorrent abre sesión sola a todo cliente de la lista, y
//! entonces `auth/login` acepta cualquier contraseña. Por eso esa llamada
//! sale desde `login_desde` si se da (en modo lista blanca, por defecto
//! 127.0.0.1), que no puede estar en la lista. Nunca lleva la cookie de la
//! sesión de semillero.

use std::fmt;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;

mod anadir;
pub use anadir::ArchivoTorrent;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("qBittorrent no responde: {0}")]
    NoResponde(String),
    #[error("qBittorrent rechazó la petición ({0})")]
    Rechazado(u16),
    #[error("respuesta de qBittorrent no válida: {0}")]
    Respuesta(String),
    #[error("hash no válido")]
    HashNoValido,
    #[error("qBittorrent no acepta el usuario y la contraseña de semillero")]
    CuentaRechazada,
    /// `torrents/add` contestó «Fails.» (o 415): no añadió alguno.
    #[error("qBittorrent no ha añadido los torrents")]
    NoAnadido,
}

/// Usuario y contraseña con los que semillero entra en qBittorrent (modo
/// con contraseña). `Debug` no enseña la contraseña.
#[derive(Clone)]
pub struct Cuenta {
    usuario: String,
    clave: String,
}

impl Cuenta {
    pub fn new(usuario: impl Into<String>, clave: impl Into<String>) -> Self {
        Self {
            usuario: usuario.into(),
            clave: clave.into(),
        }
    }
}

impl fmt::Debug for Cuenta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cuenta")
            .field("usuario", &self.usuario)
            .field("clave", &"(oculta)")
            .finish()
    }
}

const PLAZO: Duration = Duration::from_secs(10);
/// Tamaño máximo de una respuesta de la API (la lista de torrents ocupa
/// unos 2 KiB por torrent: da para decenas de miles).
pub const MAX_RESPUESTA: usize = 64 * 1024 * 1024;
/// Tamaño máximo de la respuesta del login («Ok.», «Fails.» o nada).
pub const MAX_RESPUESTA_LOGIN: usize = 64 * 1024;

/// Campos de `torrents/info` que usa semillero.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct TorrentQbt {
    pub hash: String,
    pub name: String,
    #[serde(default)]
    pub category: String,
    pub state: String,
    pub downloaded: i64,
    pub total_size: i64,
    pub uploaded: i64,
    pub seeding_time: i64,
    pub ratio: f64,
    pub added_on: i64,
    pub save_path: String,
    pub upspeed: i64,
    pub dlspeed: i64,
    pub progress: f64,
    /// Host del tracker que funciona ahora (campo `tracker`); vacío si
    /// qBittorrent no tiene ninguno que conteste. El anuncio entero lleva el
    /// passkey del usuario: se lee directamente como host y nunca se guarda.
    #[serde(default, deserialize_with = "leer_tracker")]
    pub tracker: String,
}

/// Lee el campo `tracker` de `torrents/info` y se queda solo con el host.
/// Nunca falla: algo que no es un anuncio con host queda vacío (y un error
/// aquí podría repetir el anuncio en el mensaje).
fn leer_tracker<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = Option::<serde_json::Value>::deserialize(d)?;
    Ok(v.as_ref()
        .and_then(serde_json::Value::as_str)
        .map(host_del_anuncio)
        .unwrap_or_default())
}

/// El host de un anuncio (`https://tracker.ejemplo.example:443/passkey/announce`
/// → `tracker.ejemplo.example`): sin esquema, usuario, puerto ni ruta, en
/// minúsculas. Una IP, como la escribe la librería estándar. Vacío si no es
/// un anuncio con un host válido.
pub fn host_del_anuncio(anuncio: &str) -> String {
    let Some((esquema, resto)) = anuncio.trim().split_once("://") else {
        return String::new();
    };
    let esquema_bueno = esquema.starts_with(|c: char| c.is_ascii_alphabetic())
        && esquema
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    if !esquema_bueno {
        return String::new();
    }
    let autoridad = resto
        .split(['/', '\\', '?', '#'])
        .next()
        .unwrap_or_default();
    let autoridad = autoridad.rsplit_once('@').map_or(autoridad, |(_, h)| h);
    if let Some(r) = autoridad.strip_prefix('[') {
        let Some((ip, tras)) = r.split_once(']') else {
            return String::new();
        };
        if !(tras.is_empty() || tras.starts_with(':')) {
            return String::new();
        }
        return ip
            .parse::<std::net::Ipv6Addr>()
            .map(|ip| ip.to_string())
            .unwrap_or_default();
    }
    let host = autoridad.split_once(':').map_or(autoridad, |(h, _)| h);
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty() || host.len() > 253 || !host.is_ascii() {
        return String::new();
    }
    if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
        return ip.to_string();
    }
    let host = host.to_ascii_lowercase();
    let etiqueta_buena = |e: &str| {
        (1..=63).contains(&e.len())
            && !e.starts_with('-')
            && !e.ends_with('-')
            && e.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    if host.split('.').all(etiqueta_buena) {
        host
    } else {
        String::new()
    }
}

/// SHA-1 (v1) o SHA-256 (v2) en minúsculas: con `hashes` vacío o raro
/// qBittorrent devolvería todos, y un borrado se llevaría el primero.
pub fn hash_valido(h: &str) -> bool {
    matches!(h.len(), 40 | 64)
        && h.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub struct Qbt {
    base: String,
    api: Client,
    login: Client,
    /// Modo con contraseña: la cuenta y la cookie de la sesión abierta.
    cuenta: Option<Cuenta>,
    cookie: Mutex<Option<String>>,
}

fn no_responde(e: reqwest::Error) -> Error {
    Error::NoResponde(e.to_string())
}

/// El cuerpo de `r` como texto, sin pasar de `max` bytes (si lo dice
/// `content-length`, sin leer nada; si no, al llegar al límite).
async fn texto_limitado(mut r: reqwest::Response, max: usize) -> Result<String, Error> {
    let grande = || Error::Respuesta(format!("la respuesta pasa de {max} bytes"));
    if r.content_length().is_some_and(|n| n > max as u64) {
        return Err(grande());
    }
    let mut cuerpo = Vec::new();
    while let Some(trozo) = r.chunk().await.map_err(no_responde)? {
        if cuerpo.len() + trozo.len() > max {
            return Err(grande());
        }
        cuerpo.extend_from_slice(&trozo);
    }
    String::from_utf8(cuerpo).map_err(|_| Error::Respuesta("texto que no es UTF-8".into()))
}

fn formulario(campos: &[(&str, &str)]) -> String {
    form_urlencoded::Serializer::new(String::new())
        .extend_pairs(campos)
        .finish()
}

impl Qbt {
    /// Modo lista blanca: la API sin sesión; las contraseñas se comprueban
    /// desde `login_desde`.
    pub fn new(base: &str, login_desde: IpAddr) -> Self {
        Self::armar(base, Some(login_desde), None)
    }

    /// Modo con contraseña: la API entra con `cuenta` cuando hace falta;
    /// las contraseñas se comprueban desde `login_desde` si se da (si no,
    /// desde la dirección que elija el sistema).
    pub fn con_cuenta(base: &str, login_desde: Option<IpAddr>, cuenta: Cuenta) -> Self {
        Self::armar(base, login_desde, Some(cuenta))
    }

    fn armar(base: &str, login_desde: Option<IpAddr>, cuenta: Option<Cuenta>) -> Self {
        let cliente = |desde: Option<IpAddr>| {
            Client::builder()
                .timeout(PLAZO)
                .local_address(desde)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("cliente HTTP")
        };
        Self {
            base: base.trim_end_matches('/').into(),
            api: cliente(None),
            login: cliente(login_desde),
            cuenta,
            cookie: Mutex::new(None),
        }
    }

    fn cookie(&self) -> Option<String> {
        self.cookie
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Hace la petición que arma `peticion`, con la cookie de la sesión si
    /// la hay. En modo con contraseña, un 403 (sin sesión o caducada) abre
    /// sesión y repite la petición una sola vez: un 403 dice que no se
    /// hizo nada, así que repetir un borrado no borra dos veces.
    async fn pedir(
        &self,
        peticion: impl Fn(&Client) -> reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, Error> {
        let mut entrado = false;
        loop {
            let mut req = peticion(&self.api);
            if let Some(c) = self.cookie() {
                req = req.header(reqwest::header::COOKIE, c);
            }
            let r = req.send().await.map_err(no_responde)?;
            if r.status() == reqwest::StatusCode::FORBIDDEN
                && !entrado
                && let Some(cuenta) = &self.cuenta
            {
                self.entrar(cuenta).await?;
                entrado = true;
                continue;
            }
            if !r.status().is_success() {
                return Err(Error::Rechazado(r.status().as_u16()));
            }
            return Ok(r);
        }
    }

    /// Abre sesión con `cuenta` y guarda sus cookies para las peticiones
    /// siguientes.
    async fn entrar(&self, cuenta: &Cuenta) -> Result<(), Error> {
        let r = self
            .api
            .post(format!("{}/api/v2/auth/login", self.base))
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(formulario(&[
                ("username", &cuenta.usuario),
                ("password", &cuenta.clave),
            ]))
            .send()
            .await
            .map_err(no_responde)?;
        let estado = r.status();
        // Solo el nombre y el valor de cada cookie («SID=…»), sin atributos.
        let cookies: Vec<String> = r
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .filter_map(|v| v.split(';').next())
            .map(str::trim)
            .filter(|c| c.split_once('=').is_some_and(|(n, _)| !n.is_empty()))
            .map(String::from)
            .collect();
        let cuerpo = texto_limitado(r, MAX_RESPUESTA_LOGIN).await?;
        if estado == reqwest::StatusCode::UNAUTHORIZED {
            return Err(Error::CuentaRechazada);
        }
        if !estado.is_success() {
            return Err(Error::Rechazado(estado.as_u16()));
        }
        if cuerpo.trim() == "Fails." {
            return Err(Error::CuentaRechazada);
        }
        if cookies.is_empty() {
            return Err(Error::Respuesta("el login no trae cookie de sesión".into()));
        }
        *self.cookie.lock().unwrap_or_else(|e| e.into_inner()) = Some(cookies.join("; "));
        Ok(())
    }

    async fn leer(&self, ruta: &str, consulta: &[(&str, &str)]) -> Result<String, Error> {
        let mut url = format!("{}{ruta}", self.base);
        if !consulta.is_empty() {
            url = format!("{url}?{}", formulario(consulta));
        }
        let r = self.pedir(|c| c.get(&url)).await?;
        texto_limitado(r, MAX_RESPUESTA).await
    }

    async fn enviar(&self, ruta: &str, campos: &[(&str, &str)]) -> Result<(), Error> {
        let url = format!("{}{ruta}", self.base);
        let cuerpo = formulario(campos);
        self.pedir(|c| {
            c.post(&url)
                .header(
                    reqwest::header::CONTENT_TYPE,
                    "application/x-www-form-urlencoded",
                )
                .body(cuerpo.clone())
        })
        .await?;
        Ok(())
    }

    fn json<T: for<'de> Deserialize<'de>>(texto: &str) -> Result<T, Error> {
        serde_json::from_str(texto).map_err(|e| Error::Respuesta(e.to_string()))
    }

    pub async fn torrents(&self) -> Result<Vec<TorrentQbt>, Error> {
        Self::json(&self.leer("/api/v2/torrents/info", &[]).await?)
    }

    pub async fn obtener(&self, hash: &str) -> Result<Option<TorrentQbt>, Error> {
        if !hash_valido(hash) {
            return Err(Error::HashNoValido);
        }
        let ts: Vec<TorrentQbt> = Self::json(
            &self
                .leer("/api/v2/torrents/info", &[("hashes", hash)])
                .await?,
        )?;
        Ok(ts.into_iter().find(|t| t.hash == hash))
    }

    async fn sobre_varios(&self, ruta: &str, hashes: &[String]) -> Result<(), Error> {
        if hashes.is_empty() {
            return Ok(());
        }
        if !hashes.iter().all(|h| hash_valido(h)) {
            return Err(Error::HashNoValido);
        }
        self.enviar(ruta, &[("hashes", &hashes.join("|"))]).await
    }

    pub async fn parar(&self, hashes: &[String]) -> Result<(), Error> {
        self.sobre_varios("/api/v2/torrents/stop", hashes).await
    }

    pub async fn reanudar(&self, hashes: &[String]) -> Result<(), Error> {
        self.sobre_varios("/api/v2/torrents/start", hashes).await
    }

    pub async fn borrar(&self, hash: &str, ficheros: bool) -> Result<(), Error> {
        if !hash_valido(hash) {
            return Err(Error::HashNoValido);
        }
        self.enviar(
            "/api/v2/torrents/delete",
            &[
                ("hashes", hash),
                ("deleteFiles", if ficheros { "true" } else { "false" }),
            ],
        )
        .await
    }

    pub async fn credenciales_validas(&self, usuario: &str, clave: &str) -> Result<bool, Error> {
        let r = self
            .login
            .post(format!("{}/api/v2/auth/login", self.base))
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(formulario(&[("username", usuario), ("password", clave)]))
            .send()
            .await
            .map_err(no_responde)?;
        let estado = r.status();
        let cuerpo = texto_limitado(r, MAX_RESPUESTA_LOGIN).await?;
        // qBittorrent 5.2 contesta con el código HTTP (204 si entra, 401 si
        // no); las versiones anteriores, 200 con «Ok.» o «Fails.». Su propia
        // web da por buena cualquier 2xx; aquí, 2xx salvo un «Fails.» antiguo.
        // Cualquier otra cosa (403 de una IP baneada, 5xx) no es una clave
        // mala, y no se dice que lo sea.
        if estado == reqwest::StatusCode::UNAUTHORIZED {
            return Ok(false);
        }
        if !estado.is_success() {
            return Err(Error::Rechazado(estado.as_u16()));
        }
        Ok(cuerpo.trim() != "Fails.")
    }
}

#[cfg(test)]
mod pruebas {
    use super::*;

    const PASSKEY: &str = "abc123secreto";

    #[test]
    fn del_anuncio_solo_queda_el_host() {
        for (url, host) in [
            (
                "https://tracker.ejemplo.example:443/abc123secreto/announce",
                "tracker.ejemplo.example",
            ),
            (
                "http://Tracker.Ejemplo.EXAMPLE/announce?passkey=abc123secreto",
                "tracker.ejemplo.example",
            ),
            (
                "udp://tracker.segundo.example:1337/announce",
                "tracker.segundo.example",
            ),
            (
                "https://usuario:abc123secreto@ejemplo.example/a",
                "ejemplo.example",
            ),
            ("http://127.0.0.1:8080/announce", "127.0.0.1"),
            ("http://[2001:DB8::1]:6969/announce", "2001:db8::1"),
            ("https://ejemplo.example./announce", "ejemplo.example"),
            ("https://ejemplo.example#abc123secreto", "ejemplo.example"),
            // Como un navegador: `\` corta la autoridad igual que `/`.
            (
                "https://ejemplo.example\\@otro.example/announce",
                "ejemplo.example",
            ),
        ] {
            assert_eq!(host_del_anuncio(url), host, "{url}");
        }
        for malo in [
            "",
            "ejemplo.example/announce",
            "https://",
            "https:///announce",
            "https://ejem plo.example/",
            "https://ejémplo.example/",
            "https://[ejemplo.example]/",
            "://ejemplo.example/",
        ] {
            assert_eq!(host_del_anuncio(malo), "", "{malo:?}");
        }
    }

    fn info(tracker: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "hash": "1111111111111111111111111111111111111111", "name": "Una",
            "state": "uploading", "downloaded": 1, "total_size": 1, "uploaded": 1,
            "seeding_time": 1, "ratio": 1.0, "added_on": 1, "save_path": "/m",
            "upspeed": 0, "dlspeed": 0, "progress": 1.0, "tracker": tracker
        })
    }

    #[test]
    fn el_passkey_no_llega_al_torrent_ni_a_su_debug() {
        let url = format!("https://tracker.ejemplo.example:443/{PASSKEY}/announce");
        let t: TorrentQbt = serde_json::from_value(info(url.into())).unwrap();
        assert_eq!(t.tracker, "tracker.ejemplo.example");
        assert!(!format!("{t:?}").contains(PASSKEY));
        assert!(!format!("{t:#?}").contains("announce"));
    }

    #[test]
    fn sin_tracker_o_con_uno_raro_el_host_queda_vacio() {
        let mut sin = info(serde_json::Value::Null);
        sin.as_object_mut().unwrap().remove("tracker");
        let t: TorrentQbt = serde_json::from_value(sin).unwrap();
        assert_eq!(t.tracker, "");
        for raro in [
            serde_json::json!(""),
            serde_json::json!(null),
            serde_json::json!(7),
        ] {
            let t: TorrentQbt = serde_json::from_value(info(raro)).unwrap();
            assert_eq!(t.tracker, "");
        }
    }
}
