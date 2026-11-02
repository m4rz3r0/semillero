//! Aviso mínimo a Jellyfin tras borrar un torrent con sus ficheros.
//!
//! No se borra ningún elemento por su id: se pide reescanear la biblioteca
//! (`POST /Library/Refresh`) y Jellyfin quita por sí mismo lo que ya no está
//! en el disco. Así no puede tocar otro fichero por un id equivocado, y pedirlo
//! dos veces no hace daño.
//!
//! La clave de la API nunca aparece en los errores.

use std::time::Duration;

use reqwest::{Client, StatusCode};

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("Jellyfin no responde")]
    NoResponde,
    #[error("Jellyfin no acepta la clave de la API")]
    ClaveRechazada,
    #[error("Jellyfin contestó con un error ({0})")]
    Rechazado(u16),
}

const PLAZO: Duration = Duration::from_secs(10);

pub struct Jellyfin {
    base: String,
    clave: String,
    cliente: Client,
}

impl Jellyfin {
    /// `base` es la dirección de Jellyfin (`http://IP:8096`), sin la barra final.
    pub fn new(base: &str, clave: &str) -> Self {
        Self {
            base: base.trim_end_matches('/').to_owned(),
            clave: clave.trim().to_owned(),
            cliente: Client::builder().timeout(PLAZO).build().unwrap_or_default(),
        }
    }

    /// Pide a Jellyfin que reescanee la biblioteca.
    pub async fn reescanear(&self) -> Result<(), Error> {
        self.pedir(self.cliente.post(format!("{}/Library/Refresh", self.base)))
            .await
    }

    /// Comprueba que Jellyfin contesta y acepta la clave, con una lectura
    /// que no cambia nada (`GET /System/Info`).
    pub async fn comprobar(&self) -> Result<(), Error> {
        self.pedir(self.cliente.get(format!("{}/System/Info", self.base)))
            .await
    }

    async fn pedir(&self, peticion: reqwest::RequestBuilder) -> Result<(), Error> {
        let r = peticion
            .header(
                "Authorization",
                format!("MediaBrowser Token=\"{}\"", self.clave),
            )
            .send()
            .await
            .map_err(|_| Error::NoResponde)?;
        match r.status() {
            s if s.is_success() => Ok(()),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(Error::ClaveRechazada),
            s => Err(Error::Rechazado(s.as_u16())),
        }
    }
}
