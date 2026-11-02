use std::collections::{HashMap, HashSet};

use reglas::*;

const DIA: i64 = 86_400;
const GIB: i64 = 1 << 30;
const N: NormasTracker = NormasTracker::POR_DEFECTO;
const AVISO_S: i64 = N.aviso_s();

fn t(hash: &str) -> Torrent {
    Torrent {
        hash: hash.into(),
        categoria: "peliculas".into(),
        estado_qbt: "uploading".into(),
        descargado: 10 * GIB,
        tamano: 10 * GIB,
        siembra_s: 8 * DIA,
        ratio: 1.5,
        anadido_en: 0,
        ultima_subida_en: None,
    }
}

// Clasificación: los mismos casos que la versión anterior.

#[test]
fn menos_del_diez_por_ciento_es_libre() {
    let mut x = t("a");
    x.descargado = GIB - 1;
    assert_eq!(
        clasificar(&x, &N),
        Clasificacion {
            estado: Estado::Libre,
            falta_s: 0,
            borrable: true
        }
    );
}

#[test]
fn justo_el_diez_por_ciento_obliga() {
    let mut x = t("a");
    x.descargado = GIB;
    x.siembra_s = 0;
    assert_eq!(clasificar(&x, &N).estado, Estado::Obligado);
}

#[test]
fn sin_tamano_y_con_descarga_es_obligado() {
    let mut x = t("a");
    x.tamano = 0;
    x.descargado = 5;
    x.siembra_s = 0;
    assert_eq!(clasificar(&x, &N).estado, Estado::Obligado);
    x.descargado = 0;
    assert_eq!(clasificar(&x, &N).estado, Estado::Libre);
}

#[test]
fn obligado_dice_cuanto_falta() {
    let mut x = t("a");
    x.siembra_s = 3 * DIA;
    assert_eq!(
        clasificar(&x, &N),
        Clasificacion {
            estado: Estado::Obligado,
            falta_s: 4 * DIA,
            borrable: false
        }
    );
}

#[test]
fn siete_dias_y_ratio_uno_cumple() {
    let mut x = t("a");
    x.siembra_s = 7 * DIA;
    x.ratio = 1.0;
    assert_eq!(
        clasificar(&x, &N),
        Clasificacion {
            estado: Estado::Cumplido,
            falta_s: 0,
            borrable: true
        }
    );
}

#[test]
fn ratio_bajo_espera_hasta_treinta_dias() {
    let mut x = t("a");
    x.ratio = 0.4;
    assert_eq!(
        clasificar(&x, &N),
        Clasificacion {
            estado: Estado::EnEspera,
            falta_s: 22 * DIA,
            borrable: false
        }
    );
    x.siembra_s = 30 * DIA;
    assert_eq!(clasificar(&x, &N).estado, Estado::Cumplido);
}

#[test]
fn pausa_solo_activos_con_siete_dias_y_ratio_dos() {
    let mut a = t("a");
    a.ratio = 2.0;
    let mut b = t("b");
    b.ratio = 2.0;
    b.estado_qbt = "stoppedUP".into();
    let mut c = t("c");
    c.ratio = 1.9;
    let mut d = t("d");
    d.ratio = 3.0;
    d.siembra_s = 6 * DIA;
    assert_eq!(a_pausar(&[a, b, c, d], &N), vec!["a".to_string()]);
}

// Reglas de borrado.

fn reglas(r: ReglasCategoria) -> HashMap<String, ReglasCategoria> {
    HashMap::from([("peliculas".to_string(), r)])
}

fn sin_discos() -> HashMap<String, Disco> {
    HashMap::new()
}

#[test]
fn reglas_desactivadas_por_defecto_no_eligen_nada() {
    let r = reglas(ReglasCategoria::default());
    assert!(candidatos(&[t("a")], &N, &r, &sin_discos(), &HashSet::new(), 100 * DIA).is_empty());
}

#[test]
fn regla_de_ratio() {
    let r = reglas(ReglasCategoria {
        ratio: Some(1.5),
        ..Default::default()
    });
    let mut b = t("b");
    b.ratio = 1.4;
    let c = candidatos(
        &[t("a"), b],
        &N,
        &r,
        &sin_discos(),
        &HashSet::new(),
        100 * DIA,
    );
    assert_eq!(
        c,
        vec![Candidato {
            hash: "a".into(),
            regla: TipoRegla::Ratio
        }]
    );
}

#[test]
fn regla_de_tiempo_sembrando() {
    let r = reglas(ReglasCategoria {
        siembra_dias: Some(30),
        ..Default::default()
    });
    let mut b = t("b");
    b.siembra_s = 30 * DIA;
    let c = candidatos(
        &[t("a"), b],
        &N,
        &r,
        &sin_discos(),
        &HashSet::new(),
        100 * DIA,
    );
    assert_eq!(
        c,
        vec![Candidato {
            hash: "b".into(),
            regla: TipoRegla::Siembra
        }]
    );
}

#[test]
fn regla_de_inactividad() {
    let ahora = 100 * DIA;
    let r = reglas(ReglasCategoria {
        inactividad_dias: Some(14),
        ..Default::default()
    });
    let mut a = t("a");
    a.ultima_subida_en = Some(ahora - 14 * DIA);
    let mut b = t("b");
    b.ultima_subida_en = Some(ahora - 13 * DIA);
    // Sin muestras todavía no se sabe: no se elige.
    let c0 = t("c");
    let c = candidatos(&[a, b, c0], &N, &r, &sin_discos(), &HashSet::new(), ahora);
    assert_eq!(
        c,
        vec![Candidato {
            hash: "a".into(),
            regla: TipoRegla::Inactividad
        }]
    );
}

#[test]
fn regla_de_disco_borra_del_mas_antiguo_hasta_recuperar_el_umbral() {
    let r = reglas(ReglasCategoria {
        disco_libre_pct: Some(10.0),
        ..Default::default()
    });
    // 100 GiB, 5 libres: hacen falta 5 GiB más.
    let discos = HashMap::from([(
        "peliculas".to_string(),
        Disco {
            libre: 5 * GIB,
            total: 100 * GIB,
        },
    )]);
    let mut viejo = t("viejo");
    viejo.anadido_en = 1;
    viejo.tamano = 3 * GIB;
    viejo.descargado = 3 * GIB;
    let mut medio = t("medio");
    medio.anadido_en = 2;
    medio.tamano = 3 * GIB;
    medio.descargado = 3 * GIB;
    let mut nuevo = t("nuevo");
    nuevo.anadido_en = 3;
    let c = candidatos(
        &[nuevo, medio, viejo],
        &N,
        &r,
        &discos,
        &HashSet::new(),
        100 * DIA,
    );
    assert_eq!(
        c,
        vec![
            Candidato {
                hash: "viejo".into(),
                regla: TipoRegla::Disco
            },
            Candidato {
                hash: "medio".into(),
                regla: TipoRegla::Disco
            },
        ]
    );
}

#[test]
fn regla_de_disco_con_espacio_suficiente_no_elige() {
    let r = reglas(ReglasCategoria {
        disco_libre_pct: Some(10.0),
        ..Default::default()
    });
    let discos = HashMap::from([(
        "peliculas".to_string(),
        Disco {
            libre: 10 * GIB,
            total: 100 * GIB,
        },
    )]);
    assert!(candidatos(&[t("a")], &N, &r, &discos, &HashSet::new(), 100 * DIA).is_empty());
}

#[test]
fn nunca_elige_obligados_ni_en_espera() {
    let r = reglas(ReglasCategoria {
        ratio: Some(0.0),
        siembra_dias: Some(0),
        inactividad_dias: Some(0),
        disco_libre_pct: Some(100.0),
    });
    let discos = HashMap::from([(
        "peliculas".to_string(),
        Disco {
            libre: 0,
            total: 100 * GIB,
        },
    )]);
    let mut obligado = t("o");
    obligado.siembra_s = DIA;
    obligado.ultima_subida_en = Some(0);
    let mut espera = t("e");
    espera.ratio = 0.1;
    espera.ultima_subida_en = Some(0);
    assert!(
        candidatos(
            &[obligado, espera],
            &N,
            &r,
            &discos,
            &HashSet::new(),
            100 * DIA
        )
        .is_empty()
    );
}

#[test]
fn las_reglas_son_por_categoria() {
    let r = reglas(ReglasCategoria {
        ratio: Some(1.0),
        ..Default::default()
    });
    let mut s = t("s");
    s.categoria = "series".into();
    assert!(candidatos(&[s], &N, &r, &sin_discos(), &HashSet::new(), 100 * DIA).is_empty());
}

#[test]
fn una_exencion_salta_esa_regla_pero_no_las_demas() {
    let r = reglas(ReglasCategoria {
        ratio: Some(1.0),
        siembra_dias: Some(8),
        ..Default::default()
    });
    let ex = HashSet::from([("a".to_string(), TipoRegla::Ratio)]);
    let c = candidatos(&[t("a")], &N, &r, &sin_discos(), &ex, 100 * DIA);
    assert_eq!(
        c,
        vec![Candidato {
            hash: "a".into(),
            regla: TipoRegla::Siembra
        }]
    );
    let ex = HashSet::from([
        ("a".to_string(), TipoRegla::Ratio),
        ("a".to_string(), TipoRegla::Siembra),
    ]);
    assert!(candidatos(&[t("a")], &N, &r, &sin_discos(), &ex, 100 * DIA).is_empty());
}

// Ciclo de los avisos.

fn cand(h: &str) -> Candidato {
    Candidato {
        hash: h.into(),
        regla: TipoRegla::Ratio,
    }
}

#[test]
fn un_candidato_nuevo_recibe_aviso_de_24_horas() {
    let p = avanzar(&[], &[cand("a")], 1000, 0, true, &N);
    let esperado = Aviso {
        hash: "a".into(),
        regla: TipoRegla::Ratio,
        creado_en: 1000,
        vence_en: 1000 + AVISO_S,
    };
    assert_eq!(p.nuevos, vec![esperado.clone()]);
    assert_eq!(p.vigentes, vec![esperado]);
    assert!(p.vencidos.is_empty() && p.retirados.is_empty());
}

#[test]
fn el_aviso_vence_a_las_24_horas() {
    let a = Aviso {
        hash: "a".into(),
        regla: TipoRegla::Ratio,
        creado_en: 0,
        vence_en: AVISO_S,
    };
    let p = avanzar(
        std::slice::from_ref(&a),
        &[cand("a")],
        AVISO_S - 1,
        0,
        true,
        &N,
    );
    assert!(p.vencidos.is_empty());
    assert_eq!(p.vigentes, vec![a.clone()]);
    let p = avanzar(std::slice::from_ref(&a), &[cand("a")], AVISO_S, 0, true, &N);
    assert_eq!(p.vencidos, vec![a]);
    assert!(p.vigentes.is_empty());
}

#[test]
fn si_deja_de_cumplir_la_regla_el_aviso_se_retira() {
    let a = Aviso {
        hash: "a".into(),
        regla: TipoRegla::Ratio,
        creado_en: 0,
        vence_en: AVISO_S,
    };
    let p = avanzar(std::slice::from_ref(&a), &[], 2 * AVISO_S, 0, true, &N);
    assert_eq!(p.retirados, vec![a]);
    assert!(p.vencidos.is_empty() && p.vigentes.is_empty());
}

#[test]
fn con_la_vpn_caida_nada_vence_y_el_tiempo_caido_se_suma() {
    let a = Aviso {
        hash: "a".into(),
        regla: TipoRegla::Ratio,
        creado_en: 0,
        vence_en: AVISO_S,
    };
    // Una hora caída: no vence aunque ya toque.
    let p = avanzar(
        std::slice::from_ref(&a),
        &[cand("a")],
        AVISO_S,
        3600,
        false,
        &N,
    );
    assert!(p.vencidos.is_empty());
    let movido = Aviso {
        vence_en: AVISO_S + 3600,
        ..a.clone()
    };
    assert_eq!(p.vigentes, vec![movido.clone()]);
    // Vuelve la red: vence cuando suma 24 h efectivas.
    let p = avanzar(
        std::slice::from_ref(&movido),
        &[cand("a")],
        AVISO_S + 3599,
        0,
        true,
        &N,
    );
    assert!(p.vencidos.is_empty());
    let p = avanzar(
        std::slice::from_ref(&movido),
        &[cand("a")],
        AVISO_S + 3600,
        0,
        true,
        &N,
    );
    assert_eq!(p.vencidos, vec![movido]);
}

#[test]
fn con_la_vpn_caida_no_vence_aunque_el_plazo_ya_haya_pasado() {
    let a = Aviso {
        hash: "a".into(),
        regla: TipoRegla::Ratio,
        creado_en: 0,
        vence_en: AVISO_S,
    };
    let p = avanzar(
        std::slice::from_ref(&a),
        &[cand("a")],
        2 * AVISO_S,
        0,
        false,
        &N,
    );
    assert!(p.vencidos.is_empty());
    assert_eq!(p.vigentes, vec![a]);
}
