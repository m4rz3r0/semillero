//! Estado de la VPN y del puerto reenviado, leído de un fichero de estado
//! (`SEMILLERO_ESTADO`, líneas `clave=valor`) que escribe el programa que
//! mantiene el túnel y abre el puerto:
//!
//! ```text
//! escrito_en=<segundos Unix de esta escritura>
//! handshake=<segundos Unix del último saludo del túnel>
//! puerto=<puerto reenviado, o vacío>
//! fallos=<renovaciones seguidas del puerto que han fallado>
//! ```
//!
//! - El fichero tiene que haberse escrito hace como mucho 120 s (se espera
//!   que se reescriba cada 45 s o menos); si no, ese programa no corre y no
//!   se sabe nada: VPN caída y puerto sin abrir.
//! - La VPN está arriba si el último saludo tiene como mucho 300 s.
//! - El puerto está abierto salvo que lleve 3 fallos seguidos, y eso es
//!   independiente de la VPN.
//!
//! Sin `SEMILLERO_ESTADO` no se vigila nada: la red cuenta como bien y las
//! cuentas atrás avanzan (ver [`EstadoRed::SIN_VIGILAR`]).

use std::io::Read;
use std::path::Path;

const ESCRITO_MAX_S: f64 = 120.0;
const SALUDO_MAX_S: f64 = 300.0;
const FALLOS_MAX: i64 = 3;
/// El fichero tiene cuatro líneas cortas; de uno más grande no se lee nada.
pub const MAX_BYTES: u64 = 4 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EstadoRed {
    /// Hay fichero de estado configurado. Si no, no se vigila la red.
    pub vigilada: bool,
    pub vpn: bool,
    /// Puerto reenviado, si está abierto.
    pub puerto: Option<u16>,
}

/// Vigilada y sin saber nada: VPN caída y puerto sin abrir.
impl Default for EstadoRed {
    fn default() -> Self {
        Self {
            vigilada: true,
            vpn: false,
            puerto: None,
        }
    }
}

impl EstadoRed {
    /// Sin fichero de estado: no se vigila y la red cuenta como bien.
    pub const SIN_VIGILAR: EstadoRed = EstadoRed {
        vigilada: false,
        vpn: false,
        puerto: None,
    };

    /// VPN arriba y puerto abierto (o red sin vigilar): solo entonces
    /// avanzan los avisos.
    pub fn red_ok(&self) -> bool {
        !self.vigilada || (self.vpn && self.puerto.is_some())
    }
}

/// Interpreta el contenido del fichero a la hora `ahora` (segundos Unix).
pub fn interpretar(texto: &str, ahora: f64) -> EstadoRed {
    let campo = |k: &str| {
        texto
            .lines()
            .filter_map(|l| l.trim().split_once('='))
            .filter(|(c, _)| c.trim() == k)
            .map(|(_, v)| v.trim())
            .next_back()
    };
    let num = |k: &str| -> Option<f64> {
        match campo(k) {
            None => Some(0.0),
            Some(v) => v.parse().ok(),
        }
    };
    let (Some(escrito_en), Some(saludo)) = (num("escrito_en"), num("handshake")) else {
        return EstadoRed::default();
    };
    let fallos: i64 = match campo("fallos") {
        None => 0,
        Some(v) => match v.parse() {
            Ok(f) => f,
            Err(_) => return EstadoRed::default(),
        },
    };
    let puerto: Option<u16> = match campo("puerto") {
        None | Some("") => None,
        Some(v) => match v.parse() {
            Ok(p) => Some(p),
            Err(_) => return EstadoRed::default(),
        },
    };
    if ahora - escrito_en > ESCRITO_MAX_S {
        return EstadoRed::default();
    }
    EstadoRed {
        vigilada: true,
        vpn: ahora - saludo <= SALUDO_MAX_S,
        puerto: if fallos >= FALLOS_MAX { None } else { puerto },
    }
}

/// Lee el fichero `ruta` (como mucho [`MAX_BYTES`]); si no se puede leer o
/// es más grande, todo caído. Sin `ruta`, [`EstadoRed::SIN_VIGILAR`].
pub fn leer(ruta: Option<&Path>, ahora: f64) -> EstadoRed {
    let Some(ruta) = ruta else {
        return EstadoRed::SIN_VIGILAR;
    };
    let mut texto = String::new();
    let leido =
        std::fs::File::open(ruta).and_then(|f| f.take(MAX_BYTES + 1).read_to_string(&mut texto));
    match leido {
        Ok(n) if n as u64 <= MAX_BYTES => interpretar(&texto, ahora),
        _ => EstadoRed::default(),
    }
}
