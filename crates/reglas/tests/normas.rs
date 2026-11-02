//! Normas del tracker como datos: los valores por defecto son los de
//! siempre y, cambiándolos, cambia la clasificación.

use std::collections::{HashMap, HashSet};

use reglas::*;

const DIA: i64 = 86_400;
const N: NormasTracker = NormasTracker::POR_DEFECTO;

fn t(hash: &str) -> Torrent {
    Torrent {
        hash: hash.into(),
        categoria: "peliculas".into(),
        estado_qbt: "uploading".into(),
        descargado: 1000,
        tamano: 1000,
        siembra_s: 8 * DIA,
        ratio: 1.5,
        anadido_en: 0,
        ultima_subida_en: None,
    }
}

#[test]
fn los_valores_por_defecto_son_los_de_siempre() {
    let n = NormasTracker::default();
    assert_eq!(n, N);
    assert_eq!(n.umbral_pct, 10.0);
    assert_eq!(n.siembra_s(), 7 * DIA);
    assert_eq!(n.ratio_min, 1.0);
    assert_eq!(n.plazo_largo_s(), 30 * DIA);
    assert_eq!(n.ratio_pausa, 2.0);
    assert_eq!(n.aviso_s(), 24 * 3600);
    assert_eq!(n.validar(), Ok(()));
}

#[test]
fn con_menos_dias_de_siembra_cumple_antes() {
    let mut x = t("a");
    x.siembra_s = 4 * DIA;
    assert_eq!(clasificar(&x, &N).estado, Estado::Obligado);
    let n = NormasTracker {
        siembra_dias: 3,
        ..N
    };
    assert_eq!(clasificar(&x, &n).estado, Estado::Cumplido);
}

#[test]
fn con_un_umbral_mas_alto_mas_torrents_son_libres() {
    let mut x = t("a");
    x.descargado = 300;
    x.siembra_s = 0;
    assert_eq!(clasificar(&x, &N).estado, Estado::Obligado);
    let n = NormasTracker {
        umbral_pct: 50.0,
        ..N
    };
    assert_eq!(clasificar(&x, &n).estado, Estado::Libre);
}

#[test]
fn la_ratio_minima_y_el_plazo_largo_salen_de_las_normas() {
    let mut x = t("a");
    x.ratio = 1.5;
    x.siembra_s = 10 * DIA;
    assert_eq!(clasificar(&x, &N).estado, Estado::Cumplido);
    let n = NormasTracker {
        ratio_min: 2.0,
        ratio_pausa: 3.0,
        plazo_largo_dias: 15,
        ..N
    };
    let c = clasificar(&x, &n);
    assert_eq!(c.estado, Estado::EnEspera);
    assert_eq!(c.falta_s, 5 * DIA);
    x.siembra_s = 15 * DIA;
    assert_eq!(clasificar(&x, &n).estado, Estado::Cumplido);
}

#[test]
fn la_pausa_y_el_aviso_salen_de_las_normas() {
    let mut a = t("a");
    a.ratio = 2.5;
    assert_eq!(
        a_pausar(std::slice::from_ref(&a), &N),
        vec!["a".to_string()]
    );
    let n = NormasTracker {
        ratio_pausa: 3.0,
        aviso_horas: 48,
        ..N
    };
    assert!(a_pausar(std::slice::from_ref(&a), &n).is_empty());
    let c = Candidato {
        hash: "a".into(),
        regla: TipoRegla::Ratio,
    };
    let p = avanzar(&[], std::slice::from_ref(&c), 100, 0, true, &n);
    assert_eq!(p.nuevos[0].vence_en, 100 + 48 * 3600);
}

#[test]
fn las_normas_deciden_que_puede_recoger_una_regla() {
    let r = HashMap::from([(
        "peliculas".to_string(),
        ReglasCategoria {
            ratio: Some(1.0),
            ..Default::default()
        },
    )]);
    let mut x = t("a");
    x.siembra_s = 4 * DIA;
    let sin = HashMap::new();
    let ex = HashSet::new();
    assert!(candidatos(std::slice::from_ref(&x), &N, &r, &sin, &ex, 0).is_empty());
    let n = NormasTracker {
        siembra_dias: 3,
        ..N
    };
    assert_eq!(candidatos(&[x], &n, &r, &sin, &ex, 0).len(), 1);
}

#[test]
fn las_normas_fuera_de_rango_se_rechazan_con_su_motivo() {
    let casos: [(NormasTracker, &str); 10] = [
        (
            NormasTracker {
                umbral_pct: -1.0,
                ..N
            },
            "porcentaje",
        ),
        (
            NormasTracker {
                umbral_pct: 100.5,
                ..N
            },
            "porcentaje",
        ),
        (
            NormasTracker {
                umbral_pct: f64::NAN,
                ..N
            },
            "porcentaje",
        ),
        (
            NormasTracker {
                siembra_dias: 366,
                plazo_largo_dias: 366,
                ..N
            },
            "días de siembra",
        ),
        (
            NormasTracker {
                ratio_min: 101.0,
                ratio_pausa: 101.0,
                ..N
            },
            "ratio mínima",
        ),
        (
            NormasTracker {
                plazo_largo_dias: 400,
                ..N
            },
            "plazo máximo",
        ),
        (
            NormasTracker {
                plazo_largo_dias: 5,
                ..N
            },
            "plazo máximo",
        ),
        (
            NormasTracker {
                ratio_pausa: 0.5,
                ..N
            },
            "ratio para parar",
        ),
        (
            NormasTracker {
                aviso_horas: 0,
                ..N
            },
            "horas de aviso",
        ),
        (
            NormasTracker {
                aviso_horas: 721,
                ..N
            },
            "horas de aviso",
        ),
    ];
    for (n, palabra) in casos {
        let e = n.validar().expect_err(&format!("{n:?}"));
        assert!(e.contains(palabra), "{e:?} no habla de {palabra:?}");
    }
    // Los extremos valen.
    let extremos = NormasTracker {
        umbral_pct: 0.0,
        siembra_dias: 0,
        ratio_min: 0.0,
        plazo_largo_dias: 365,
        ratio_pausa: 100.0,
        aviso_horas: 1,
    };
    assert_eq!(extremos.validar(), Ok(()));
}
