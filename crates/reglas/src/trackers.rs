//! Normas por tracker: el juego por defecto y las de cada tracker, elegidas
//! por el host del anuncio de cada torrent.
//!
//! Un host configurado vale para él y para sus subdominios
//! (`ejemplo.example` vale para `tracker.ejemplo.example`, no para
//! `malejemplo.example`); si encajan varios, el más concreto. Sin host o sin
//! ninguno que encaje, las normas por defecto.

use std::collections::BTreeMap;
use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use crate::NormasTracker;

/// Caracteres como mucho en un host (los de un nombre DNS).
pub const MAX_HOST: usize = 253;

/// Un host de tracker en su forma de guardar: minúsculas ASCII, sin punto
/// final; una IP, como la escribe la librería estándar. `None` si no es un
/// nombre DNS ni una IP (con esquema, usuario, puerto, ruta, espacios…).
pub fn normalizar_host(host: &str) -> Option<String> {
    let h = host.trim();
    let h = h.strip_suffix('.').unwrap_or(h);
    if let Some(ip) = h.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
        return ip
            .parse::<std::net::Ipv6Addr>()
            .ok()
            .map(|ip| ip.to_string());
    }
    if h.is_empty() || h.len() > MAX_HOST || !h.is_ascii() {
        return None;
    }
    if let Ok(ip) = h.parse::<IpAddr>() {
        return Some(ip.to_string());
    }
    let h = h.to_ascii_lowercase();
    let etiqueta_buena = |e: &str| {
        (1..=63).contains(&e.len())
            && !e.starts_with('-')
            && !e.ends_with('-')
            && e.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    };
    h.split('.').all(etiqueta_buena).then_some(h)
}

/// ¿Vale el host configurado `configurado` para el torrent con host
/// `host`? Iguales, o `host` es un subdominio de `configurado` (separado
/// por un punto). Una IP solo vale para ella misma.
pub fn encaja(host: &str, configurado: &str) -> bool {
    if host.is_empty() || configurado.is_empty() {
        return false;
    }
    if host == configurado {
        return true;
    }
    if host.parse::<IpAddr>().is_ok() || configurado.parse::<IpAddr>().is_ok() {
        return false;
    }
    host.strip_suffix(configurado)
        .is_some_and(|antes| antes.ends_with('.'))
}

/// Las normas por defecto y las de cada tracker (por host).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct JuegoNormas {
    pub defecto: NormasTracker,
    pub trackers: BTreeMap<String, NormasTracker>,
}

impl JuegoNormas {
    /// Solo las normas por defecto.
    pub fn solo(defecto: NormasTracker) -> Self {
        Self {
            defecto,
            trackers: BTreeMap::new(),
        }
    }

    /// El host configurado cuyas normas valen para `host`: el más concreto
    /// de los que encajan. `None`: valen las de por defecto.
    pub fn tracker_de(&self, host: &str) -> Option<&str> {
        self.trackers
            .keys()
            .filter(|c| encaja(host, c))
            .max_by_key(|c| c.len())
            .map(String::as_str)
    }

    /// Las normas de un torrent con host `host` (vacío: no se sabe).
    pub fn para(&self, host: &str) -> &NormasTracker {
        self.tracker_de(host)
            .and_then(|c| self.trackers.get(c))
            .unwrap_or(&self.defecto)
    }
}
