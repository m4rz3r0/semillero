//! Arranque de semillero en proceso, con `qbt-sim` y una base de datos en un
//! directorio temporal fuera de la copia de trabajo.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Request, Response};
use leptos::prelude::LeptosOptions;
use semillero::config::Config;
use semillero::estado::AppState;
use serde_json::Value;
use tower::ServiceExt;

pub const ORIGEN: &str = "https://semillero.example";
pub const USUARIO: &str = "admin";
pub const CLAVE: &str = "secreta";
/// IP del proxy de confianza en las pruebas (no es la de [`get`] y [`post`]).
pub const PROXY: &str = "192.0.2.99";

pub struct Prueba {
    pub app: Router,
    pub state: AppState,
    pub sim: Option<qbt_sim::Sim>,
    _dir: tempfile::TempDir,
}

impl Prueba {
    pub fn sim(&self) -> &qbt_sim::Sim {
        self.sim.as_ref().expect("prueba con simulador")
    }

    /// Llamadas de login que ha recibido el simulador.
    pub fn logins_en_qbt(&self) -> usize {
        self.sim()
            .control
            .llamadas()
            .iter()
            .filter(|l| l.ruta == "/api/v2/auth/login")
            .count()
    }
}

fn config(dir: &std::path::Path, qbt: &str) -> Config {
    Config {
        escucha: "127.0.0.1:0".parse().unwrap(),
        qbt: qbt.into(),
        login_desde: Some([127, 0, 0, 1].into()),
        cuenta_qbt: None,
        db: dir.join("semillero.db"),
        estado_red: Some(dir.join("estado")),
        origenes: vec![ORIGEN.into()],
        proxy: Some(PROXY.parse().unwrap()),
        jellyfin: None,
    }
}

async fn montar(qbt: &str, sim: Option<qbt_sim::Sim>) -> Prueba {
    montar_con(qbt, sim, |_| {}).await
}

async fn montar_con(
    qbt: &str,
    sim: Option<qbt_sim::Sim>,
    ajustar: impl FnOnce(&mut Config),
) -> Prueba {
    let dir = tempfile::tempdir().unwrap();
    let site = dir.path().join("site");
    std::fs::create_dir_all(site.join("pkg")).unwrap();
    std::fs::write(site.join("pkg/semillero.css"), "main { padding: 0 }\n").unwrap();
    let leptos = LeptosOptions::builder()
        .output_name("semillero")
        .site_root(site.to_str().unwrap())
        .build();
    let mut cfg = config(dir.path(), qbt);
    ajustar(&mut cfg);
    let state = AppState::nuevo(cfg, leptos).await.unwrap();
    Prueba {
        app: semillero::construir_router(state.clone()),
        state,
        sim,
        _dir: dir,
    }
}

/// semillero contra un simulador con `conf`.
pub async fn con_sim(conf: qbt_sim::Conf) -> Prueba {
    let sim = qbt_sim::arrancar(conf).await;
    let url = sim.url.clone();
    montar(&url, Some(sim)).await
}

/// Como [`con_sim`], cambiando antes la configuración con `ajustar`.
pub async fn con_sim_y(conf: qbt_sim::Conf, ajustar: impl FnOnce(&mut Config)) -> Prueba {
    let sim = qbt_sim::arrancar(conf).await;
    let url = sim.url.clone();
    montar_con(&url, Some(sim), ajustar).await
}

/// semillero contra un qBittorrent de verdad en `url`, en modo con
/// contraseña con `cuenta` (ver `real`).
pub async fn con_qbt(url: &str, cuenta: semillero::config::ConfCuentaQbt) -> Prueba {
    montar_con(url, None, |c| {
        c.login_desde = None;
        c.cuenta_qbt = Some(cuenta);
    })
    .await
}

/// qBittorrent apagado. El puerto 1 (tcpmux) no escucha: reservar un puerto
/// libre y soltarlo dejaba que el simulador de otra prueba en paralelo lo
/// ocupara entre medias.
pub async fn sin_qbt() -> Prueba {
    montar("http://127.0.0.1:1", None).await
}

/// qBittorrent de pega que borra de verdad pero contesta 500 a
/// `torrents/delete`, como uno que se cuelga a medio borrar: semillero no
/// sabe si se borró.
#[derive(Clone, Default)]
pub struct QbtQueFallaAlBorrar {
    pub torrents: Arc<Mutex<Vec<Value>>>,
    pub borrados: Arc<Mutex<Vec<String>>>,
}

/// semillero contra un [`QbtQueFallaAlBorrar`] con `torrents`.
pub async fn con_qbt_que_falla_al_borrar(torrents: Vec<Value>) -> (Prueba, QbtQueFallaAlBorrar) {
    use axum::extract::{Query, State};
    use axum::http::StatusCode;
    use axum::routing::{get, post};

    #[derive(serde::Deserialize)]
    struct Info {
        hashes: Option<String>,
    }
    async fn info(State(q): State<QbtQueFallaAlBorrar>, Query(i): Query<Info>) -> String {
        let ts = q.torrents.lock().unwrap().clone();
        let ts: Vec<Value> = match i.hashes.as_deref() {
            None | Some("") => ts,
            Some(h) => ts.into_iter().filter(|t| t["hash"] == h).collect(),
        };
        Value::from(ts).to_string()
    }
    async fn borrar(State(q): State<QbtQueFallaAlBorrar>, cuerpo: String) -> StatusCode {
        let hash = cuerpo
            .split('&')
            .find_map(|c| c.strip_prefix("hashes="))
            .unwrap_or_default()
            .to_owned();
        q.torrents
            .lock()
            .unwrap()
            .retain(|t| t["hash"] != hash.as_str());
        q.borrados.lock().unwrap().push(hash);
        StatusCode::INTERNAL_SERVER_ERROR
    }

    let q = QbtQueFallaAlBorrar::default();
    *q.torrents.lock().unwrap() = torrents;
    let app = Router::new()
        .route("/api/v2/torrents/info", get(info))
        .route("/api/v2/torrents/delete", post(borrar))
        .route("/api/v2/torrents/stop", post(|| async { StatusCode::OK }))
        .with_state(q.clone());
    let oyente = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", oyente.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(oyente, app).await.unwrap() });
    (montar(&url, None).await, q)
}

pub async fn enviar(app: &Router, req: Request<Body>) -> Response<Body> {
    app.clone().oneshot(req).await.unwrap()
}

fn con_cliente(mut req: Request<Body>) -> Request<Body> {
    let peer: SocketAddr = "127.0.0.1:50000".parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    req
}

pub fn get(uri: &str, cookie: Option<&str>) -> Request<Body> {
    let mut b = Request::get(uri);
    if let Some(c) = cookie {
        b = b.header("cookie", c);
    }
    con_cliente(b.body(Body::empty()).unwrap())
}

/// `POST` de formulario como lo haría el navegador desde la propia web.
pub fn post(uri: &str, cuerpo: &str) -> Request<Body> {
    con_cliente(
        Request::post(uri)
            .header("content-type", "application/x-www-form-urlencoded")
            .header("origin", ORIGEN)
            .header("sec-fetch-site", "same-origin")
            .body(Body::from(cuerpo.to_owned()))
            .unwrap(),
    )
}

/// Como [`post`], pero la conexión llega desde `peer` (`IP:puerto`) con
/// `X-Forwarded-For: xff` si se da.
pub fn post_desde(uri: &str, cuerpo: &str, peer: &str, xff: Option<&str>) -> Request<Body> {
    let mut req = post(uri, cuerpo);
    let peer: SocketAddr = peer.parse().unwrap();
    req.extensions_mut().insert(ConnectInfo(peer));
    if let Some(x) = xff {
        req.headers_mut()
            .insert("x-forwarded-for", x.parse().unwrap());
    }
    req
}

pub fn cuerpo_login(usuario: &str, clave: &str) -> String {
    format!("usuario={usuario}&contrasena={clave}")
}

pub async fn texto(resp: Response<Body>) -> String {
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

pub async fn login(p: &Prueba, usuario: &str, clave: &str) -> Response<Body> {
    enviar(&p.app, post("/api/login", &cuerpo_login(usuario, clave))).await
}

/// Entra con la cuenta buena y devuelve la cookie (`nombre=valor`).
pub async fn sesion(p: &Prueba) -> String {
    let resp = login(p, USUARIO, CLAVE).await;
    assert_eq!(resp.status(), 200);
    resp.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

/// Token CSRF de la portada con la sesión `cookie`.
pub async fn token(p: &Prueba, cookie: &str) -> String {
    let resp = enviar(&p.app, get("/", Some(cookie))).await;
    assert_eq!(resp.status(), 200);
    let html = texto(resp).await;
    let i = html.find("name=\"csrf-token\"").expect("meta del token");
    let resto = &html[i..];
    let c = resto.find("content=\"").unwrap() + "content=\"".len();
    let fin = resto[c..].find('"').unwrap();
    resto[c..c + fin].to_owned()
}

/// Una base de datos en `ruta` con solo las migraciones hasta la `nivel`
/// (incluida), como la deja una versión anterior; sin abrirla con
/// [`semillero::db::abrir`], que aplicaría las demás.
pub async fn base_hasta_migracion(ruta: &std::path::Path, nivel: i64) -> sqlx::SqlitePool {
    let opciones = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(ruta)
        .create_if_missing(true);
    let pool = sqlx::SqlitePool::connect_with(opciones).await.unwrap();
    let mut m = sqlx::migrate!("./migrations");
    m.migrations = m
        .migrations
        .iter()
        .filter(|x| x.version <= nivel)
        .cloned()
        .collect::<Vec<_>>()
        .into();
    m.run(&pool).await.unwrap();
    pool
}
