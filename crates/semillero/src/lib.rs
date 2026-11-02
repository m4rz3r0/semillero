//! semillero: web para cuidar los torrents de qBittorrent sin arriesgar un
//! H&R. Leptos (SSR + hidratación) sobre Axum.
//!
//! Con la feature `ssr` se compila el servidor; con `hydrate`, la parte que se
//! ejecuta en el navegador (WASM).

pub mod api;
pub mod app;
pub mod auth;
pub mod cliente;
pub mod ui;

#[cfg(feature = "ssr")]
pub mod config;
#[cfg(feature = "ssr")]
pub mod configurar;
#[cfg(feature = "ssr")]
pub mod db;
#[cfg(feature = "ssr")]
pub mod disco;
#[cfg(feature = "ssr")]
pub mod estado;
#[cfg(feature = "ssr")]
pub mod estado_red;
#[cfg(feature = "ssr")]
pub mod mediateca;
#[cfg(feature = "ssr")]
pub mod seguridad;
#[cfg(feature = "ssr")]
pub mod tarea;

#[cfg(feature = "ssr")]
pub use servidor::construir_router;

#[cfg(feature = "ssr")]
mod servidor {
    use std::net::SocketAddr;
    use std::sync::OnceLock;

    use axum::extract::{ConnectInfo, State};
    use axum::http::HeaderMap;
    use axum::routing::get;
    use axum::{Json, Router, middleware};
    use leptos_axum::{AxumRouteListing, LeptosRoutes, generate_route_list};
    use serde_json::{Value, json};

    use crate::app::{App, shell};
    use crate::estado::AppState;
    use crate::seguridad;

    /// Rutas de la aplicación, generadas una sola vez por proceso (como en el
    /// panel: generarlas con el servidor ya sirviendo puede dejar colgada
    /// una página de otro router del mismo proceso, en las pruebas).
    fn rutas() -> Vec<AxumRouteListing> {
        static RUTAS: OnceLock<Vec<AxumRouteListing>> = OnceLock::new();
        RUTAS.get_or_init(|| generate_route_list(App)).clone()
    }

    /// Router Axum completo: páginas de Leptos (con sesión salvo `/login`),
    /// server functions en `/api/*` y la descarga de presets (tras el
    /// guardián), `/salud` y los
    /// ficheros estáticos; límite de tamaño y cabeceras de seguridad en todo.
    pub fn construir_router(state: AppState) -> Router {
        Router::new()
            .leptos_routes(&state, rutas(), {
                let opciones = state.leptos.clone();
                move || shell(opciones.clone())
            })
            .route("/salud", get(salud))
            .route(crate::api::RUTA_DESCARGA, get(crate::api::descargar_preset))
            .fallback(leptos_axum::file_and_error_handler::<AppState, _>(shell))
            // En todo, también en el `fallback`: el router de Leptos reconoce
            // rutas que el de axum no (`/./`, `/reglas/`) y las pintaría.
            .layer(middleware::from_fn_with_state(
                state.clone(),
                seguridad::paginas,
            ))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                seguridad::guardian,
            ))
            .layer(middleware::from_fn(seguridad::limitar_cuerpo))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                seguridad::cabeceras,
            ))
            .with_state(state)
    }

    /// Comprobación de vida (sin login ni preguntar a qBittorrent). La
    /// versión, que necesita el actualizador, solo a las peticiones del
    /// propio equipo ([`seguridad::peticion_local`]); a las demás, solo que
    /// está vivo.
    async fn salud(
        State(state): State<AppState>,
        ConnectInfo(peer): ConnectInfo<SocketAddr>,
        cabeceras: HeaderMap,
    ) -> Json<Value> {
        if seguridad::peticion_local(peer.ip(), &cabeceras, &state.cfg) {
            Json(json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") }))
        } else {
            Json(json!({ "ok": true }))
        }
    }
}

/// Punto de entrada del WASM: hidrata el HTML que generó el servidor.
#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(app::App);
}
