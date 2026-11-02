//! Estado compartido del servidor.

use std::sync::Arc;

use axum::extract::FromRef;
use leptos::prelude::LeptosOptions;
use qbt::{Cuenta, Qbt};
use sqlx::SqlitePool;
use tokio::sync::Mutex;

use crate::auth::Canario;
use crate::config::Config;
use crate::mediateca::Compartida;

/// Estado del servidor, compartido por las rutas Axum y las server functions.
/// Barato de clonar.
#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub db: SqlitePool,
    pub qbt: Arc<Qbt>,
    pub leptos: LeptosOptions,
    /// Turno de las comprobaciones de contraseña (login y reautenticación),
    /// con el resultado del canario de arranque: el freno se comprueba y se
    /// actualiza sin carreras, y el canario no se repite a la vez.
    pub acceso: Arc<Mutex<Canario>>,
    /// Mediateca a la que se pide olvidar lo borrado con sus ficheros, en la
    /// tarea horaria y en los borrados a mano. Sin Jellyfin, no hace nada.
    pub mediateca: Compartida,
}

impl AppState {
    /// El error, en palabras para quien arranca el servidor (nunca lleva la
    /// contraseña de qBittorrent).
    pub async fn nuevo(cfg: Config, leptos: LeptosOptions) -> Result<Self, String> {
        let qbt = match (&cfg.cuenta_qbt, cfg.login_desde_efectivo()) {
            (None, Some(desde)) => Qbt::new(&cfg.qbt, desde),
            (None, None) => unreachable!("en modo lista blanca siempre hay IP de origen"),
            (Some(cuenta), desde) => {
                let clave = crate::config::leer_contrasena_qbt(cuenta)?;
                #[cfg(unix)]
                if crate::config::legible_por_otros(&cuenta.clave) {
                    tracing::warn!(
                        "otros usuarios pueden leer el fichero de la contraseña de qBittorrent \
                         ({}); déjalo solo para el dueño (chmod 600)",
                        cuenta.clave.display()
                    );
                }
                Qbt::con_cuenta(&cfg.qbt, desde, Cuenta::new(&cuenta.usuario, clave))
            }
        };
        let db = crate::db::abrir(&cfg.db)
            .await
            .map_err(|e| format!("no se puede abrir la base de datos: {e}"))?;
        Ok(Self {
            cfg: Arc::new(cfg),
            db,
            qbt: Arc::new(qbt),
            leptos,
            acceso: Arc::new(Mutex::new(Canario::Pendiente)),
            mediateca: Compartida::default(),
        })
    }
}

impl FromRef<AppState> for LeptosOptions {
    fn from_ref(s: &AppState) -> Self {
        s.leptos.clone()
    }
}
