//! qBittorrent 5.2 simulado, para las pruebas.
//!
//! Imita lo que semillero usa de la API web v2:
//! - `auth/login` contesta 204 y una cookie `SID` si las credenciales son
//!   buenas y 401 si no. A un cliente de la lista blanca le abre sesión con
//!   cualquier contraseña, como el de verdad.
//! - Sin sesión ni lista blanca, el resto de la API contesta 403.
//! - `torrents/info` (con `hashes`), `torrents/stop`,
//!   `torrents/start` y `torrents/delete`.

use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::{ConnectInfo, Form, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::Value;

pub struct Conf {
    pub usuario: String,
    pub clave: String,
    pub lista_blanca: Vec<IpAddr>,
    /// Login como antes de 5.2: 200 con «Ok.» o «Fails.».
    pub antiguo: bool,
    /// IP baneada tras demasiados fallos: el login contesta 403.
    pub baneado: bool,
    /// El resto de la API contesta sin sesión, pero el login sigue
    /// comprobando la contraseña: como en producción, donde semillero pide
    /// los torrents desde una IP de la lista blanca y comprueba contraseñas
    /// desde otra que no lo está (en las pruebas las dos son 127.0.0.1).
    pub api_abierta: bool,
}

impl Default for Conf {
    fn default() -> Self {
        Self {
            usuario: "admin".into(),
            clave: "secreta".into(),
            lista_blanca: Vec::new(),
            antiguo: false,
            baneado: false,
            api_abierta: false,
        }
    }
}

/// Una petición recibida, para comprobar desde las pruebas qué se pidió.
#[derive(Clone, Debug, PartialEq)]
pub struct Llamada {
    pub ruta: String,
    pub cuerpo: String,
    pub desde: IpAddr,
}

#[derive(Default)]
struct Interior {
    torrents: Vec<Value>,
    llamadas: Vec<Llamada>,
    sesiones: Vec<String>,
    /// Número de la última sesión abierta (no se repiten al olvidarlas).
    ultimo_sid: usize,
    relecturas: Vec<(String, Option<Value>)>,
}

struct Estado {
    conf: Conf,
    interior: Mutex<Interior>,
    /// Empieza como `conf.baneado`; se cambia con [`SimControl::banear`].
    baneado: AtomicBool,
}

#[derive(Clone)]
pub struct SimControl(Arc<Estado>);

impl SimControl {
    pub fn fijar_torrents(&self, torrents: Vec<Value>) {
        self.0.interior.lock().unwrap().torrents = torrents;
    }

    pub fn torrents(&self) -> Vec<Value> {
        self.0.interior.lock().unwrap().torrents.clone()
    }

    pub fn cambiar_al_releer(&self, hash: &str, otro: Option<Value>) {
        let mut i = self.0.interior.lock().unwrap();
        i.relecturas.retain(|(h, _)| h != hash);
        i.relecturas.push((hash.into(), otro));
    }

    /// Cierra todas las sesiones abiertas, como al reiniciar qBittorrent o
    /// al caducar: la API vuelve a contestar 403 a sus cookies.
    pub fn olvidar_sesiones(&self) {
        self.0.interior.lock().unwrap().sesiones.clear();
    }

    /// Banea o perdona la IP de las pruebas (el login contesta 403).
    pub fn banear(&self, si: bool) {
        self.0.baneado.store(si, Ordering::SeqCst);
    }

    pub fn llamadas(&self) -> Vec<Llamada> {
        self.0.interior.lock().unwrap().llamadas.clone()
    }
}

pub struct Sim {
    /// `http://127.0.0.1:<puerto>`
    pub url: String,
    pub control: SimControl,
}

/// Arranca el simulador en 127.0.0.1 en un puerto libre.
pub async fn arrancar(conf: Conf) -> Sim {
    let estado = Arc::new(Estado {
        baneado: AtomicBool::new(conf.baneado),
        conf,
        interior: Mutex::default(),
    });
    let app = Router::new()
        .route("/api/v2/auth/login", post(login))
        .route("/api/v2/torrents/info", get(info))
        .route("/api/v2/torrents/stop", post(accion))
        .route("/api/v2/torrents/start", post(accion))
        .route("/api/v2/torrents/delete", post(accion))
        .with_state(estado.clone());
    let oyente = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", oyente.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(
            oyente,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    Sim {
        url,
        control: SimControl(estado),
    }
}

type Est = State<Arc<Estado>>;
type Desde = ConnectInfo<SocketAddr>;

fn apuntar(e: &Estado, ruta: &str, cuerpo: String, desde: SocketAddr) {
    e.interior.lock().unwrap().llamadas.push(Llamada {
        ruta: ruta.into(),
        cuerpo,
        desde: desde.ip(),
    });
}

fn autorizado(e: &Estado, cabeceras: &HeaderMap, desde: SocketAddr) -> bool {
    if e.conf.api_abierta || e.conf.lista_blanca.contains(&desde.ip()) {
        return true;
    }
    let sesiones = &e.interior.lock().unwrap().sesiones;
    cabeceras
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|c| c.trim().strip_prefix("SID="))
        .any(|sid| sesiones.iter().any(|s| s == sid))
}

#[derive(Deserialize)]
struct Login {
    username: String,
    password: String,
}

async fn login(State(e): Est, ConnectInfo(desde): Desde, Form(l): Form<Login>) -> Response {
    apuntar(
        &e,
        "/api/v2/auth/login",
        format!("username={}", l.username),
        desde,
    );
    if e.baneado.load(Ordering::SeqCst) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let buena = l.username == e.conf.usuario && l.password == e.conf.clave;
    if !buena && !e.conf.lista_blanca.contains(&desde.ip()) {
        if e.conf.antiguo {
            return (StatusCode::OK, "Fails.").into_response();
        }
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let mut i = e.interior.lock().unwrap();
    i.ultimo_sid += 1;
    let sid = format!("sid{}", i.ultimo_sid);
    i.sesiones.push(sid.clone());
    (
        if e.conf.antiguo {
            StatusCode::OK
        } else {
            StatusCode::NO_CONTENT
        },
        [(header::SET_COOKIE, format!("SID={sid}; HttpOnly; path=/"))],
        if e.conf.antiguo { "Ok." } else { "" },
    )
        .into_response()
}

#[derive(Deserialize)]
struct Info {
    hashes: Option<String>,
}

async fn info(
    State(e): Est,
    ConnectInfo(desde): Desde,
    h: HeaderMap,
    Query(q): Query<Info>,
) -> Response {
    apuntar(
        &e,
        "/api/v2/torrents/info",
        q.hashes.clone().unwrap_or_default(),
        desde,
    );
    if !autorizado(&e, &h, desde) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let i = e.interior.lock().unwrap();
    let ts: Vec<Value> = match q.hashes.as_deref() {
        // Como el de verdad: «hashes» vacío o ausente devuelve todos.
        None | Some("") => i.torrents.clone(),
        Some(hs) => {
            let hs: Vec<&str> = hs.split('|').collect();
            i.torrents
                .iter()
                .filter(|t| t["hash"].as_str().is_some_and(|x| hs.contains(&x)))
                .filter_map(
                    |t| match i.relecturas.iter().find(|(h, _)| t["hash"] == h.as_str()) {
                        Some((_, otro)) => otro.clone(),
                        None => Some(t.clone()),
                    },
                )
                .collect()
        }
    };
    drop(i);
    Json(ts).into_response()
}

async fn accion(
    State(e): Est,
    ConnectInfo(desde): Desde,
    uri: axum::http::Uri,
    h: HeaderMap,
    cuerpo: String,
) -> Response {
    apuntar(&e, uri.path(), cuerpo.clone(), desde);
    if !autorizado(&e, &h, desde) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let campos: Vec<(String, String)> = serde_urlencoded_simple(&cuerpo);
    let hashes: Vec<String> = campos
        .iter()
        .find(|(k, _)| k == "hashes")
        .map(|(_, v)| v.split('|').map(String::from).collect())
        .unwrap_or_default();
    let mut i = e.interior.lock().unwrap();
    let nuevo_estado = match uri.path() {
        "/api/v2/torrents/stop" => Some("stoppedUP"),
        "/api/v2/torrents/start" => Some("uploading"),
        _ => None,
    };
    match nuevo_estado {
        Some(s) => {
            for t in i
                .torrents
                .iter_mut()
                .filter(|t| hashes.iter().any(|h| t["hash"] == h.as_str()))
            {
                t["state"] = s.into();
            }
        }
        None => i
            .torrents
            .retain(|t| !hashes.iter().any(|h| t["hash"] == h.as_str())),
    }
    StatusCode::OK.into_response()
}

fn serde_urlencoded_simple(cuerpo: &str) -> Vec<(String, String)> {
    axum::extract::Query::<Vec<(String, String)>>::try_from_uri(
        &format!("/?{cuerpo}").parse().expect("cuerpo de formulario"),
    )
    .map(|q| q.0)
    .unwrap_or_default()
}
