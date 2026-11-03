//! Los presets que exportó cada versión publicada (en `tests/presets/`, ver
//! su `LEEME.md`) se siguen cargando con las mismas normas y reglas.

use std::collections::BTreeMap;
use std::path::PathBuf;

use reglas::preset::{self, Preset};
use reglas::{NormasTracker, ReglasCategoria};

/// Las versiones publicadas con presets. Al publicar una nueva, añádela
/// aquí y guarda en `tests/presets/` el preset que exporta, hecho con su
/// propio código.
const PUBLICADAS: [&str; 7] = [
    "0.3.0", "0.3.1", "0.3.2", "0.4.0", "0.4.1", "0.5.0", "0.6.0",
];

fn carpeta() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/presets")
}

fn leer_archivo(archivo: &str) -> String {
    let ruta = carpeta().join(archivo);
    std::fs::read_to_string(&ruta)
        .unwrap_or_else(|e| panic!("no se puede leer {}: {e}", ruta.display()))
}

/// Las normas por defecto que llevan todos los presets publicados.
fn normas() -> NormasTracker {
    NormasTracker {
        umbral_pct: 12.5,
        siembra_dias: 11,
        ratio_min: 1.25,
        plazo_largo_dias: 45,
        ratio_pausa: 2.75,
        aviso_horas: 36,
    }
}

/// Las reglas de las tres categorías, con cada clave de regla al menos una vez.
fn categorias() -> BTreeMap<String, ReglasCategoria> {
    BTreeMap::from([
        (
            "peliculas".to_string(),
            ReglasCategoria {
                ratio: Some(2.5),
                siembra_dias: Some(21),
                inactividad_dias: None,
                disco_libre_pct: None,
            },
        ),
        (
            "series".to_string(),
            ReglasCategoria {
                ratio: Some(1.75),
                siembra_dias: None,
                inactividad_dias: Some(9),
                disco_libre_pct: None,
            },
        ),
        (
            String::new(),
            ReglasCategoria {
                ratio: None,
                siembra_dias: None,
                inactividad_dias: None,
                disco_libre_pct: Some(12.5),
            },
        ),
    ])
}

/// Las normas de los dos trackers del preset de la 0.6 con trackers.
fn trackers() -> BTreeMap<String, NormasTracker> {
    BTreeMap::from([
        (
            "tracker.ejemplo.example".to_string(),
            NormasTracker {
                umbral_pct: 5.0,
                siembra_dias: 14,
                ratio_min: 1.0,
                plazo_largo_dias: 60,
                ratio_pausa: 3.0,
                aviso_horas: 48,
            },
        ),
        (
            "segundo.example".to_string(),
            NormasTracker {
                umbral_pct: 20.0,
                siembra_dias: 3,
                ratio_min: 0.8,
                plazo_largo_dias: 20,
                ratio_pausa: 1.5,
                aviso_horas: 12,
            },
        ),
    ])
}

fn esperado(version: &str, trackers: BTreeMap<String, NormasTracker>) -> Preset {
    Preset {
        nombre: format!("Exportado por semillero {version}"),
        descripcion: "Preset de compatibilidad: lo escribió esta versión y no se edita.".into(),
        normas: normas(),
        categorias: categorias(),
        trackers,
    }
}

/// Carga `archivo` con el código de ahora y lo compara con `esperado`; el
/// número de formato es el del archivo y, al volver a escribirlo, el mismo.
fn comprobar(archivo: &str, formato: i64, esperado: Preset) {
    let texto = leer_archivo(archivo);
    let tabla: toml::Table = texto.parse().unwrap();
    assert_eq!(
        tabla.get("formato").and_then(toml::Value::as_integer),
        Some(formato),
        "{archivo}"
    );
    let p = Preset::leer(&texto).unwrap_or_else(|e| panic!("{archivo} no carga: {e}"));
    assert_eq!(p, esperado, "{archivo}");
    assert!(
        p.escribir().starts_with(&format!("formato = {formato}\n")),
        "{archivo}"
    );
}

/// Una prueba por archivo y la lista de los archivos probados.
macro_rules! presets_publicados {
    ($($prueba:ident: $archivo:literal, $formato:expr, $esperado:expr;)*) => {
        const PROBADOS: &[&str] = &[$($archivo),*];
        $(
            #[test]
            fn $prueba() {
                comprobar($archivo, $formato, $esperado);
            }
        )*
    };
}

presets_publicados! {
    carga_el_de_la_0_3_0: "0.3.0.toml", preset::FORMATO, esperado("0.3.0", BTreeMap::new());
    carga_el_de_la_0_3_1: "0.3.1.toml", preset::FORMATO, esperado("0.3.1", BTreeMap::new());
    carga_el_de_la_0_3_2: "0.3.2.toml", preset::FORMATO, esperado("0.3.2", BTreeMap::new());
    carga_el_de_la_0_4_0: "0.4.0.toml", preset::FORMATO, esperado("0.4.0", BTreeMap::new());
    carga_el_de_la_0_4_1: "0.4.1.toml", preset::FORMATO, esperado("0.4.1", BTreeMap::new());
    carga_el_de_la_0_5_0: "0.5.0.toml", preset::FORMATO, esperado("0.5.0", BTreeMap::new());
    carga_el_de_la_0_6_0: "0.6.0.toml", preset::FORMATO, esperado("0.6.0", BTreeMap::new());
    carga_el_de_la_0_6_0_con_trackers: "0.6.0-trackers.toml", preset::FORMATO_TRACKERS,
        esperado("0.6.0", trackers());
}

#[test]
fn cada_preset_de_la_carpeta_tiene_su_prueba() {
    let mut sin_prueba = Vec::new();
    for entrada in std::fs::read_dir(carpeta()).unwrap() {
        let nombre = entrada.unwrap().file_name().into_string().unwrap();
        if nombre != "LEEME.md" && !PROBADOS.contains(&nombre.as_str()) {
            sin_prueba.push(nombre);
        }
    }
    assert!(
        sin_prueba.is_empty(),
        "tests/presets/ tiene archivos sin prueba: {sin_prueba:?}. Añade cada uno a \
         presets_publicados! con las normas que lleva."
    );
}

#[test]
fn cada_version_publicada_tiene_su_preset() {
    for version in PUBLICADAS {
        let archivo = format!("{version}.toml");
        assert!(
            carpeta().join(&archivo).is_file(),
            "Falta tests/presets/{archivo}, el preset que exportó semillero {version}. Cada \
             versión publicada guarda el suyo, hecho con su propio código (ver \
             tests/presets/LEEME.md), y lo añade a PUBLICADAS y a presets_publicados!."
        );
        assert!(PROBADOS.contains(&archivo.as_str()), "{archivo} sin prueba");
    }
}
