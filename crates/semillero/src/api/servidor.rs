//! Lo que hacen las server functions de [`crate::api`] (feature `ssr`).
//!
//! El guardián de `/api` ya exigió sesión y token; aquí se lee la sesión de
//! la petición para la reautenticación.

use std::fmt;
use std::net::SocketAddr;
use std::path::Path;

use axum::extract::{ConnectInfo, Extension, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use reglas::preset::{self, Preset};
use reglas::{Estado, NormasTracker, ReglasCategoria, TipoRegla};

use super::*;
use crate::auth::{self, ErrorLogin, Sesion, app_state, fallar};
use crate::db::{self, Evento, Modo};
use crate::estado::AppState;
use crate::tarea::{self, NoQuitado, ahora_unix};

/// Líneas del historial que se enseñan.
const LINEAS_HISTORIAL: i64 = 200;
/// Sin regla de espacio, por debajo de este porcentaje libre el disco se
/// marca como bajo.
const DISCO_BAJO_PCT: f64 = 10.0;

fn interno<T>(e: impl fmt::Display) -> Result<T, ServerFnError> {
    tracing::error!("error interno en la interfaz: {e}");
    fallar(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Error interno. Prueba de nuevo.",
    )
}

async fn sesion() -> Result<Sesion, ServerFnError> {
    let Extension(s) = leptos_axum::extract::<Extension<Sesion>>().await?;
    Ok(s)
}

fn hash_bueno(hash: &str) -> Result<(), ServerFnError> {
    if qbt::hash_valido(hash) {
        Ok(())
    } else {
        fallar(StatusCode::BAD_REQUEST, "Ese torrent no existe.")
    }
}

/// Exige la contraseña reciente: con `clave`, la comprueba (con el freno de
/// la IP del cliente) y abre la ventana de 5 minutos; sin ella, la ventana
/// tiene que estar abierta (si no, [`ERROR_REAUTH`]).
async fn exigir_contrasena(
    state: &AppState,
    sesion: &Sesion,
    clave: Option<String>,
) -> Result<(), ServerFnError> {
    let Some(clave) = clave else {
        return match auth::reautenticacion_vigente(&state.db, sesion).await {
            Ok(true) => Ok(()),
            Ok(false) => fallar(StatusCode::FORBIDDEN, ERROR_REAUTH),
            Err(e) => interno(e),
        };
    };
    let ConnectInfo(peer) = leptos_axum::extract::<ConnectInfo<SocketAddr>>().await?;
    let cabeceras: HeaderMap = leptos_axum::extract().await?;
    let ip = auth::ip_cliente(peer.ip(), &cabeceras, state.cfg.proxy);
    match auth::reautenticar(state, ip, sesion, &clave).await {
        Ok(()) => Ok(()),
        Err(ErrorLogin::Credenciales) => {
            fallar(StatusCode::FORBIDDEN, "La contraseña no es correcta.")
        }
        Err(ErrorLogin::Interno(e)) => interno(e),
        Err(e) => fallar(e.estado(), &e.mensaje()),
    }
}

/// Con el borrado real activo, lo que cambia qué se borra (aplicar un
/// preset, guardar normas o reglas) exige la contraseña reciente, como
/// activarlo (ver [`exigir_contrasena`]). En modo solo aviso no la pide.
async fn exigir_contrasena_si_borrado_real(
    state: &AppState,
    clave: Option<String>,
) -> Result<(), ServerFnError> {
    if db::modo(&state.db).await.or_else(interno)? == Modo::BorradoReal {
        let sesion = sesion().await?;
        exigir_contrasena(state, &sesion, clave).await?;
    }
    Ok(())
}

// ---------------------------------------------------------------- portada

pub(super) async fn portada() -> Result<DatosPortada, ServerFnError> {
    // También aquí: la página las pide al pintarse en el servidor.
    sesion().await?;
    let state = app_state()?;
    let ahora = ahora_unix();
    let red = crate::estado_red::leer(state.cfg.estado_red.as_deref(), ahora as f64);
    let lista = state.qbt.torrents().await.map_err(|e| {
        tracing::warn!("portada sin torrents: {e}");
        e
    });
    match armar_portada(&state, ahora, red, lista).await {
        Ok(p) => Ok(p),
        Err(e) => interno(e),
    }
}

async fn armar_portada(
    state: &AppState,
    ahora: i64,
    red: crate::estado_red::EstadoRed,
    lista: Result<Vec<qbt::TorrentQbt>, qbt::Error>,
) -> Result<DatosPortada, sqlx::Error> {
    let db = &state.db;
    let qbt_ok = lista.is_ok();
    let qbt_clave_mala = matches!(lista, Err(qbt::Error::CuentaRechazada));
    let lista = lista.unwrap_or_default();
    let juego = db::juego_normas(db).await?;
    let vistos = db::trackers_vistos(db).await?;
    let reglas_cat = db::reglas(db).await?;
    let avisos = db::avisos(db).await?;
    let exenciones = db::exenciones(db).await?;

    let mut torrents = Vec::with_capacity(lista.len());
    for t in &lista {
        let tracker = tarea::host_de(t, &vistos);
        let normas = *juego.para(tracker);
        let c = reglas::clasificar(&tarea::torrent_de_reglas(db, t).await?, &normas);
        torrents.push(FichaTorrent {
            hash: t.hash.clone(),
            nombre: t.name.clone(),
            categoria: nombre_categoria(&t.category),
            estado: c.estado,
            actividad: Actividad::de_qbt(&t.state),
            ratio: t.ratio,
            siembra_s: t.seeding_time,
            falta_s: c.falta_s,
            tamano: t.total_size,
            aviso: avisos.iter().find(|a| a.hash == t.hash).map(|a| a.regla),
            exenciones: {
                let mut v: Vec<TipoRegla> = exenciones
                    .iter()
                    .filter(|(h, _)| *h == t.hash)
                    .map(|(_, r)| *r)
                    .collect();
                v.sort();
                v
            },
            tracker: tracker.to_owned(),
            normas,
        });
    }
    torrents.sort_by_key(|t| t.nombre.to_lowercase());

    let mut fichas_avisos = Vec::with_capacity(avisos.len());
    for a in &avisos {
        let nombre = match lista.iter().find(|t| t.hash == a.hash) {
            Some(t) => t.name.clone(),
            None => db::ultimo_nombre(db, &a.hash).await?,
        };
        fichas_avisos.push(FichaAviso {
            hash: a.hash.clone(),
            nombre,
            regla: a.regla,
            vence_en: a.vence_en,
        });
    }

    // Discos: uno por categoría con torrents, en el orden de las tarjetas.
    let mut categorias: Vec<&str> = CATEGORIAS.iter().map(|(c, _)| *c).collect();
    for t in &lista {
        if !categorias.contains(&t.category.as_str()) {
            categorias.push(&t.category);
        }
    }
    let discos = categorias
        .into_iter()
        .filter_map(|cat| {
            let ruta = tarea::ruta_de_categoria(&lista, cat)?;
            let d = crate::disco::medir(Path::new(ruta))?;
            let minimo = reglas_cat
                .get(cat)
                .and_then(|r| r.disco_libre_pct)
                .unwrap_or(DISCO_BAJO_PCT);
            let pct = if d.total > 0 {
                d.libre as f64 * 100.0 / d.total as f64
            } else {
                0.0
            };
            Some(FichaDisco {
                categoria: nombre_categoria(cat),
                libre: d.libre,
                total: d.total,
                bajo: pct < minimo,
            })
        })
        .collect();

    Ok(DatosPortada {
        ahora,
        normas: juego.defecto,
        franja: Franja {
            vigilada: red.vigilada,
            vpn: red.vpn,
            puerto: red.puerto.is_some(),
            qbt: qbt_ok,
            qbt_clave_mala,
            discos,
        },
        modo_real: db::modo(db).await? == Modo::BorradoReal,
        torrents,
        avisos: fichas_avisos,
    })
}

// ---------------------------------------------------------------- acciones

pub(super) async fn pausar(hash: String, parar: bool) -> Result<(), ServerFnError> {
    let state = app_state()?;
    hash_bueno(&hash)?;
    let hashes = [hash];
    let r = if parar {
        state.qbt.parar(&hashes).await
    } else {
        state.qbt.reanudar(&hashes).await
    };
    r.or_else(|e| {
        tracing::warn!("no se pudo parar o reanudar: {e}");
        fallar(
            StatusCode::BAD_GATEWAY,
            "qBittorrent no lo ha hecho. Prueba de nuevo en un momento.",
        )
    })
}

pub(super) async fn quitar(
    hash: String,
    ficheros: bool,
    clave: Option<String>,
) -> Result<(), ServerFnError> {
    let state = app_state()?;
    hash_bueno(&hash)?;
    let sesion = sesion().await?;
    exigir_contrasena(&state, &sesion, clave).await?;
    let r = tarea::quitar_a_mano(&state, &state.mediateca, &hash, ficheros, ahora_unix()).await;
    match r {
        Ok(()) => {
            tracing::info!(hash = %hash, ficheros, "quitado a mano");
            Ok(())
        }
        Err(NoQuitado::HashMalo) => fallar(StatusCode::BAD_REQUEST, "Ese torrent no existe."),
        Err(NoQuitado::NoEsta) => fallar(
            StatusCode::NOT_FOUND,
            "Ese torrent ya no está en qBittorrent.",
        ),
        Err(NoQuitado::Protegido(tarea::Proteccion {
            clasificacion: c,
            normas: n,
        })) => {
            use crate::ui::formato::{cifra, falta};
            let msg = match c.estado {
                Estado::Obligado => format!(
                    "No se puede quitar: está recién sembrado y {} para los {} días.",
                    falta(c.falta_s),
                    n.siembra_dias
                ),
                _ => format!(
                    "No se puede quitar: aún está creciendo hasta llegar a ratio {} o a los \
                     {} días sembrando ({}).",
                    cifra(n.ratio_min),
                    n.plazo_largo_dias,
                    falta(c.falta_s)
                ),
            };
            fallar(StatusCode::CONFLICT, &msg)
        }
        Err(NoQuitado::Qbt(e)) => {
            tracing::warn!(hash = %hash, "qBittorrent no quitó el torrent: {e}");
            fallar(
                StatusCode::BAD_GATEWAY,
                "qBittorrent no contestó bien. Mira si sigue en la lista antes de repetirlo.",
            )
        }
        Err(NoQuitado::Db(e)) => interno(e),
    }
}

pub(super) async fn no_borrar(hash: String, regla: TipoRegla) -> Result<(), ServerFnError> {
    let state = app_state()?;
    hash_bueno(&hash)?;
    tarea::no_borrar(&state.db, &hash, regla, ahora_unix())
        .await
        .or_else(interno)
}

pub(super) async fn quitar_exencion(hash: String, regla: TipoRegla) -> Result<(), ServerFnError> {
    let state = app_state()?;
    hash_bueno(&hash)?;
    tarea::quitar_exencion(&state.db, &hash, regla, ahora_unix())
        .await
        .or_else(interno)
}

// ---------------------------------------------------------------- reglas

pub(super) async fn pagina_reglas() -> Result<DatosReglas, ServerFnError> {
    // También aquí: la página las pide al pintarse en el servidor.
    sesion().await?;
    let state = app_state()?;
    let r = async {
        let db = &state.db;
        let guardadas = db::reglas(db).await?;
        let tarjetas = CATEGORIAS
            .iter()
            .map(|(c, _)| TarjetaReglas {
                categoria: (*c).to_owned(),
                reglas: guardadas.get(*c).copied().unwrap_or_default(),
            })
            .collect();
        let mut exenciones = Vec::new();
        let mut lista: Vec<(String, TipoRegla)> = db::exenciones(db).await?.into_iter().collect();
        lista.sort();
        for (hash, regla) in lista {
            exenciones.push(FichaExencion {
                nombre: db::ultimo_nombre(db, &hash).await?,
                hash,
                regla,
            });
        }
        let neutro = Preset::neutro();
        let mut presets = vec![FichaPreset {
            id: ID_INCLUIDO,
            nombre: neutro.nombre,
            descripcion: neutro.descripcion,
            incluido: true,
            trackers: neutro.trackers.len(),
        }];
        presets.extend(db::presets(db).await?.into_iter().map(|f| FichaPreset {
            id: f.id,
            nombre: f.nombre,
            descripcion: f.descripcion,
            incluido: false,
            trackers: f.trackers,
        }));
        let juego = db::juego_normas(db).await?;
        Ok::<_, sqlx::Error>(DatosReglas {
            modo_real: db::modo(db).await? == Modo::BorradoReal,
            trackers: fichas_trackers(&state, &juego).await?,
            normas: juego.defecto,
            tarjetas,
            exenciones,
            presets,
        })
    }
    .await;
    r.or_else(interno)
}

/// Los trackers de la página de reglas: los de los torrents de ahora (con
/// cuántos tiene cada uno) y los que tienen normas propias, por host. Si
/// qBittorrent no contesta, los últimos vistos.
async fn fichas_trackers(
    state: &AppState,
    juego: &reglas::JuegoNormas,
) -> Result<Vec<FichaTracker>, sqlx::Error> {
    let vistos = db::trackers_vistos(&state.db).await?;
    let mut cuantos: std::collections::BTreeMap<String, usize> = Default::default();
    match state.qbt.torrents().await {
        Ok(lista) => {
            for t in &lista {
                let host = tarea::host_de(t, &vistos);
                if !host.is_empty() {
                    *cuantos.entry(host.to_owned()).or_default() += 1;
                }
            }
        }
        Err(e) => {
            tracing::warn!("reglas sin torrents: {e}");
            for host in vistos.values() {
                *cuantos.entry(host.clone()).or_default() += 1;
            }
        }
    }
    for host in juego.trackers.keys() {
        cuantos.entry(host.clone()).or_default();
    }
    Ok(cuantos
        .into_iter()
        .map(|(host, torrents)| {
            let propias = juego.trackers.get(&host).copied();
            let usa = propias
                .is_none()
                .then(|| juego.tracker_de(&host).map(str::to_owned))
                .flatten();
            FichaTracker {
                host,
                torrents,
                propias,
                usa,
            }
        })
        .collect())
}

pub(super) async fn guardar_reglas(
    categoria: String,
    reglas: ReglasCategoria,
    clave: Option<String>,
) -> Result<(), ServerFnError> {
    let state = app_state()?;
    if !CATEGORIAS.iter().any(|(c, _)| *c == categoria) {
        return fallar(StatusCode::BAD_REQUEST, "Esa categoría no existe.");
    }
    if let Err(msg) = reglas.validar() {
        return fallar(StatusCode::BAD_REQUEST, msg);
    }
    exigir_contrasena_si_borrado_real(&state, clave).await?;
    db::guardar_reglas(&state.db, &categoria, &reglas)
        .await
        .or_else(interno)
}

pub(super) async fn guardar_normas(
    normas: NormasTracker,
    clave: Option<String>,
) -> Result<(), ServerFnError> {
    let state = app_state()?;
    if let Err(msg) = normas.validar() {
        return fallar(StatusCode::BAD_REQUEST, msg);
    }
    exigir_contrasena_si_borrado_real(&state, clave).await?;
    db::guardar_normas(&state.db, &normas)
        .await
        .or_else(interno)
}

/// El host de un tracker tal como llega de la interfaz: tiene que estar ya
/// en su forma de guardar.
fn host_bueno(tracker: &str) -> Result<(), ServerFnError> {
    if reglas::trackers::normalizar_host(tracker).as_deref() == Some(tracker) {
        Ok(())
    } else {
        fallar(StatusCode::BAD_REQUEST, "Ese tracker no vale.")
    }
}

pub(super) async fn guardar_normas_tracker(
    tracker: String,
    normas: NormasTracker,
    clave: Option<String>,
) -> Result<(), ServerFnError> {
    let state = app_state()?;
    host_bueno(&tracker)?;
    if let Err(msg) = normas.validar() {
        return fallar(StatusCode::BAD_REQUEST, msg);
    }
    let ya = db::normas_trackers(&state.db).await.or_else(interno)?;
    if !ya.contains_key(&tracker) && ya.len() >= preset::MAX_TRACKERS {
        return fallar(
            StatusCode::CONFLICT,
            &format!(
                "Ya hay {} trackers con normas propias: quita alguno antes.",
                preset::MAX_TRACKERS
            ),
        );
    }
    exigir_contrasena_si_borrado_real(&state, clave).await?;
    db::guardar_normas_tracker(&state.db, &tracker, &normas)
        .await
        .or_else(interno)
}

pub(super) async fn quitar_normas_tracker(
    tracker: String,
    clave: Option<String>,
) -> Result<(), ServerFnError> {
    let state = app_state()?;
    host_bueno(&tracker)?;
    exigir_contrasena_si_borrado_real(&state, clave).await?;
    db::quitar_normas_tracker(&state.db, &tracker)
        .await
        .or_else(interno)
        .map(|_| ())
}

pub(super) async fn cambiar_modo(real: bool, clave: Option<String>) -> Result<(), ServerFnError> {
    let state = app_state()?;
    let sesion = sesion().await?;
    let modo = if real {
        exigir_contrasena(&state, &sesion, clave).await?;
        Modo::BorradoReal
    } else {
        Modo::SoloAviso
    };
    tarea::cambiar_modo(&state.db, modo, ahora_unix())
        .await
        .or_else(interno)?;
    tracing::info!(real, "modo cambiado");
    Ok(())
}

// ---------------------------------------------------------------- presets

/// Las categorías del preset tienen que ser las de semillero.
fn categorias_conocidas(p: &Preset) -> Result<(), ServerFnError> {
    let conocida = |c: &String| CATEGORIAS.iter().any(|(k, _)| k == c);
    match p.categorias.keys().find(|c| !conocida(c)) {
        Some(c) => fallar(
            StatusCode::BAD_REQUEST,
            &format!(
                "El preset trae reglas para la categoría «{c}», que semillero no tiene: solo \
                 peliculas, series y {}.",
                preset::SIN_CATEGORIA
            ),
        ),
        None => Ok(()),
    }
}

fn nombre_repetido<T>() -> Result<T, ServerFnError> {
    fallar(
        StatusCode::CONFLICT,
        "Ya hay un preset con ese nombre. Elige otro.",
    )
}

/// ¿Es el nombre del preset incluido (sin mirar mayúsculas)?
fn es_del_incluido(nombre: &str) -> bool {
    nombre.to_lowercase() == Preset::neutro().nombre.to_lowercase()
}

fn es_repetido(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .is_some_and(|d| d.is_unique_violation())
}

/// Las normas (las de por defecto y las de cada tracker) y las reglas de
/// ahora como un preset (sin las categorías con todo apagado).
async fn configuracion_actual(
    db: &sqlx::SqlitePool,
    nombre: String,
    descripcion: String,
) -> Result<Preset, sqlx::Error> {
    let categorias = db::reglas(db)
        .await?
        .into_iter()
        .filter(|(c, r)| CATEGORIAS.iter().any(|(k, _)| k == c) && !r.apagadas())
        .collect();
    Ok(Preset {
        nombre,
        descripcion,
        normas: db::normas(db).await?,
        categorias,
        trackers: db::normas_trackers(db).await?,
    })
}

/// Nombre y descripción comprobados (400 si no valen; 409 si es el del
/// incluido).
fn nombre_y_descripcion(
    nombre: &str,
    descripcion: &str,
) -> Result<(String, String), ServerFnError> {
    let nombre = preset::validar_nombre(nombre).or_else(|m| fallar(StatusCode::BAD_REQUEST, m))?;
    let descripcion =
        preset::validar_descripcion(descripcion).or_else(|m| fallar(StatusCode::BAD_REQUEST, m))?;
    if es_del_incluido(&nombre) {
        return nombre_repetido();
    }
    Ok((nombre, descripcion))
}

async fn guardar_nuevo(state: &AppState, p: Preset) -> Result<(), ServerFnError> {
    if es_del_incluido(&p.nombre) {
        return nombre_repetido();
    }
    if db::cuantos_presets(&state.db).await.or_else(interno)? >= MAX_PRESETS {
        return fallar(
            StatusCode::CONFLICT,
            &format!("Ya hay {MAX_PRESETS} presets guardados: elimina alguno antes."),
        );
    }
    match db::crear_preset(&state.db, &p, ahora_unix()).await {
        Ok(_) => Ok(()),
        Err(e) if es_repetido(&e) => nombre_repetido(),
        Err(e) => interno(e),
    }
}

pub(super) async fn guardar_preset(
    nombre: String,
    descripcion: String,
) -> Result<(), ServerFnError> {
    let state = app_state()?;
    let (nombre, descripcion) = nombre_y_descripcion(&nombre, &descripcion)?;
    let p = configuracion_actual(&state.db, nombre, descripcion)
        .await
        .or_else(interno)?;
    guardar_nuevo(&state, p).await
}

pub(super) async fn subir_preset(archivo: String) -> Result<(), ServerFnError> {
    let state = app_state()?;
    // Antes de leer nada.
    if archivo.len() > preset::MAX_BYTES {
        return fallar(
            StatusCode::PAYLOAD_TOO_LARGE,
            "El archivo pasa de 64 KiB: un preset es mucho más pequeño.",
        );
    }
    let p = Preset::leer(&archivo).or_else(|m| fallar(StatusCode::BAD_REQUEST, &m))?;
    categorias_conocidas(&p)?;
    guardar_nuevo(&state, p).await
}

pub(super) async fn cambiar_preset(
    id: i64,
    nombre: String,
    descripcion: String,
    sobrescribir: bool,
) -> Result<(), ServerFnError> {
    let state = app_state()?;
    if id == ID_INCLUIDO {
        return fallar(
            StatusCode::CONFLICT,
            "El preset incluido no se puede cambiar. Guarda uno nuevo con «Guardar como preset».",
        );
    }
    let (nombre, descripcion) = nombre_y_descripcion(&nombre, &descripcion)?;
    let Some(antes) = db::preset(&state.db, id).await.or_else(interno)? else {
        return fallar(StatusCode::NOT_FOUND, "Ese preset ya no está.");
    };
    let nuevo = if sobrescribir {
        configuracion_actual(&state.db, nombre, descripcion)
            .await
            .or_else(interno)?
    } else {
        Preset {
            nombre,
            descripcion,
            ..antes
        }
    };
    match db::cambiar_preset(&state.db, id, &nuevo, ahora_unix()).await {
        Ok(true) => Ok(()),
        Ok(false) => fallar(StatusCode::NOT_FOUND, "Ese preset ya no está."),
        Err(e) if es_repetido(&e) => nombre_repetido(),
        Err(e) => interno(e),
    }
}

pub(super) async fn aplicar_preset(id: i64, clave: Option<String>) -> Result<(), ServerFnError> {
    let state = app_state()?;
    // Con el borrado real, un preset puede adelantar borrados.
    exigir_contrasena_si_borrado_real(&state, clave).await?;
    let p = if id == ID_INCLUIDO {
        Preset::neutro()
    } else {
        match db::preset(&state.db, id).await.or_else(interno)? {
            Some(p) => p,
            None => return fallar(StatusCode::NOT_FOUND, "Ese preset ya no está."),
        }
    };
    categorias_conocidas(&p)?;
    db::aplicar_preset(&state.db, &p, ahora_unix())
        .await
        .or_else(interno)?;
    tracing::info!(id, "preset aplicado");
    Ok(())
}

pub(super) async fn eliminar_preset(id: i64) -> Result<(), ServerFnError> {
    let state = app_state()?;
    if id == ID_INCLUIDO {
        return fallar(
            StatusCode::CONFLICT,
            "El preset incluido no se puede eliminar.",
        );
    }
    match db::borrar_preset(&state.db, id).await {
        Ok(true) => Ok(()),
        Ok(false) => fallar(StatusCode::NOT_FOUND, "Ese preset ya no está."),
        Err(e) => interno(e),
    }
}

/// Un trozo seguro para el nombre del archivo: minúsculas, cifras y
/// guiones.
fn nombre_de_archivo(nombre: &str) -> String {
    let mut s = String::new();
    for c in nombre.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            s.push(c);
        } else if !s.ends_with('-') {
            s.push('-');
        }
    }
    let s: String = s.trim_matches('-').chars().take(40).collect();
    let s = s.trim_end_matches('-');
    if s.is_empty() {
        "preset".to_owned()
    } else {
        s.to_owned()
    }
}

#[derive(serde::Deserialize)]
pub struct Descarga {
    id: i64,
}

/// `GET` [`RUTA_DESCARGA`]`?id=N`: el archivo del preset para guardarlo (el
/// guardián de `/api` ya exigió la sesión).
pub async fn descargar_preset(
    State(state): State<AppState>,
    Query(q): Query<Descarga>,
) -> Response {
    let p = if q.id == ID_INCLUIDO {
        Some(Preset::neutro())
    } else {
        match db::preset(&state.db, q.id).await {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("error interno al descargar un preset: {e}");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    };
    let Some(p) = p else {
        return (StatusCode::NOT_FOUND, "Ese preset no existe.").into_response();
    };
    let archivo = format!(
        "attachment; filename=\"semillero-preset-{}.toml\"",
        nombre_de_archivo(&p.nombre)
    );
    (
        [
            (
                header::CONTENT_TYPE,
                "application/toml; charset=utf-8".to_owned(),
            ),
            (header::CONTENT_DISPOSITION, archivo),
        ],
        p.escribir(),
    )
        .into_response()
}

// ---------------------------------------------------------------- historial

fn suceso(e: Evento) -> Suceso {
    match e {
        Evento::Aviso => Suceso::Aviso,
        Evento::Cancelado => Suceso::Cancelado,
        Evento::Retirado => Suceso::Retirado,
        Evento::SeHabriaBorrado => Suceso::SeHabriaBorrado,
        Evento::Borrado => Suceso::Borrado,
        Evento::Quitado => Suceso::Quitado,
        Evento::Pausado => Suceso::Pausado,
        Evento::ExencionQuitada => Suceso::ExencionQuitada,
        Evento::ModoCambiado => Suceso::ModoCambiado,
        Evento::PresetAplicado => Suceso::PresetAplicado,
        Evento::Anadido => Suceso::Anadido,
    }
}

pub(super) async fn historial() -> Result<DatosHistorial, ServerFnError> {
    // También aquí: la página las pide al pintarse en el servidor.
    sesion().await?;
    let state = app_state()?;
    let lineas = db::historial(&state.db, LINEAS_HISTORIAL)
        .await
        .or_else(interno)?
        .into_iter()
        .map(|a| LineaHistorial {
            ts: a.ts,
            general: a.hash.is_empty(),
            nombre: a.nombre,
            suceso: suceso(a.evento),
            regla: a.regla,
            detalle: a.detalle,
        })
        .collect();
    Ok(DatosHistorial {
        ahora: ahora_unix(),
        lineas,
    })
}

// ---------------------------------------------------------------- añadir

pub(super) async fn pagina_anadir() -> Result<DatosAnadir, ServerFnError> {
    // También aquí: la página las pide al pintarse en el servidor.
    sesion().await?;
    let state = app_state()?;
    match state.qbt.categorias().await {
        Ok(categorias) => Ok(DatosAnadir { categorias }),
        Err(e) => {
            tracing::warn!("sin las categorías de qBittorrent: {e}");
            fallar(
                StatusCode::BAD_GATEWAY,
                "qBittorrent no contesta. Prueba de nuevo en un momento.",
            )
        }
    }
}

/// El nombre de un archivo subido, para los mensajes: sin carpetas.
fn nombre_subido(nombre: &str) -> String {
    let base = nombre.rsplit(['/', '\\']).next().unwrap_or_default();
    let n = super::anadir::limpio(base);
    if n.is_empty() {
        "(sin nombre)".to_owned()
    } else {
        n
    }
}

fn demasiado_grande<T>(nombre: &str) -> Result<T, ServerFnError> {
    fallar(
        StatusCode::PAYLOAD_TOO_LARGE,
        &format!(
            "El archivo «{nombre}» pasa de {} MiB: no se ha añadido nada.",
            MAX_BYTES_TORRENT / (1024 * 1024)
        ),
    )
}

pub(super) async fn anadir_torrents(
    archivos: Vec<ArchivoSubido>,
    magnets: String,
    categoria: String,
) -> Result<ResultadoAnadir, ServerFnError> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;

    let state = app_state()?;
    // Primero, cuánto: antes de leer nada.
    if archivos.len() > MAX_ARCHIVOS {
        return fallar(
            StatusCode::BAD_REQUEST,
            &format!("Como mucho {MAX_ARCHIVOS} archivos .torrent de una vez."),
        );
    }
    if magnets.len() > MAX_MAGNETS * MAX_BYTES_MAGNET {
        return fallar(
            StatusCode::PAYLOAD_TOO_LARGE,
            "Hay demasiado texto en los enlaces magnet.",
        );
    }
    let lineas: Vec<(usize, &str)> = magnets
        .lines()
        .enumerate()
        .map(|(i, l)| (i + 1, l.trim()))
        .filter(|(_, l)| !l.is_empty())
        .collect();
    if lineas.len() > MAX_MAGNETS {
        return fallar(
            StatusCode::BAD_REQUEST,
            &format!("Como mucho {MAX_MAGNETS} enlaces magnet de una vez."),
        );
    }
    if archivos.is_empty() && lineas.is_empty() {
        return fallar(
            StatusCode::BAD_REQUEST,
            "Elige un archivo .torrent o pega un enlace magnet.",
        );
    }

    // Después, si vale cada uno. Si alguno no, no se añade ninguno.
    let mut problemas = Vec::new();
    let mut a_qbt = Vec::with_capacity(archivos.len());
    let mut apuntes = Vec::new();
    for a in &archivos {
        let nombre = nombre_subido(&a.nombre);
        if a.datos.len() > largo_base64(MAX_BYTES_TORRENT) {
            return demasiado_grande(&nombre);
        }
        let Ok(datos) = STANDARD.decode(a.datos.trim()) else {
            problemas.push(format!("El archivo «{nombre}» no se ha podido leer."));
            continue;
        };
        if datos.len() > MAX_BYTES_TORRENT {
            return demasiado_grande(&nombre);
        }
        match super::anadir::torrent(&datos) {
            Some(t) => {
                apuntes.push((t.hash, t.nombre.unwrap_or_else(|| nombre.clone())));
                a_qbt.push(qbt::ArchivoTorrent {
                    nombre: nombre.clone(),
                    datos,
                });
            }
            None => problemas.push(format!("El archivo «{nombre}» no es un archivo .torrent.")),
        }
    }
    let mut enlaces = Vec::with_capacity(lineas.len());
    let mut nombres_magnet = Vec::with_capacity(lineas.len());
    for (n, linea) in lineas {
        match super::anadir::magnet(linea, n) {
            Ok(m) => {
                apuntes.push((m.hash, m.nombre.clone()));
                nombres_magnet.push(m.nombre);
                enlaces.push(m.enlace);
            }
            Err(p) => problemas.push(p),
        }
    }
    if !problemas.is_empty() {
        return fallar(
            StatusCode::BAD_REQUEST,
            &format!("No se ha añadido nada. {}", problemas.join(" ")),
        );
    }

    let categoria = categoria.trim();
    if !categoria.is_empty() {
        match state.qbt.categorias().await {
            Ok(cs) if cs.iter().any(|c| c == categoria) => {}
            Ok(_) => {
                return fallar(
                    StatusCode::BAD_REQUEST,
                    &format!(
                        "La categoría «{}» no está en qBittorrent: no se ha añadido nada.",
                        super::anadir::limpio(categoria)
                    ),
                );
            }
            Err(e) => {
                tracing::warn!("sin las categorías de qBittorrent: {e}");
                return fallar(
                    StatusCode::BAD_GATEWAY,
                    "qBittorrent no contesta: no se ha añadido nada. Prueba de nuevo en un momento.",
                );
            }
        }
    }

    let categoria = (!categoria.is_empty()).then_some(categoria);
    match state.qbt.anadir(&a_qbt, &enlaces, categoria).await {
        Ok(()) => {}
        Err(qbt::Error::NoAnadido) => {
            return fallar(
                StatusCode::BAD_GATEWAY,
                "qBittorrent no lo ha añadido: puede que ya lo tuviera o que algún archivo esté \
                 dañado. Mira la lista antes de repetirlo.",
            );
        }
        Err(e) => {
            tracing::warn!("qBittorrent no añadió los torrents: {e}");
            return fallar(
                StatusCode::BAD_GATEWAY,
                "qBittorrent no contestó bien. Mira si están en la lista antes de repetirlo.",
            );
        }
    }
    tracing::info!(
        archivos = a_qbt.len(),
        magnets = enlaces.len(),
        "torrents añadidos a mano"
    );

    // Ya están en qBittorrent: un fallo al apuntarlos no lo deshace.
    let detalle = match categoria {
        Some(c) => format!("Añadido desde semillero con la categoría «{c}»"),
        None => "Añadido desde semillero, sin categoría".to_owned(),
    };
    let ahora = ahora_unix();
    for (hash, nombre) in apuntes {
        let apunte = db::Apunte {
            ts: ahora,
            hash,
            nombre,
            evento: Evento::Anadido,
            regla: None,
            detalle: detalle.clone(),
        };
        if let Err(e) = db::apuntar(&state.db, &apunte).await {
            tracing::error!(hash = %apunte.hash, "añadido SIN APUNTAR en el historial: {e}");
        }
    }
    let mut anadidos: Vec<String> = a_qbt.into_iter().map(|a| a.nombre).collect();
    anadidos.extend(nombres_magnet);
    Ok(ResultadoAnadir { anadidos })
}
