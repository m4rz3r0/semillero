//! Presets con normas por tracker (formato 2) y compatibilidad con los del
//! formato 1 (semillero 0.3 a 0.5).

use std::collections::BTreeMap;

use reglas::NormasTracker;
use reglas::preset::{self, Preset};

const FORMATO_1: &str = r#"formato = 1
nombre = "De la 0.5"

[normas]
umbral_pct = 5
siembra_dias = 10
ratio_min = 1.5
plazo_largo_dias = 40
ratio_pausa = 3.0
aviso_horas = 48

[categorias.peliculas]
ratio = 2.0
"#;

const FORMATO_2: &str = r#"formato = 2
nombre = "Dos trackers"

[normas]
umbral_pct = 10
siembra_dias = 7
ratio_min = 1.0
plazo_largo_dias = 30
ratio_pausa = 2.0
aviso_horas = 24

[trackers."ejemplo.example"]
umbral_pct = 5
siembra_dias = 14
ratio_min = 1.0
plazo_largo_dias = 60
ratio_pausa = 3.0
aviso_horas = 48

[trackers."tracker.segundo.example"]
umbral_pct = 10
siembra_dias = 3
ratio_min = 0.5
plazo_largo_dias = 10
ratio_pausa = 1.0
aviso_horas = 12
"#;

fn rechaza(texto: &str, palabra: &str) -> String {
    let e = Preset::leer(texto).expect_err(texto);
    assert!(e.contains(palabra), "{e:?} no habla de {palabra:?}");
    e
}

#[test]
fn un_preset_del_formato_1_carga_sus_normas_como_las_de_por_defecto() {
    let p = Preset::leer(FORMATO_1).unwrap();
    assert_eq!(p.normas.siembra_dias, 10);
    assert!(p.trackers.is_empty());
    // Y se vuelve a escribir igual: una 0.5 lo puede importar.
    let texto = p.escribir();
    assert!(texto.starts_with("formato = 1\n"), "{texto}");
    assert!(!texto.contains("trackers"), "{texto}");
}

#[test]
fn el_formato_2_trae_las_normas_de_cada_tracker() {
    let p = Preset::leer(FORMATO_2).unwrap();
    assert_eq!(p.normas, NormasTracker::default());
    assert_eq!(
        p.trackers.keys().collect::<Vec<_>>(),
        ["ejemplo.example", "tracker.segundo.example"]
    );
    assert_eq!(p.trackers["ejemplo.example"].siembra_dias, 14);
    assert_eq!(p.trackers["tracker.segundo.example"].aviso_horas, 12);
}

#[test]
fn con_normas_por_tracker_se_escribe_el_formato_2_y_se_lee_igual() {
    let mut p = Preset::neutro();
    p.nombre = "Con trackers".into();
    p.trackers = BTreeMap::from([(
        "ejemplo.example".to_string(),
        NormasTracker {
            siembra_dias: 9,
            plazo_largo_dias: 9,
            ..NormasTracker::default()
        },
    )]);
    let texto = p.escribir();
    assert!(texto.starts_with("formato = 2\n"), "{texto}");
    assert!(texto.contains("[trackers.\"ejemplo.example\"]"), "{texto}");
    assert_eq!(Preset::leer(&texto), Ok(p.clone()));
    assert_eq!(Preset::leer(&texto).unwrap().escribir(), texto);
}

#[test]
fn sin_normas_por_tracker_se_escribe_el_formato_1() {
    let p = Preset::leer(FORMATO_2).unwrap();
    let sin = Preset {
        trackers: BTreeMap::new(),
        ..p
    };
    assert!(sin.escribir().starts_with("formato = 1\n"));
    assert!(Preset::neutro().escribir().starts_with("formato = 1\n"));
}

#[test]
fn los_hosts_se_guardan_en_minusculas_y_sin_repetir() {
    let p = Preset::leer(&FORMATO_2.replace("\"ejemplo.example\"", "\"EJEMPLO.example\"")).unwrap();
    assert!(p.trackers.contains_key("ejemplo.example"));
    rechaza(
        &FORMATO_2.replace("\"tracker.segundo.example\"", "\"Ejemplo.Example\""),
        "repetido",
    );
}

#[test]
fn cada_fallo_de_las_normas_por_tracker_se_explica() {
    // Normas por tracker en un formato 1: hacen falta el 2 y la 0.6.
    let e = rechaza(
        &FORMATO_2.replace("formato = 2", "formato = 1"),
        "formato = 2",
    );
    assert!(e.contains("0.6"), "{e}");
    // Un formato 2 sin trackers es un formato 1.
    rechaza(
        "formato = 2\nnombre = \"x\"\n[normas]\numbral_pct = 10\nsiembra_dias = 7\n\
         ratio_min = 1.0\nplazo_largo_dias = 30\nratio_pausa = 2.0\naviso_horas = 24\n",
        "formato 2",
    );
    let e = rechaza(
        &FORMATO_2.replace("formato = 2", "formato = 3"),
        "formato 3",
    );
    assert!(e.contains("1 y 2"), "{e}");
    rechaza(
        &FORMATO_2.replace(
            "\"ejemplo.example\"",
            "\"https://ejemplo.example/abc/announce\"",
        ),
        "no es un host",
    );
    rechaza(
        &FORMATO_2.replace("siembra_dias = 14", "siembra_dias = 14\nvelocidad = 3"),
        "trackers.ejemplo.example.velocidad",
    );
    rechaza(
        &FORMATO_2.replace("plazo_largo_dias = 60", "plazo_largo_dias = 2"),
        "ejemplo.example",
    );
    rechaza(
        &FORMATO_2.replace("aviso_horas = 12\n", ""),
        "trackers.tracker.segundo.example.aviso_horas",
    );
    rechaza(
        &FORMATO_1.replace("formato = 1\n", "formato = 2\ntrackers = 3\n"),
        "trackers",
    );
    let mut muchos = FORMATO_2.to_owned();
    for i in 0..preset::MAX_TRACKERS {
        muchos.push_str(&format!(
            "\n[trackers.\"t{i}.segundo.example\"]\numbral_pct = 10\nsiembra_dias = 7\n\
             ratio_min = 1.0\nplazo_largo_dias = 30\nratio_pausa = 2.0\naviso_horas = 24\n"
        ));
    }
    rechaza(&muchos, "máximo");
}
