//! Reglas de semillero, como funciones puras (sin E/S).
//!
//! Las normas del tracker ([`NormasTracker`]) dicen qué torrents obligan a
//! sembrar y cuánto: por defecto, quien descarga al menos un 10 % de un
//! torrent tiene que sembrarlo 7 días y el ratio tiene que llegar a 1.0 (o
//! sembrarse 30 días). Las reglas de cada categoría ([`ReglasCategoria`])
//! deciden qué se recoge de lo que ya cumple. Los presets ([`preset`]) las
//! guardan juntas en un archivo que se puede compartir. Cada tracker puede
//! tener sus propias normas ([`trackers`]); las funciones `_con` reciben las
//! de cada torrent.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

pub mod preset;
pub mod trackers;

pub use trackers::JuegoNormas;

const DIA_S: i64 = 86_400;
const HORA_S: i64 = 3600;

/// Las normas del tracker: cuándo un torrent obliga a sembrar, cuánto, y
/// cuándo se para solo. Los valores por defecto son los de un tracker
/// privado habitual.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct NormasTracker {
    /// Porcentaje descargado (0–100) a partir del cual el torrent obliga.
    pub umbral_pct: f64,
    /// Días que hay que sembrar un torrent que obliga.
    pub siembra_dias: u32,
    /// Ratio con la que, pasados los días de siembra, ya cumple.
    pub ratio_min: f64,
    /// Días de siembra con los que cumple aunque no llegue a la ratio.
    pub plazo_largo_dias: u32,
    /// Ratio a la que se para solo (con los días de siembra cumplidos). Por
    /// encima de la mínima: si parase en ella, ningún torrent subiría más de
    /// lo que bajó y el ratio global nunca pasaría de ahí.
    pub ratio_pausa: f64,
    /// Horas entre el aviso de una regla y la recogida.
    pub aviso_horas: u32,
}

impl NormasTracker {
    /// Los valores por defecto.
    pub const POR_DEFECTO: NormasTracker = NormasTracker {
        umbral_pct: 10.0,
        siembra_dias: 7,
        ratio_min: 1.0,
        plazo_largo_dias: 30,
        ratio_pausa: 2.0,
        aviso_horas: 24,
    };
    pub const MAX_DIAS: u32 = 365;
    pub const MAX_RATIO: f64 = 100.0;
    pub const MAX_AVISO_HORAS: u32 = 720;

    pub const fn siembra_s(&self) -> i64 {
        self.siembra_dias as i64 * DIA_S
    }

    pub const fn plazo_largo_s(&self) -> i64 {
        self.plazo_largo_dias as i64 * DIA_S
    }

    pub const fn aviso_s(&self) -> i64 {
        self.aviso_horas as i64 * HORA_S
    }

    /// Comprueba los valores; el mensaje dice qué falla.
    pub fn validar(&self) -> Result<(), &'static str> {
        let entre = |v: f64, max: f64| v.is_finite() && (0.0..=max).contains(&v);
        if !entre(self.umbral_pct, 100.0) {
            return Err("El porcentaje descargado que obliga tiene que estar entre 0 y 100.");
        }
        if self.siembra_dias > Self::MAX_DIAS {
            return Err("Los días de siembra obligatoria tienen que estar entre 0 y 365.");
        }
        if !entre(self.ratio_min, Self::MAX_RATIO) {
            return Err("La ratio mínima tiene que estar entre 0 y 100.");
        }
        if self.plazo_largo_dias > Self::MAX_DIAS {
            return Err("El plazo máximo tiene que estar entre 0 y 365 días.");
        }
        if self.plazo_largo_dias < self.siembra_dias {
            return Err("El plazo máximo no puede ser menor que los días de siembra obligatoria.");
        }
        if !entre(self.ratio_pausa, Self::MAX_RATIO) {
            return Err("La ratio para parar tiene que estar entre 0 y 100.");
        }
        if self.ratio_pausa < self.ratio_min {
            return Err("La ratio para parar no puede ser menor que la ratio mínima.");
        }
        if !(1..=Self::MAX_AVISO_HORAS).contains(&self.aviso_horas) {
            return Err("Las horas de aviso antes de recoger tienen que estar entre 1 y 720.");
        }
        Ok(())
    }
}

impl Default for NormasTracker {
    fn default() -> Self {
        Self::POR_DEFECTO
    }
}

const PARADOS: [&str; 2] = ["stoppedUP", "stoppedDL"];

/// Lo que semillero necesita de un torrent de qBittorrent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Torrent {
    pub hash: String,
    /// Vacía: sin categoría.
    pub categoria: String,
    /// `state` de qBittorrent.
    pub estado_qbt: String,
    pub descargado: i64,
    pub tamano: i64,
    pub siembra_s: i64,
    pub ratio: f64,
    pub anadido_en: i64,
    /// Última vez que semillero vio crecer lo subido; `None` sin muestras aún.
    pub ultima_subida_en: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Estado {
    /// Descargado por debajo del umbral: no obliga a nada.
    Libre,
    /// Aún no suma los días de siembra.
    Obligado,
    /// Con los días de siembra, sin la ratio mínima y antes del plazo
    /// máximo: sigue sembrando.
    EnEspera,
    /// Días de siembra y ratio mínima, o el plazo máximo sembrando.
    Cumplido,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clasificacion {
    pub estado: Estado,
    /// Segundos de siembra que faltan para salir de Obligado o En espera.
    pub falta_s: i64,
    pub borrable: bool,
}

fn es_libre(t: &Torrent, n: &NormasTracker) -> bool {
    // Sin metadatos (magnet a medias) no se sabe el tamaño: si ya se ha bajado
    // algo, se trata como obligado, que es lo prudente.
    if t.tamano <= 0 {
        return t.descargado <= 0;
    }
    (t.descargado as f64) < n.umbral_pct / 100.0 * t.tamano as f64
}

pub fn clasificar(t: &Torrent, n: &NormasTracker) -> Clasificacion {
    if es_libre(t, n) {
        return Clasificacion {
            estado: Estado::Libre,
            falta_s: 0,
            borrable: true,
        };
    }
    if t.siembra_s < n.siembra_s() {
        return Clasificacion {
            estado: Estado::Obligado,
            falta_s: n.siembra_s() - t.siembra_s,
            borrable: false,
        };
    }
    if t.ratio >= n.ratio_min || t.siembra_s >= n.plazo_largo_s() {
        return Clasificacion {
            estado: Estado::Cumplido,
            falta_s: 0,
            borrable: true,
        };
    }
    Clasificacion {
        estado: Estado::EnEspera,
        falta_s: n.plazo_largo_s() - t.siembra_s,
        borrable: false,
    }
}

/// Torrents con los días de siembra y la ratio para parar que siguen
/// activos: se paran, nunca se borran.
pub fn a_pausar(torrents: &[Torrent], n: &NormasTracker) -> Vec<String> {
    a_pausar_con(torrents, |_| n)
}

/// Como [`a_pausar`], con las normas de cada torrent.
pub fn a_pausar_con<'n>(
    torrents: &[Torrent],
    normas: impl Fn(&Torrent) -> &'n NormasTracker,
) -> Vec<String> {
    torrents
        .iter()
        .filter(|t| {
            let n = normas(t);
            !PARADOS.contains(&t.estado_qbt.as_str())
                && t.siembra_s >= n.siembra_s()
                && t.ratio >= n.ratio_pausa
        })
        .map(|t| t.hash.clone())
        .collect()
}

/// Tipos de regla, en el orden en que se comprueban.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TipoRegla {
    Ratio,
    Siembra,
    Inactividad,
    Disco,
}

/// Reglas de una categoría; `None` es una regla desactivada.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReglasCategoria {
    pub ratio: Option<f64>,
    pub siembra_dias: Option<u32>,
    pub inactividad_dias: Option<u32>,
    /// Porcentaje de disco libre por debajo del cual se libera espacio.
    pub disco_libre_pct: Option<f64>,
}

impl ReglasCategoria {
    /// Comprueba los valores; el mensaje dice qué falla.
    pub fn validar(&self) -> Result<(), &'static str> {
        let entre = |v: f64, min: f64, max: f64| v.is_finite() && v >= min && v <= max;
        if self.ratio.is_some_and(|v| !entre(v, 0.1, 100.0)) {
            return Err("La ratio tiene que estar entre 0,1 y 100.");
        }
        if self.siembra_dias.is_some_and(|d| !(1..=3650).contains(&d)) {
            return Err("Los días sembrando tienen que estar entre 1 y 3650.");
        }
        if self
            .inactividad_dias
            .is_some_and(|d| !(1..=3650).contains(&d))
        {
            return Err("Los días sin subir tienen que estar entre 1 y 3650.");
        }
        if self.disco_libre_pct.is_some_and(|v| !entre(v, 1.0, 90.0)) {
            return Err("El espacio libre tiene que estar entre 1 % y 90 %.");
        }
        Ok(())
    }

    /// Ninguna regla encendida.
    pub fn apagadas(&self) -> bool {
        *self == Self::default()
    }
}

/// Espacio del dataset donde descarga una categoría, en bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Disco {
    pub libre: i64,
    pub total: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Candidato {
    pub hash: String,
    pub regla: TipoRegla,
}

/// Torrents que alguna regla activa quiere borrar, con la primera regla que
/// los elige. Solo Cumplidos y Libres; las exenciones («No borrar») se
/// guardan por torrent y regla.
pub fn candidatos(
    torrents: &[Torrent],
    normas: &NormasTracker,
    reglas: &HashMap<String, ReglasCategoria>,
    discos: &HashMap<String, Disco>,
    exenciones: &HashSet<(String, TipoRegla)>,
    ahora: i64,
) -> Vec<Candidato> {
    candidatos_con(torrents, |_| normas, reglas, discos, exenciones, ahora)
}

/// Como [`candidatos`], con las normas de cada torrent.
pub fn candidatos_con<'n>(
    torrents: &[Torrent],
    normas: impl Fn(&Torrent) -> &'n NormasTracker,
    reglas: &HashMap<String, ReglasCategoria>,
    discos: &HashMap<String, Disco>,
    exenciones: &HashSet<(String, TipoRegla)>,
    ahora: i64,
) -> Vec<Candidato> {
    let exento = |t: &Torrent, r: TipoRegla| exenciones.contains(&(t.hash.clone(), r));
    let mut elegidos: Vec<Candidato> = Vec::new();
    let mut por_disco: HashMap<&str, Vec<&Torrent>> = HashMap::new();

    for t in torrents
        .iter()
        .filter(|t| clasificar(t, normas(t)).borrable)
    {
        let Some(r) = reglas.get(&t.categoria) else {
            continue;
        };
        let cumple = |tipo| match tipo {
            TipoRegla::Ratio => r.ratio.is_some_and(|x| t.ratio >= x),
            TipoRegla::Siembra => r
                .siembra_dias
                .is_some_and(|d| t.siembra_s >= i64::from(d) * 86_400),
            TipoRegla::Inactividad => r
                .inactividad_dias
                .zip(t.ultima_subida_en)
                .is_some_and(|(d, u)| ahora - u >= i64::from(d) * 86_400),
            TipoRegla::Disco => false,
        };
        if let Some(regla) = [TipoRegla::Ratio, TipoRegla::Siembra, TipoRegla::Inactividad]
            .into_iter()
            .find(|&x| cumple(x) && !exento(t, x))
        {
            elegidos.push(Candidato {
                hash: t.hash.clone(),
                regla,
            });
        } else if r.disco_libre_pct.is_some() && !exento(t, TipoRegla::Disco) {
            por_disco.entry(t.categoria.as_str()).or_default().push(t);
        }
    }

    // Disco: del más antiguo al más nuevo hasta volver a pasar del umbral,
    // contando lo que ya liberan los elegidos por otras reglas.
    let mut categorias: Vec<_> = por_disco.into_iter().collect();
    categorias.sort_by_key(|(c, _)| *c);
    for (cat, mut lista) in categorias {
        let (Some(pct), Some(d)) = (reglas[cat].disco_libre_pct, discos.get(cat)) else {
            continue;
        };
        let objetivo = (pct / 100.0 * d.total as f64).ceil() as i64;
        let mut libre = d.libre
            + torrents
                .iter()
                .filter(|t| t.categoria == cat && elegidos.iter().any(|c| c.hash == t.hash))
                .map(|t| t.tamano.max(0))
                .sum::<i64>();
        lista.sort_by(|a, b| {
            a.anadido_en
                .cmp(&b.anadido_en)
                .then_with(|| a.hash.cmp(&b.hash))
        });
        for t in lista {
            if libre >= objetivo {
                break;
            }
            libre += t.tamano.max(0);
            elegidos.push(Candidato {
                hash: t.hash.clone(),
                regla: TipoRegla::Disco,
            });
        }
    }
    elegidos
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Aviso {
    pub hash: String,
    pub regla: TipoRegla,
    pub creado_en: i64,
    pub vence_en: i64,
}

/// Resultado de una pasada del ciclo de avisos.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Paso {
    /// Avisos creados en esta pasada (también están en `vigentes`).
    pub nuevos: Vec<Aviso>,
    /// Ya no cumplen ninguna regla: desaparecen.
    pub retirados: Vec<Aviso>,
    /// Vencidos: toca borrarlos (o, en «solo aviso», apuntarlo).
    pub vencidos: Vec<Aviso>,
    /// Siguen pendientes.
    pub vigentes: Vec<Aviso>,
}

/// Una pasada: crea, retira y vence avisos. `caido_s` son los segundos que la
/// VPN o el puerto han estado caídos desde la pasada anterior y se suman a la
/// hora de borrado; con `red_ok` falso nada vence. Los avisos nuevos vencen
/// a las horas de aviso de las normas.
pub fn avanzar(
    avisos: &[Aviso],
    candidatos: &[Candidato],
    ahora: i64,
    caido_s: i64,
    red_ok: bool,
    normas: &NormasTracker,
) -> Paso {
    avanzar_con(avisos, candidatos, ahora, caido_s, red_ok, |_| normas)
}

/// Como [`avanzar`], con las normas de cada torrent (por su hash): un aviso
/// nuevo vence a las horas de aviso de las suyas.
pub fn avanzar_con<'n>(
    avisos: &[Aviso],
    candidatos: &[Candidato],
    ahora: i64,
    caido_s: i64,
    red_ok: bool,
    normas: impl Fn(&str) -> &'n NormasTracker,
) -> Paso {
    let mut paso = Paso::default();
    for a in avisos {
        let mut a = a.clone();
        a.vence_en = a.vence_en.saturating_add(caido_s.max(0));
        match candidatos.iter().find(|c| c.hash == a.hash) {
            None => paso.retirados.push(a),
            Some(_) if red_ok && ahora >= a.vence_en => paso.vencidos.push(a),
            Some(c) => {
                // Si cambió la regla que lo elige, el plazo sigue corriendo.
                a.regla = c.regla;
                paso.vigentes.push(a);
            }
        }
    }
    for c in candidatos {
        if avisos.iter().all(|a| a.hash != c.hash) {
            let a = Aviso {
                hash: c.hash.clone(),
                regla: c.regla,
                creado_en: ahora,
                vence_en: ahora + normas(&c.hash).aviso_s(),
            };
            paso.nuevos.push(a.clone());
            paso.vigentes.push(a);
        }
    }
    paso
}
