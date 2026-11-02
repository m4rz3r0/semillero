//! Seguridad de las peticiones: cabeceras comunes (CSP con nonce en las
//! páginas, HSTS si el origen es https), límite de tamaño, el guardián de
//! `/api`, las páginas con sesión y qué peticiones son del propio equipo.

use std::net::IpAddr;
use std::sync::LazyLock;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Redirect, Response};

use crate::auth::{self, ERROR_SESION};
use crate::config::Config;
use crate::estado::AppState;

/// Tamaño máximo del cuerpo de una petición: los formularios de semillero
/// son pequeños.
pub const LIMITE_CUERPO: usize = 16 * 1024;

/// La subida de un preset lleva un archivo de hasta 64 KiB dentro de JSON
/// (comillas y saltos de línea ocupan el doble). La server function vuelve a
/// comprobar los 64 KiB antes de leerlo.
pub const LIMITE_CUERPO_PRESET: usize = 2 * reglas::preset::MAX_BYTES + 4 * 1024;
const RUTA_SUBIR_PRESET: &str = "/api/subir_preset";

/// Server functions a las que se llega sin sesión.
const API_PUBLICA: [&str; 1] = ["/api/login"];

const SCRIPT_SRC: &str = "'self' 'wasm-unsafe-eval'";

/// Política de seguridad de contenidos. Con `nonce`, `script-src` admite
/// además los scripts en línea que Leptos marca con ese nonce.
pub fn csp(nonce: Option<&str>) -> String {
    let script_src = match nonce {
        Some(n) => format!("{SCRIPT_SRC} 'nonce-{n}'"),
        None => SCRIPT_SRC.to_owned(),
    };
    format!(
        "default-src 'self'; script-src {script_src}; style-src 'self'; img-src 'self' data:; \
         connect-src 'self'; font-src 'self'; form-action 'self'; base-uri 'none'; \
         frame-ancestors 'none'"
    )
}

static CSP_SIN_NONCE: LazyLock<HeaderValue> =
    LazyLock::new(|| HeaderValue::from_str(&csp(None)).expect("CSP válida como cabecera"));

const OTRAS_CABECERAS: [(HeaderName, &str); 3] = [
    // `same-origin` y no `no-referrer`: con `no-referrer` los navegadores
    // mandan `Origin: null` en los POST de la propia web.
    (header::REFERRER_POLICY, "same-origin"),
    (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
    (header::X_FRAME_OPTIONS, "DENY"),
];

/// HSTS durante un año. Sin `includeSubDomains`: semillero no sabe qué más
/// hay bajo su dominio.
const HSTS: &str = "max-age=31536000";

/// ¿Llega la petición por el nombre de uno de los orígenes `https://`?
/// Solo entonces tiene sentido HSTS: el acceso local por http no lo lleva.
fn por_https(cabeceras: &HeaderMap, origenes: &[String]) -> bool {
    let Some(host) = cabeceras
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_ascii_lowercase)
    else {
        return false;
    };
    origenes
        .iter()
        .filter_map(|o| o.strip_prefix("https://"))
        .map(str::to_ascii_lowercase)
        .any(|o| host == o || host == format!("{o}:443"))
}

/// Añade las cabeceras de seguridad a cada respuesta (y HSTS si llegó por
/// un origen https). Respeta la CSP que ya traiga (las páginas la ponen con
/// su nonce).
pub async fn cabeceras(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let hsts = por_https(req.headers(), &state.cfg.origenes);
    let mut resp = poner_cabeceras(next.run(req).await);
    if hsts {
        resp.headers_mut().insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static(HSTS),
        );
    }
    resp
}

fn poner_cabeceras(mut resp: Response) -> Response {
    let h = resp.headers_mut();
    h.entry(header::CONTENT_SECURITY_POLICY)
        .or_insert_with(|| CSP_SIN_NONCE.clone());
    for (nombre, valor) in OTRAS_CABECERAS {
        h.insert(nombre, HeaderValue::from_static(valor));
    }
    if !h.contains_key(header::CACHE_CONTROL) {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    resp
}

/// Rechaza con 413 las peticiones de más de [`LIMITE_CUERPO`] bytes
/// ([`LIMITE_CUERPO_PRESET`] en la subida de presets).
pub async fn limitar_cuerpo(req: Request, next: Next) -> Response {
    let limite = if req.uri().path() == RUTA_SUBIR_PRESET {
        LIMITE_CUERPO_PRESET
    } else {
        LIMITE_CUERPO
    };
    let declarado = req
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if declarado.is_some_and(|n| n > limite as u64) {
        return demasiado_grande();
    }
    let (partes, cuerpo) = req.into_parts();
    match axum::body::to_bytes(cuerpo, limite).await {
        Ok(bytes) => {
            next.run(Request::from_parts(partes, Body::from(bytes)))
                .await
        }
        Err(_) => demasiado_grande(),
    }
}

fn demasiado_grande() -> Response {
    (
        StatusCode::PAYLOAD_TOO_LARGE,
        "La petición es demasiado grande.",
    )
        .into_response()
}

/// ¿Viene la petición de la propia web? Hace falta `Sec-Fetch-Site` o
/// `Origin`: si está `Sec-Fetch-Site`, todas sus copias deben ser
/// `same-origin`; si no, cada copia de `Origin` debe ser uno de los
/// `origenes` públicos (`Origin: null` no vale). Sin ninguna de las dos, no.
pub fn mismo_origen(cabeceras: &HeaderMap, origenes: &[String]) -> bool {
    let sitios = cabeceras.get_all("sec-fetch-site");
    if sitios.iter().next().is_some() {
        return sitios.iter().all(|v| v.as_bytes() == b"same-origin");
    }
    let mut vistos = cabeceras.get_all(header::ORIGIN).iter().peekable();
    vistos.peek().is_some() && vistos.all(|v| origenes.iter().any(|o| v.as_bytes() == o.as_bytes()))
}

/// El guardián de todo `/api`:
/// - lo que cambia algo (todo salvo `GET`, `HEAD` y `OPTIONS`) tiene que
///   venir de la propia web ([`mismo_origen`]): si no, 403;
/// - salvo `/api/login`, exige sesión vigente (si no, 401) y, en lo que
///   cambia algo, el token CSRF de la sesión en `x-csrf-token` (si no, 403).
///
/// La sesión queda en las extensiones de la petición para la server function.
pub async fn guardian(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    let ruta = req.uri().path();
    if !ruta.starts_with("/api/") {
        return next.run(req).await;
    }
    let cambia = !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
    if cambia && !mismo_origen(req.headers(), &state.cfg.origenes) {
        return (StatusCode::FORBIDDEN, "Origen no permitido.").into_response();
    }
    if API_PUBLICA.contains(&ruta) {
        return next.run(req).await;
    }
    let sesion = match auth::sesion_de_cabeceras(&state.db, req.headers()).await {
        Ok(Some(s)) => s,
        Ok(None) => return (StatusCode::UNAUTHORIZED, ERROR_SESION).into_response(),
        Err(e) => {
            tracing::error!("error interno en el guardián: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };
    if cambia && !auth::token_valido(req.headers(), &sesion) {
        return (StatusCode::FORBIDDEN, "Falta el token de seguridad.").into_response();
    }
    req.extensions_mut().insert(sesion);
    next.run(req).await
}

/// Páginas: todo exige sesión salvo `/login`, que con sesión lleva a la
/// portada, `/salud` y los ficheros estáticos. Sin sesión, a `/login`.
/// `/api` lo protege [`guardian`].
pub async fn paginas(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    let ruta = req.uri().path();
    if ruta.starts_with("/api/") || ruta == "/salud" || estatico(ruta) {
        return next.run(req).await;
    }
    let login = ruta == "/login";
    match auth::sesion_de_cabeceras(&state.db, req.headers()).await {
        Ok(Some(_)) if login => Redirect::to("/").into_response(),
        Ok(None) if login => next.run(req).await,
        Ok(Some(sesion)) => {
            req.extensions_mut().insert(sesion);
            next.run(req).await
        }
        Ok(None) => Redirect::to("/login").into_response(),
        Err(e) => {
            tracing::error!("error interno en las páginas: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Ficheros estáticos públicos (la web compilada y las fuentes), sin
/// segmentos `.`, `..` ni vacíos: con ellos la ruta podría acabar en una
/// página.
fn estatico(ruta: &str) -> bool {
    ["/pkg/", "/fuentes/"].iter().any(|p| {
        ruta.strip_prefix(p)
            .is_some_and(|r| r.split('/').all(|s| !matches!(s, "" | "." | "..")))
    })
}

/// ¿Es una petición del propio equipo, hecha directamente? Desde una
/// dirección de bucle local o desde la misma IP en la que escucha semillero
/// (el actualizador la hace así), y no a través del proxy: si viene de él,
/// o trae cabeceras de reenvío, es de fuera aunque diga lo contrario.
pub fn peticion_local(peer: IpAddr, cabeceras: &HeaderMap, cfg: &Config) -> bool {
    let reenviada = ["x-forwarded-for", "forwarded", "x-real-ip"]
        .iter()
        .any(|c| cabeceras.contains_key(*c));
    let peer = peer.to_canonical();
    if reenviada || cfg.proxy.map(|p| p.to_canonical()) == Some(peer) {
        return false;
    }
    let escucha = cfg.escucha.ip().to_canonical();
    peer.is_loopback() || (!escucha.is_unspecified() && peer == escucha)
}
