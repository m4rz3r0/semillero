//! Servidor del acceso (feature `ssr`).

use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::extract::{ConnectInfo, Extension};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use leptos::prelude::*;
use leptos_axum::ResponseOptions;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::estado::AppState;

/// Nombre de la cookie de sesión.
pub const SESSION_COOKIE: &str = "__Host-sm_session";
/// Cabecera con el token CSRF de la sesión en las peticiones que cambian algo.
pub const CABECERA_CSRF: &str = "x-csrf-token";
/// Ventana de reautenticación: tras volver a escribir la contraseña, borrar a
/// mano o activar el borrado real no la piden de nuevo durante este tiempo.
pub const REAUTH_S: i64 = 5 * 60;
/// Una contraseña más larga es un fallo normal, sin preguntar a qBittorrent.
const MAX_CLAVE_BYTES: usize = 1024;
const MAX_USUARIO_CHARS: usize = 64;
/// Caducidad por inactividad.
const SESION_INACTIVA_S: i64 = 30 * 60;
/// Caducidad absoluta.
const SESION_MAX_S: i64 = 12 * 60 * 60;
/// `vista` se actualiza como mucho una vez por minuto.
const TOCAR_CADA_S: i64 = 60;
/// Espera máxima del freno.
const ESPERA_MAX_S: i64 = 60;
/// Los contadores sin fallos nuevos durante un día se olvidan.
const OLVIDAR_FALLOS_MS: i64 = 24 * 60 * 60 * 1000;
/// Longitud del usuario y la contraseña al azar del canario.
const LARGO_CANARIO: usize = 24;

fn ahora_ms() -> i64 {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("reloj posterior a 1970");
    i64::try_from(d.as_millis()).expect("milisegundos en i64")
}

fn ahora_s() -> i64 {
    ahora_ms() / 1000
}

/// Resultado del canario de arranque: un usuario y una contraseña al azar
/// que qBittorrent tiene que rechazar. Si los acepta (el servidor sale desde
/// una IP de su lista blanca, o se salta la autenticación en local),
/// cualquier contraseña valdría y el acceso queda cerrado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Canario {
    /// Aún no se ha podido comprobar.
    Pendiente,
    /// qBittorrent rechazó la contraseña al azar.
    Bueno,
    /// qBittorrent la aceptó: login desactivado hasta reiniciar.
    Malo,
}

/// Por qué no se entra.
#[derive(Debug, PartialEq, Eq)]
pub enum ErrorLogin {
    /// El freno está activo: faltan estos segundos.
    Frenado(i64),
    /// Usuario o contraseña incorrectos (no se distingue).
    Credenciales,
    QbtNoResponde,
    /// qBittorrent contestó, pero no con un sí o un no (IP bloqueada, error).
    QbtRechaza,
    /// El canario falló: qBittorrent acepta cualquier contraseña.
    MalConfigurado,
    Interno(String),
}

impl From<sqlx::Error> for ErrorLogin {
    fn from(e: sqlx::Error) -> Self {
        ErrorLogin::Interno(e.to_string())
    }
}

impl From<qbt::Error> for ErrorLogin {
    fn from(e: qbt::Error) -> Self {
        match e {
            qbt::Error::NoResponde(_) => ErrorLogin::QbtNoResponde,
            _ => ErrorLogin::QbtRechaza,
        }
    }
}

impl ErrorLogin {
    /// Texto para la persona que intenta entrar.
    pub fn mensaje(&self) -> String {
        match self {
            ErrorLogin::Frenado(s) => {
                format!("Demasiados intentos seguidos. Espera {s} s y vuelve a probar.")
            }
            ErrorLogin::Credenciales => "Usuario o contraseña incorrectos.".into(),
            ErrorLogin::QbtNoResponde => {
                "qBittorrent no responde y no se puede comprobar la contraseña. \
                 Prueba de nuevo en un momento."
                    .into()
            }
            ErrorLogin::QbtRechaza => {
                "qBittorrent rechazó la comprobación de la contraseña (puede que haya \
                 bloqueado los intentos durante un tiempo). Prueba más tarde."
                    .into()
            }
            ErrorLogin::MalConfigurado => {
                "El acceso está desactivado: el servidor está mal configurado y \
                 qBittorrent aceptaría cualquier contraseña."
                    .into()
            }
            ErrorLogin::Interno(_) => "Error interno. Prueba de nuevo.".into(),
        }
    }

    pub fn estado(&self) -> StatusCode {
        match self {
            ErrorLogin::Frenado(_) => StatusCode::TOO_MANY_REQUESTS,
            ErrorLogin::Credenciales => StatusCode::UNAUTHORIZED,
            ErrorLogin::QbtNoResponde | ErrorLogin::MalConfigurado => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            ErrorLogin::QbtRechaza => StatusCode::BAD_GATEWAY,
            ErrorLogin::Interno(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

// ---------------------------------------------------------------- sesiones

/// Sesión vigente de una petición. El guardián de `/api` la deja en las
/// extensiones de la petición.
#[derive(Clone, Debug)]
pub struct Sesion {
    pub usuario: String,
    /// Token CSRF de la sesión (ver [`token_csrf`]).
    pub csrf: String,
    /// SHA-256 del id: la clave de la fila.
    clave: String,
}

fn id_nuevo() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("generador aleatorio del sistema");
    URL_SAFE_NO_PAD.encode(bytes)
}

fn clave_de(id: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(id.as_bytes()))
}

/// Token CSRF de la sesión `id`: se deriva del id (que solo está en la
/// cookie `HttpOnly`), así que no hace falta guardarlo y otra web no lo
/// puede saber.
fn token_csrf(id: &str) -> String {
    let mut h = Sha256::new();
    h.update(b"semillero-csrf:");
    h.update(id.as_bytes());
    URL_SAFE_NO_PAD.encode(h.finalize())
}

/// Comparación en tiempo constante.
pub(crate) fn iguales(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn cookie_sesion(id: &str) -> String {
    format!(
        "{SESSION_COOKIE}={id}; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age={SESION_MAX_S}"
    )
}

fn cookie_caducada() -> String {
    format!("{SESSION_COOKIE}=; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=0")
}

/// Id de sesión de las cabeceras `Cookie`, si lo hay.
fn id_de_cookie(cabeceras: &HeaderMap) -> Option<&str> {
    cabeceras
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|par| par.trim().split_once('='))
        .find(|(nombre, _)| *nombre == SESSION_COOKIE)
        .map(|(_, id)| id)
        .filter(|id| !id.is_empty())
}

async fn purgar(db: &SqlitePool) -> Result<(), sqlx::Error> {
    let ahora = ahora_s();
    sqlx::query("DELETE FROM sesiones WHERE vista <= ? OR creada <= ?")
        .bind(ahora - SESION_INACTIVA_S)
        .bind(ahora - SESION_MAX_S)
        .execute(db)
        .await?;
    sqlx::query("DELETE FROM freno WHERE ultimo_fallo_ms <= ?")
        .bind(ahora_ms() - OLVIDAR_FALLOS_MS)
        .execute(db)
        .await?;
    Ok(())
}

async fn crear_sesion(db: &SqlitePool, usuario: &str) -> Result<String, sqlx::Error> {
    purgar(db).await?;
    let id = id_nuevo();
    let ahora = ahora_s();
    sqlx::query("INSERT INTO sesiones (id, usuario, creada, vista) VALUES (?, ?, ?, ?)")
        .bind(clave_de(&id))
        .bind(usuario)
        .bind(ahora)
        .bind(ahora)
        .execute(db)
        .await?;
    Ok(id)
}

/// Sesión de la cookie de `cabeceras`, si sigue vigente. Una caducada se
/// borra; una vigente renueva su última vista.
pub async fn sesion_de_cabeceras(
    db: &SqlitePool,
    cabeceras: &HeaderMap,
) -> Result<Option<Sesion>, sqlx::Error> {
    let Some(id) = id_de_cookie(cabeceras) else {
        return Ok(None);
    };
    let clave = clave_de(id);
    let fila: Option<(String, i64, i64)> =
        sqlx::query_as("SELECT usuario, creada, vista FROM sesiones WHERE id = ?")
            .bind(&clave)
            .fetch_optional(db)
            .await?;
    let Some((usuario, creada, vista)) = fila else {
        return Ok(None);
    };
    let ahora = ahora_s();
    if ahora - vista >= SESION_INACTIVA_S || ahora - creada >= SESION_MAX_S {
        sqlx::query("DELETE FROM sesiones WHERE id = ?")
            .bind(&clave)
            .execute(db)
            .await?;
        return Ok(None);
    }
    if ahora - vista >= TOCAR_CADA_S {
        sqlx::query("UPDATE sesiones SET vista = ? WHERE id = ?")
            .bind(ahora)
            .bind(&clave)
            .execute(db)
            .await?;
    }
    Ok(Some(Sesion {
        usuario,
        csrf: token_csrf(id),
        clave,
    }))
}

/// ¿Trae la petición el token CSRF de `sesion`?
pub fn token_valido(cabeceras: &HeaderMap, sesion: &Sesion) -> bool {
    let mut tokens = cabeceras.get_all(CABECERA_CSRF).iter();
    match (tokens.next(), tokens.next()) {
        (Some(t), None) => iguales(t.as_bytes(), sesion.csrf.as_bytes()),
        _ => false,
    }
}

// ---------------------------------------------------------------- freno

/// Espera tras `fallos` fallos seguidos: `min(2^(fallos-1), 60)` s.
pub fn espera_s(fallos: i64) -> i64 {
    match fallos {
        ..=0 => 0,
        1..=6 => (1 << (fallos - 1)).min(ESPERA_MAX_S),
        _ => ESPERA_MAX_S,
    }
}

/// IP real del cliente para el freno: si la conexión viene del proxy de
/// confianza, la última entrada de `X-Forwarded-For` (la que puso el
/// proxy); si no, si no hay proxy configurado, o si la cabecera falta o no
/// se entiende, la IP de la conexión.
pub fn ip_cliente(peer: IpAddr, cabeceras: &HeaderMap, proxy: Option<IpAddr>) -> IpAddr {
    let peer = peer.to_canonical();
    if proxy.map(|p| p.to_canonical()) != Some(peer) {
        return peer;
    }
    cabeceras
        .get_all("x-forwarded-for")
        .iter()
        .next_back()
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .and_then(|ip| ip.trim().parse::<IpAddr>().ok())
        .map(|ip| ip.to_canonical())
        .unwrap_or(peer)
}

/// Clave del freno para el cliente `ip`: la IPv4 tal cual; en IPv6, su red
/// /64 (un equipo cambia de dirección dentro de su /64 cuando quiere, así
/// que frenar por dirección no frenaría nada).
pub fn clave_freno(ip: IpAddr) -> String {
    match ip.to_canonical() {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => {
            let red = u128::from(v6) & !((1u128 << 64) - 1);
            format!("{}/64", std::net::Ipv6Addr::from(red))
        }
    }
}

async fn espera_ms(db: &SqlitePool, ip: &str) -> Result<i64, sqlx::Error> {
    let fila: Option<(i64, i64)> =
        sqlx::query_as("SELECT fallos, ultimo_fallo_ms FROM freno WHERE tipo = 'ip' AND clave = ?")
            .bind(ip)
            .fetch_optional(db)
            .await?;
    Ok(fila
        .map(|(fallos, ultimo)| ultimo + espera_s(fallos) * 1000 - ahora_ms())
        .unwrap_or(0)
        .max(0))
}

async fn apuntar_fallo(db: &SqlitePool, ip: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO freno (tipo, clave, fallos, ultimo_fallo_ms) VALUES ('ip', ?, 1, ?)
         ON CONFLICT (tipo, clave) DO UPDATE
         SET fallos = fallos + 1, ultimo_fallo_ms = excluded.ultimo_fallo_ms",
    )
    .bind(ip)
    .bind(ahora_ms())
    .execute(db)
    .await?;
    Ok(())
}

async fn borrar_fallos(db: &SqlitePool, ip: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM freno WHERE tipo = 'ip' AND clave = ?")
        .bind(ip)
        .execute(db)
        .await?;
    Ok(())
}

// ---------------------------------------------------------------- canario

/// Texto al azar de `n` letras y cifras.
fn al_azar(n: usize) -> String {
    const ALFABETO: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut bytes = vec![0u8; n];
    getrandom::fill(&mut bytes).expect("generador aleatorio del sistema");
    bytes
        .into_iter()
        .map(|b| ALFABETO[usize::from(b) % ALFABETO.len()] as char)
        .collect()
}

/// Comprueba el canario si aún no se pudo. Hay que llamarla con el turno
/// de `state.acceso` tomado (`canario` es lo que guarda). Solo pregunta a
/// qBittorrent mientras está pendiente: una vez por arranque si contesta,
/// para no sumar fallos a su contador de bloqueos.
async fn canario_con_turno(state: &AppState, canario: &mut Canario) -> Result<(), ErrorLogin> {
    match *canario {
        Canario::Bueno => Ok(()),
        Canario::Malo => Err(ErrorLogin::MalConfigurado),
        Canario::Pendiente => {
            let usuario = al_azar(LARGO_CANARIO);
            let clave = al_azar(LARGO_CANARIO);
            match state.qbt.credenciales_validas(&usuario, &clave).await {
                Ok(true) => {
                    let desde = state
                        .cfg
                        .login_desde_efectivo()
                        .map_or_else(|| "este equipo".to_owned(), |ip| ip.to_string());
                    tracing::error!(
                        "qBittorrent ha aceptado un usuario y una contraseña al azar desde {desde}: \
                         cualquier contraseña valdría. El acceso queda desactivado; quita esa IP \
                         de su lista blanca y de «saltar autenticación en local» y reinicia"
                    );
                    *canario = Canario::Malo;
                    Err(ErrorLogin::MalConfigurado)
                }
                Ok(false) => {
                    tracing::info!("canario de acceso: qBittorrent rechaza una contraseña al azar");
                    *canario = Canario::Bueno;
                    Ok(())
                }
                Err(e) => {
                    tracing::warn!("canario de acceso sin comprobar: {e}");
                    Err(e.into())
                }
            }
        }
    }
}

/// El canario del arranque: una sola comprobación. Si qBittorrent no
/// contesta, se repite en el siguiente intento de entrar.
pub async fn comprobar_canario(state: &AppState) -> Canario {
    let mut canario = state.acceso.lock().await;
    let _ = canario_con_turno(state, &mut canario).await;
    *canario
}

// ---------------------------------------------------------------- login

/// Comprueba la contraseña de `usuario` contra qBittorrent, con el freno de
/// la IP del cliente `ip` (ver [`ip_cliente`] y [`clave_freno`]) y el
/// canario. Hay que llamarla
/// con el turno de `state.acceso` tomado.
async fn comprobar(
    state: &AppState,
    canario: &mut Canario,
    ip: IpAddr,
    usuario: &str,
    clave: &str,
) -> Result<(), ErrorLogin> {
    let db = &state.db;
    let clave_ip = clave_freno(ip);

    let espera = espera_ms(db, &clave_ip).await?;
    if espera > 0 {
        return Err(ErrorLogin::Frenado((espera + 999) / 1000));
    }
    if usuario.is_empty()
        || clave.is_empty()
        || clave.len() > MAX_CLAVE_BYTES
        || usuario.chars().count() > MAX_USUARIO_CHARS
    {
        apuntar_fallo(db, &clave_ip).await?;
        return Err(ErrorLogin::Credenciales);
    }
    canario_con_turno(state, canario).await?;
    if state.qbt.credenciales_validas(usuario, clave).await? {
        borrar_fallos(db, &clave_ip).await?;
        Ok(())
    } else {
        apuntar_fallo(db, &clave_ip).await?;
        Err(ErrorLogin::Credenciales)
    }
}

/// Intenta entrar desde `ip`; devuelve el id de la sesión nueva.
pub async fn intentar_login(
    state: &AppState,
    ip: IpAddr,
    usuario: &str,
    clave: &str,
) -> Result<String, ErrorLogin> {
    let mut canario = state.acceso.lock().await;
    comprobar(state, &mut canario, ip, usuario, clave).await?;
    Ok(crear_sesion(&state.db, usuario).await?)
}

/// Vuelve a pedir la contraseña al usuario de `sesion` (antes de borrar a
/// mano o de activar el borrado real), con el mismo freno que el login. Si
/// es buena, abre la ventana de reautenticación durante [`REAUTH_S`].
pub async fn reautenticar(
    state: &AppState,
    ip: IpAddr,
    sesion: &Sesion,
    clave: &str,
) -> Result<(), ErrorLogin> {
    let mut canario = state.acceso.lock().await;
    comprobar(state, &mut canario, ip, &sesion.usuario, clave).await?;
    sqlx::query("UPDATE sesiones SET reautenticado_hasta = ? WHERE id = ?")
        .bind(ahora_s() + REAUTH_S)
        .bind(&sesion.clave)
        .execute(&state.db)
        .await?;
    Ok(())
}

/// ¿Tiene `sesion` abierta la ventana de reautenticación?
pub async fn reautenticacion_vigente(
    db: &SqlitePool,
    sesion: &Sesion,
) -> Result<bool, sqlx::Error> {
    let hasta: Option<Option<i64>> =
        sqlx::query_scalar("SELECT reautenticado_hasta FROM sesiones WHERE id = ?")
            .bind(&sesion.clave)
            .fetch_optional(db)
            .await?;
    Ok(hasta.flatten().is_some_and(|h| h > ahora_s()))
}

// ---------------------------------------------------------------- server functions

pub(crate) fn app_state() -> Result<AppState, ServerFnError> {
    use_context::<AppState>().ok_or_else(|| ServerFnError::new("falta el estado del servidor"))
}

/// Fija el estado HTTP de la respuesta y devuelve el error con `msg`.
pub(crate) fn fallar<T>(estado: StatusCode, msg: &str) -> Result<T, ServerFnError> {
    if let Some(resp) = use_context::<ResponseOptions>() {
        resp.set_status(estado);
    }
    Err(ServerFnError::new(msg))
}

fn interno<T>(e: impl fmt::Display) -> Result<T, ServerFnError> {
    tracing::error!("error interno en el acceso: {e}");
    fallar(
        StatusCode::INTERNAL_SERVER_ERROR,
        &ErrorLogin::Interno(String::new()).mensaje(),
    )
}

fn poner_cookie(valor: &str) {
    if let (Some(resp), Ok(valor)) = (
        use_context::<ResponseOptions>(),
        HeaderValue::from_str(valor),
    ) {
        resp.insert_header(header::SET_COOKIE, valor);
    }
}

/// ¿Lo envió un formulario HTML sin JavaScript? (pide `text/html`).
fn sin_javascript(cabeceras: &HeaderMap) -> bool {
    cabeceras
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("text/html"))
}

pub(super) async fn login_endpoint(
    usuario: String,
    contrasena: String,
) -> Result<(), ServerFnError> {
    let state = app_state()?;
    let ConnectInfo(peer) = leptos_axum::extract::<ConnectInfo<SocketAddr>>().await?;
    let cabeceras: HeaderMap = leptos_axum::extract().await?;
    let ip = ip_cliente(peer.ip(), &cabeceras, state.cfg.proxy);
    let sin_js = sin_javascript(&cabeceras);
    let estado = |s: StatusCode| if sin_js { StatusCode::SEE_OTHER } else { s };
    match intentar_login(&state, ip, &usuario, &contrasena).await {
        Ok(id) => {
            // La sesión anterior de este navegador, si la había, se cierra.
            if let Some(viejo) = id_de_cookie(&cabeceras)
                && let Err(e) = sqlx::query("DELETE FROM sesiones WHERE id = ?")
                    .bind(clave_de(viejo))
                    .execute(&state.db)
                    .await
            {
                return interno(e);
            }
            poner_cookie(&cookie_sesion(&id));
            // Sin usuario ni IP: el registro no lleva datos personales.
            tracing::info!("sesión iniciada");
            if sin_js {
                leptos_axum::redirect("/");
            }
            Ok(())
        }
        Err(ErrorLogin::Interno(e)) => interno(e),
        Err(e) => {
            tracing::info!("login rechazado: {e:?}");
            fallar(estado(e.estado()), &e.mensaje())
        }
    }
}

pub(super) async fn logout_endpoint() -> Result<(), ServerFnError> {
    let state = app_state()?;
    let Extension(sesion) = leptos_axum::extract::<Extension<Sesion>>().await?;
    poner_cookie(&cookie_caducada());
    if let Err(e) = sqlx::query("DELETE FROM sesiones WHERE id = ?")
        .bind(&sesion.clave)
        .execute(&state.db)
        .await
    {
        return interno(e);
    }
    Ok(())
}
