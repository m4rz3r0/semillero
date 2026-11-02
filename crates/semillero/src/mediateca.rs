//! Enganche con la mediateca (Jellyfin): tras borrar un torrent con sus
//! ficheros, se le pide que lo olvide. Sin `SEMILLERO_JELLYFIN` no hace nada
//! ([`SinMediateca`]).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use jellyfin::Jellyfin;
use qbt::TorrentQbt;

use crate::config::ConfJellyfin;

pub trait Mediateca: Send + Sync {
    /// Olvida lo que la mediateca sepa del torrent ya borrado. Un error se
    /// apunta en el historial, pero no deshace el borrado.
    fn olvidar(&self, t: &TorrentQbt) -> impl Future<Output = Result<(), String>> + Send;
}

/// Sin mediateca: no hace nada.
#[derive(Clone, Copy, Debug, Default)]
pub struct SinMediateca;

impl Mediateca for SinMediateca {
    async fn olvidar(&self, _t: &TorrentQbt) -> Result<(), String> {
        Ok(())
    }
}

/// Jellyfin quita por sí mismo lo que ya no está en el disco al reescanear
/// (ver el crate `jellyfin`); pedirlo otra vez no hace daño.
impl Mediateca for Jellyfin {
    async fn olvidar(&self, _t: &TorrentQbt) -> Result<(), String> {
        self.reescanear().await.map_err(|e| e.to_string())
    }
}

/// La mediateca según la configuración: Jellyfin con la clave leída de su
/// fichero, o ninguna. Si el fichero no existe y la ruta de la 0.2 sí, se
/// usa esa. El error nunca lleva la clave.
pub fn desde_config(j: Option<&ConfJellyfin>) -> Result<Compartida, String> {
    let Some(j) = j else {
        return Ok(Compartida::default());
    };
    let ruta = match &j.antigua {
        Some(antigua) if !j.clave.exists() && antigua.exists() => {
            tracing::warn!(
                "la clave de Jellyfin se lee de {}, donde estaba en la 0.2; muévela a {} o pon SEMILLERO_JELLYFIN_CLAVE",
                antigua.display(),
                j.clave.display()
            );
            antigua
        }
        _ => &j.clave,
    };
    let clave = crate::config::leer_secreto(ruta, "la clave de Jellyfin")?;
    Ok(Compartida(Arc::new(Jellyfin::new(&j.url, &clave))))
}

/// Futuro de [`MediatecaEnCaja::olvidar_en_caja`].
pub type Olvido<'a> = Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

/// [`Mediateca`] que cabe en un `dyn` (para guardarla en el estado).
pub trait MediatecaEnCaja: Send + Sync {
    fn olvidar_en_caja<'a>(&'a self, t: &'a TorrentQbt) -> Olvido<'a>;
}

impl<M: Mediateca> MediatecaEnCaja for M {
    fn olvidar_en_caja<'a>(&'a self, t: &'a TorrentQbt) -> Olvido<'a> {
        Box::pin(self.olvidar(t))
    }
}

/// La mediateca del servidor, compartida por la tarea horaria y los borrados
/// a mano (ver [`crate::estado::AppState::mediateca`]).
#[derive(Clone)]
pub struct Compartida(pub Arc<dyn MediatecaEnCaja>);

impl Default for Compartida {
    fn default() -> Self {
        Self(Arc::new(SinMediateca))
    }
}

impl Mediateca for Compartida {
    async fn olvidar(&self, t: &TorrentQbt) -> Result<(), String> {
        self.0.olvidar_en_caja(t).await
    }
}
