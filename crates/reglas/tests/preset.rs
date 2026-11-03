//! Archivos de preset: lectura, validación y vuelta completa.

use std::collections::BTreeMap;

use reglas::preset::{self, Preset};
use reglas::{Estado, NormasTracker, ReglasCategoria, Torrent, clasificar};

const BUENO: &str = r#"
formato = 1
nombre = "Tracker exigente"
descripcion = "Siembra 10 días y ratio 1,5."

[normas]
umbral_pct = 5
siembra_dias = 10
ratio_min = 1.5
plazo_largo_dias = 40
ratio_pausa = 3.0
aviso_horas = 48

[categorias.peliculas]
ratio = 2.0
siembra_dias = 30

[categorias.sin_categoria]
disco_libre_pct = 15
"#;

fn bueno() -> Preset {
    Preset {
        nombre: "Tracker exigente".into(),
        descripcion: "Siembra 10 días y ratio 1,5.".into(),
        normas: NormasTracker {
            umbral_pct: 5.0,
            siembra_dias: 10,
            ratio_min: 1.5,
            plazo_largo_dias: 40,
            ratio_pausa: 3.0,
            aviso_horas: 48,
        },
        categorias: BTreeMap::from([
            (
                "peliculas".to_string(),
                ReglasCategoria {
                    ratio: Some(2.0),
                    siembra_dias: Some(30),
                    ..Default::default()
                },
            ),
            (
                String::new(),
                ReglasCategoria {
                    disco_libre_pct: Some(15.0),
                    ..Default::default()
                },
            ),
        ]),
        trackers: BTreeMap::new(),
    }
}

#[test]
fn un_archivo_bueno_se_lee_entero() {
    assert_eq!(Preset::leer(BUENO), Ok(bueno()));
}

#[test]
fn escribir_y_volver_a_leer_da_lo_mismo() {
    for p in [bueno(), Preset::neutro()] {
        let texto = p.escribir();
        assert!(texto.starts_with("formato = 1\n"), "{texto}");
        assert_eq!(Preset::leer(&texto), Ok(p.clone()), "{texto}");
        // Y escribirlo otra vez da el mismo texto.
        assert_eq!(Preset::leer(&texto).unwrap().escribir(), texto);
    }
}

#[test]
fn el_neutro_tiene_las_normas_por_defecto_y_ninguna_regla() {
    let n = Preset::neutro();
    assert_eq!(n.normas, NormasTracker::default());
    assert!(n.categorias.is_empty());
    assert!(!n.nombre.is_empty() && !n.descripcion.is_empty());
    assert_eq!(n.validar(), Ok(()));
}

#[test]
fn sin_descripcion_ni_categorias_tambien_vale() {
    let texto = "formato = 1\nnombre = \"Corto\"\n[normas]\numbral_pct = 10\nsiembra_dias = 7\n\
                 ratio_min = 1\nplazo_largo_dias = 30\nratio_pausa = 2\naviso_horas = 24\n";
    let p = Preset::leer(texto).unwrap();
    assert_eq!(p.descripcion, "");
    assert!(p.categorias.is_empty());
    assert_eq!(p.normas, NormasTracker::default());
}

#[test]
fn aplicar_un_preset_cambia_la_clasificacion() {
    let x = Torrent {
        hash: "a".into(),
        categoria: "peliculas".into(),
        estado_qbt: "uploading".into(),
        descargado: 1000,
        tamano: 1000,
        siembra_s: 8 * 86_400,
        ratio: 1.2,
        anadido_en: 0,
        ultima_subida_en: None,
    };
    assert_eq!(
        clasificar(&x, &Preset::neutro().normas).estado,
        Estado::Cumplido
    );
    assert_eq!(clasificar(&x, &bueno().normas).estado, Estado::Obligado);
}

/// El error de leer `texto`, que tiene que hablar de `palabra`.
fn rechaza(texto: &str, palabra: &str) {
    let e = Preset::leer(texto).expect_err(texto);
    assert!(
        e.contains(palabra),
        "el error {e:?} no habla de {palabra:?}"
    );
    // Siempre una frase en castellano, sin la jerga del analizador.
    assert!(e.ends_with('.'), "{e:?}");
    for jerga in [
        "expected",
        "unknown field",
        "invalid type",
        "TOML parse error",
    ] {
        assert!(!e.contains(jerga), "{e:?}");
    }
}

#[test]
fn cada_fallo_se_explica() {
    rechaza(&BUENO.replace("formato = 1", ""), "formato");
    rechaza(&BUENO.replace("formato = 1", "formato = 2"), "formato 2");
    rechaza(
        &BUENO.replace("formato = 1", "formato = \"uno\""),
        "formato",
    );
    rechaza(
        &BUENO.replace("nombre =", "color = \"verde\"\nnombre ="),
        "color",
    );
    rechaza(
        &BUENO.replace("aviso_horas = 48", "aviso_horas = 48\nvelocidad = 3"),
        "velocidad",
    );
    rechaza(
        &BUENO.replace("ratio = 2.0", "ratio = 2.0\nprisa = true"),
        "prisa",
    );
    rechaza(&BUENO.replace("aviso_horas = 48\n", ""), "aviso_horas");
    rechaza(
        &BUENO.replace("siembra_dias = 10", "siembra_dias = 9000"),
        "días de siembra",
    );
    rechaza(
        &BUENO.replace("siembra_dias = 10", "siembra_dias = 2.5"),
        "entero",
    );
    rechaza(
        &BUENO.replace("siembra_dias = 10", "siembra_dias = -1"),
        "siembra_dias",
    );
    rechaza(
        &BUENO.replace("umbral_pct = 5", "umbral_pct = \"mucho\""),
        "número",
    );
    rechaza(&BUENO.replace("ratio = 2.0", "ratio = 500.0"), "ratio");
    rechaza(
        &BUENO.replace("disco_libre_pct = 15", "disco_libre_pct = 99"),
        "espacio libre",
    );
    rechaza("formato = 1\nnombre = \"x\"\nnormas = 3\n", "normas");
    rechaza("formato = 1\nnombre = \"x\"\n", "normas");
    rechaza(
        &BUENO.replace("nombre = \"Tracker exigente\"", "nombre = \"  \""),
        "nombre",
    );
    let largo = "x".repeat(preset::MAX_NOMBRE + 1);
    rechaza(&BUENO.replace("Tracker exigente", &largo), "nombre");
    let larga = "x".repeat(preset::MAX_DESCRIPCION + 1);
    rechaza(
        &BUENO.replace("Siembra 10 días y ratio 1,5.", &larga),
        "descripción",
    );
    rechaza(
        &BUENO.replace("nombre = \"Tracker exigente\"", "nombre = \"a\\u0007b\""),
        "nombre",
    );
    rechaza("formato = 1\nnombre = \"sin cerrar\n", "línea 2");
    rechaza("", "formato");
}

#[test]
fn demasiadas_categorias_o_demasiado_grande_no() {
    let mut muchas = BUENO.to_owned();
    for i in 0..preset::MAX_CATEGORIAS {
        muchas.push_str(&format!("\n[categorias.c{i}]\nratio = 2.0\n"));
    }
    rechaza(&muchas, "categorías");
    let largo = format!("{BUENO}\n# {}\n", "x".repeat(preset::MAX_BYTES));
    rechaza(&largo, "64 KiB");
    let clave = "c".repeat(preset::MAX_CATEGORIA + 1);
    rechaza(
        &format!("{BUENO}\n[categorias.{clave}]\nratio = 2.0\n"),
        "categoría",
    );
}
