//! Presets: las normas del tracker y las reglas de cada categoría, con un
//! nombre y una descripción, en un archivo TOML que se puede compartir.
//!
//! ```toml
//! formato = 1
//! nombre = "Tracker exigente"
//! descripcion = "Siembra 10 días y ratio 1,5."
//!
//! [normas]
//! umbral_pct = 10.0
//! siembra_dias = 10
//! ratio_min = 1.5
//! plazo_largo_dias = 40
//! ratio_pausa = 3.0
//! aviso_horas = 24
//!
//! [categorias.peliculas]
//! ratio = 2.0
//! siembra_dias = 30
//!
//! [categorias.sin_categoria]
//! disco_libre_pct = 10.0
//! ```
//!
//! Una regla que no está, está apagada; una categoría que no está, tiene
//! todas apagadas. Todo se comprueba al leer: un archivo con algo de más, de
//! menos o fuera de rango se rechaza entero con una frase que dice qué falla.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use toml::{Table, Value};

use crate::{NormasTracker, ReglasCategoria};

/// La versión del formato que se lee y se escribe.
pub const FORMATO: i64 = 1;
/// Tamaño máximo de un archivo de preset, en bytes.
pub const MAX_BYTES: usize = 64 * 1024;
/// Caracteres como mucho en el nombre.
pub const MAX_NOMBRE: usize = 60;
/// Caracteres como mucho en la descripción.
pub const MAX_DESCRIPCION: usize = 300;
/// Categorías como mucho en un preset.
pub const MAX_CATEGORIAS: usize = 16;
/// Caracteres como mucho en el nombre de una categoría.
pub const MAX_CATEGORIA: usize = 32;
/// Cómo se escribe en el archivo la categoría vacía (torrents sin categoría).
pub const SIN_CATEGORIA: &str = "sin_categoria";

const CLAVES: [&str; 5] = ["formato", "nombre", "descripcion", "normas", "categorias"];
const CLAVES_NORMAS: [&str; 6] = [
    "umbral_pct",
    "siembra_dias",
    "ratio_min",
    "plazo_largo_dias",
    "ratio_pausa",
    "aviso_horas",
];
const CLAVES_REGLAS: [&str; 4] = [
    "ratio",
    "siembra_dias",
    "inactividad_dias",
    "disco_libre_pct",
];

/// Un preset. Las categorías van por su código de qBittorrent (`""` es sin
/// categoría).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub nombre: String,
    pub descripcion: String,
    pub normas: NormasTracker,
    pub categorias: BTreeMap<String, ReglasCategoria>,
}

/// Comprueba un nombre de preset y lo devuelve sin espacios a los lados.
pub fn validar_nombre(nombre: &str) -> Result<String, &'static str> {
    let n = nombre.trim();
    if n.is_empty() {
        return Err("Escribe un nombre para el preset.");
    }
    if n.chars().count() > MAX_NOMBRE {
        return Err("El nombre del preset es demasiado largo (60 caracteres como mucho).");
    }
    if n.chars().any(char::is_control) {
        return Err(
            "El nombre del preset no puede llevar saltos de línea ni caracteres de control.",
        );
    }
    Ok(n.to_owned())
}

/// Comprueba una descripción de preset y la devuelve sin espacios a los lados.
pub fn validar_descripcion(descripcion: &str) -> Result<String, &'static str> {
    let d = descripcion.trim();
    if d.chars().count() > MAX_DESCRIPCION {
        return Err("La descripción del preset es demasiado larga (300 caracteres como mucho).");
    }
    if d.chars().any(|c| c.is_control() && c != '\n') {
        return Err("La descripción del preset no puede llevar caracteres de control.");
    }
    Ok(d.to_owned())
}

impl Preset {
    /// El preset incluido: las normas por defecto y ninguna regla de recogida.
    pub fn neutro() -> Self {
        Preset {
            nombre: "Por defecto".into(),
            descripcion: "Normas del tracker por defecto (siembra 7 días, ratio 1,0 o 30 días \
                          sembrando) y todas las reglas de recogida apagadas."
                .into(),
            normas: NormasTracker::default(),
            categorias: BTreeMap::new(),
        }
    }

    /// Comprueba todo el preset; el mensaje dice qué falla.
    pub fn validar(&self) -> Result<(), String> {
        validar_nombre(&self.nombre)?;
        validar_descripcion(&self.descripcion)?;
        self.normas.validar()?;
        if self.categorias.len() > MAX_CATEGORIAS {
            return Err(format!(
                "Hay {} categorías y el máximo es {MAX_CATEGORIAS}.",
                self.categorias.len()
            ));
        }
        for (cat, r) in &self.categorias {
            if cat.chars().count() > MAX_CATEGORIA || cat.chars().any(char::is_control) {
                return Err(format!(
                    "El nombre de categoría «{}» no vale: {MAX_CATEGORIA} caracteres como mucho y \
                     sin caracteres de control.",
                    mostrar(cat)
                ));
            }
            r.validar()
                .map_err(|e| format!("Categoría «{}»: {e}", clave_de(cat)))?;
        }
        Ok(())
    }

    /// Lee un archivo de preset. Si algo falla, nada: el mensaje dice qué.
    pub fn leer(texto: &str) -> Result<Preset, String> {
        if texto.len() > MAX_BYTES {
            return Err("El archivo pasa de 64 KiB: un preset es mucho más pequeño.".into());
        }
        let tabla: Table = texto.parse().map_err(|e: toml::de::Error| match e.span() {
            Some(s) => format!(
                "El archivo no se puede leer como TOML (línea {}).",
                texto
                    .get(..s.start)
                    .map_or(1, |antes| antes.matches('\n').count() + 1)
            ),
            None => "El archivo no se puede leer como TOML.".to_owned(),
        })?;

        match tabla.get("formato") {
            None => {
                return Err(
                    "Falta «formato = 1» al principio: no parece un preset de semillero.".into(),
                );
            }
            Some(Value::Integer(FORMATO)) => {}
            Some(Value::Integer(n)) => {
                return Err(format!(
                    "Este preset es del formato {n} y semillero solo entiende el formato {FORMATO}."
                ));
            }
            Some(_) => return Err("«formato» tiene que ser un número: formato = 1.".into()),
        }
        sobrantes(&tabla, &CLAVES, "")?;

        let nombre = match tabla.get("nombre") {
            Some(Value::String(s)) => validar_nombre(s)?,
            Some(_) => return Err("El nombre («nombre») tiene que ir entre comillas.".into()),
            None => return Err("Falta el nombre del preset («nombre»).".into()),
        };
        let descripcion = match tabla.get("descripcion") {
            Some(Value::String(s)) => validar_descripcion(s)?,
            Some(_) => {
                return Err("La descripción («descripcion») tiene que ir entre comillas.".into());
            }
            None => String::new(),
        };

        let normas = match tabla.get("normas") {
            Some(Value::Table(t)) => leer_normas(t)?,
            Some(_) => {
                return Err(
                    "«normas» tiene que ser una sección: una línea [normas] y debajo \
                            sus valores."
                        .into(),
                );
            }
            None => return Err("Falta la sección [normas] con las normas del tracker.".into()),
        };

        let mut categorias = BTreeMap::new();
        match tabla.get("categorias") {
            None => {}
            Some(Value::Table(t)) => {
                if t.len() > MAX_CATEGORIAS {
                    return Err(format!(
                        "Hay {} categorías y el máximo es {MAX_CATEGORIAS}.",
                        t.len()
                    ));
                }
                for (clave, v) in t {
                    let Value::Table(reglas) = v else {
                        return Err(format!(
                            "«categorias.{0}» tiene que ser una sección: [categorias.{0}].",
                            mostrar(clave)
                        ));
                    };
                    let cat = categoria_de(clave)?;
                    categorias.insert(cat, leer_reglas(clave, reglas)?);
                }
            }
            Some(_) => {
                return Err(
                    "«categorias» tiene que ir en secciones: [categorias.peliculas], …".into(),
                );
            }
        }

        let p = Preset {
            nombre,
            descripcion,
            normas,
            categorias,
        };
        p.validar()?;
        Ok(p)
    }

    /// El archivo del preset, listo para compartir. [`Preset::leer`] lo lee
    /// tal cual.
    pub fn escribir(&self) -> String {
        #[derive(Serialize)]
        struct Archivo<'a> {
            formato: i64,
            nombre: &'a str,
            descripcion: &'a str,
            normas: &'a NormasTracker,
            categorias: BTreeMap<&'a str, Reglas>,
        }
        #[derive(Serialize)]
        struct Reglas {
            #[serde(skip_serializing_if = "Option::is_none")]
            ratio: Option<f64>,
            #[serde(skip_serializing_if = "Option::is_none")]
            siembra_dias: Option<u32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            inactividad_dias: Option<u32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            disco_libre_pct: Option<f64>,
        }
        let archivo = Archivo {
            formato: FORMATO,
            nombre: &self.nombre,
            descripcion: &self.descripcion,
            normas: &self.normas,
            categorias: self
                .categorias
                .iter()
                .map(|(c, r)| {
                    (
                        clave_de(c),
                        Reglas {
                            ratio: r.ratio,
                            siembra_dias: r.siembra_dias,
                            inactividad_dias: r.inactividad_dias,
                            disco_libre_pct: r.disco_libre_pct,
                        },
                    )
                })
                .collect(),
        };
        // Solo números, textos y tablas: no puede fallar.
        toml::to_string(&archivo).unwrap_or_default()
    }
}

/// Una clave del archivo para enseñarla en un mensaje: corta y sin
/// caracteres raros.
fn mostrar(clave: &str) -> String {
    let limpia: String = clave
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_CATEGORIA)
        .collect();
    if limpia.chars().count() < clave.chars().count() {
        format!("{limpia}…")
    } else {
        limpia
    }
}

fn clave_de(categoria: &str) -> &str {
    if categoria.is_empty() {
        SIN_CATEGORIA
    } else {
        categoria
    }
}

fn categoria_de(clave: &str) -> Result<String, String> {
    if clave.is_empty() {
        return Err(format!(
            "Una categoría no tiene nombre: para los torrents sin categoría usa \
             [categorias.{SIN_CATEGORIA}]."
        ));
    }
    if clave.chars().count() > MAX_CATEGORIA {
        return Err(format!(
            "El nombre de categoría «{}» es demasiado largo ({MAX_CATEGORIA} caracteres como \
             mucho).",
            mostrar(clave)
        ));
    }
    Ok(if clave == SIN_CATEGORIA {
        String::new()
    } else {
        clave.to_owned()
    })
}

/// Rechaza las claves de `t` que no están en `validas`; `donde` es el
/// prefijo con que se nombran («normas.»).
fn sobrantes(t: &Table, validas: &[&str], donde: &str) -> Result<(), String> {
    match t.keys().find(|k| !validas.contains(&k.as_str())) {
        Some(k) => Err(format!(
            "«{donde}{}» sobra: aquí solo pueden ir {}.",
            mostrar(k),
            lista(validas)
        )),
        None => Ok(()),
    }
}

fn lista(claves: &[&str]) -> String {
    match claves {
        [] => String::new(),
        [una] => (*una).to_owned(),
        [resto @ .., ultima] => format!("{} y {ultima}", resto.join(", ")),
    }
}

fn numero(v: &Value, nombre: &str) -> Result<f64, String> {
    match v {
        Value::Float(f) => Ok(*f),
        Value::Integer(i) => Ok(*i as f64),
        _ => Err(format!("«{nombre}» tiene que ser un número.")),
    }
}

fn entero(v: &Value, nombre: &str) -> Result<u32, String> {
    match v {
        Value::Integer(i) if *i < 0 => Err(format!("«{nombre}» no puede ser negativo.")),
        Value::Integer(i) => {
            u32::try_from(*i).map_err(|_| format!("«{nombre}» es demasiado grande."))
        }
        _ => Err(format!(
            "«{nombre}» tiene que ser un número entero, sin decimales."
        )),
    }
}

fn leer_normas(t: &Table) -> Result<NormasTracker, String> {
    sobrantes(t, &CLAVES_NORMAS, "normas.")?;
    let campo = |k: &str| {
        t.get(k)
            .ok_or_else(|| format!("Falta «normas.{k}» en la sección [normas]."))
    };
    let num = |k: &str| numero(campo(k)?, &format!("normas.{k}"));
    let ent = |k: &str| entero(campo(k)?, &format!("normas.{k}"));
    Ok(NormasTracker {
        umbral_pct: num("umbral_pct")?,
        siembra_dias: ent("siembra_dias")?,
        ratio_min: num("ratio_min")?,
        plazo_largo_dias: ent("plazo_largo_dias")?,
        ratio_pausa: num("ratio_pausa")?,
        aviso_horas: ent("aviso_horas")?,
    })
}

fn leer_reglas(clave: &str, t: &Table) -> Result<ReglasCategoria, String> {
    let donde = format!("categorias.{}.", mostrar(clave));
    sobrantes(t, &CLAVES_REGLAS, &donde)?;
    let num = |k: &str| {
        t.get(k)
            .map(|v| numero(v, &format!("{donde}{k}")))
            .transpose()
    };
    let ent = |k: &str| {
        t.get(k)
            .map(|v| entero(v, &format!("{donde}{k}")))
            .transpose()
    };
    Ok(ReglasCategoria {
        ratio: num("ratio")?,
        siembra_dias: ent("siembra_dias")?,
        inactividad_dias: ent("inactividad_dias")?,
        disco_libre_pct: num("disco_libre_pct")?,
    })
}
