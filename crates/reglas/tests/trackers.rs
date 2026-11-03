//! Normas por tracker: qué host vale para cada torrent y cómo cambian la
//! pausa, los candidatos y los avisos.

use std::collections::{BTreeMap, HashMap, HashSet};

use reglas::trackers::{JuegoNormas, encaja, normalizar_host};
use reglas::{
    Aviso, Candidato, NormasTracker, ReglasCategoria, TipoRegla, Torrent, a_pausar, a_pausar_con,
    avanzar, avanzar_con, candidatos, candidatos_con,
};

const DIA: i64 = 86_400;

fn exigente() -> NormasTracker {
    NormasTracker {
        siembra_dias: 20,
        ratio_pausa: 5.0,
        aviso_horas: 48,
        ..NormasTracker::default()
    }
}

fn juego() -> JuegoNormas {
    JuegoNormas {
        defecto: NormasTracker::default(),
        trackers: BTreeMap::from([
            ("ejemplo.example".to_string(), exigente()),
            (
                "tracker.ejemplo.example".to_string(),
                NormasTracker {
                    siembra_dias: 2,
                    ..NormasTracker::default()
                },
            ),
            ("127.0.0.1".to_string(), exigente()),
        ]),
    }
}

#[test]
fn el_host_se_normaliza_o_se_rechaza() {
    assert_eq!(
        normalizar_host("Tracker.Ejemplo.EXAMPLE"),
        Some("tracker.ejemplo.example".into())
    );
    assert_eq!(
        normalizar_host(" ejemplo.example. "),
        Some("ejemplo.example".into())
    );
    assert_eq!(normalizar_host("127.0.0.1"), Some("127.0.0.1".into()));
    assert_eq!(normalizar_host("[2001:DB8::1]"), Some("2001:db8::1".into()));
    for malo in [
        "",
        "https://ejemplo.example",
        "ejemplo.example:443",
        "ejemplo.example/announce",
        "usuario@ejemplo.example",
        "-ejemplo.example",
        "ejemplo..org",
        "ejem plo.example",
        "ejémplo.example",
        "[ejemplo.example]",
    ] {
        assert_eq!(normalizar_host(malo), None, "{malo:?}");
    }
    assert_eq!(normalizar_host(&"a".repeat(64)), None);
}

#[test]
fn un_host_encaja_consigo_y_con_sus_dominios_padre() {
    assert!(encaja("ejemplo.example", "ejemplo.example"));
    assert!(encaja("tracker.ejemplo.example", "ejemplo.example"));
    assert!(encaja("a.b.ejemplo.example", "ejemplo.example"));
    // Solo en el límite de una etiqueta.
    assert!(!encaja("malejemplo.example", "ejemplo.example"));
    assert!(!encaja("ejemplo.example", "tracker.ejemplo.example"));
    assert!(!encaja("", "ejemplo.example"));
    // Una IP, solo ella misma.
    assert!(encaja("127.0.0.1", "127.0.0.1"));
    assert!(!encaja("192.0.2.1", "0.2.1"));
}

#[test]
fn vale_el_host_configurado_mas_concreto_y_si_no_el_de_por_defecto() {
    let j = juego();
    assert_eq!(
        j.tracker_de("tracker.ejemplo.example"),
        Some("tracker.ejemplo.example")
    );
    assert_eq!(j.para("tracker.ejemplo.example").siembra_dias, 2);
    assert_eq!(
        j.tracker_de("otro.tracker.ejemplo.example"),
        Some("tracker.ejemplo.example")
    );
    assert_eq!(j.tracker_de("www.ejemplo.example"), Some("ejemplo.example"));
    assert_eq!(*j.para("www.ejemplo.example"), exigente());
    assert_eq!(j.tracker_de("malejemplo.example"), None);
    assert_eq!(*j.para("malejemplo.example"), NormasTracker::default());
    assert_eq!(*j.para(""), NormasTracker::default());
    assert_eq!(*j.para("segundo.example"), NormasTracker::default());
    assert_eq!(*j.para("127.0.0.1"), exigente());
    assert_eq!(
        *JuegoNormas::default().para("ejemplo.example"),
        NormasTracker::default()
    );
}

fn t(hash: &str, siembra_dias: i64, ratio: f64) -> Torrent {
    Torrent {
        hash: hash.into(),
        categoria: "peliculas".into(),
        estado_qbt: "uploading".into(),
        descargado: 1000,
        tamano: 1000,
        siembra_s: siembra_dias * DIA,
        ratio,
        anadido_en: 0,
        ultima_subida_en: None,
    }
}

#[test]
fn cada_torrent_se_pausa_con_sus_normas() {
    let ts = [t("a", 10, 3.0), t("b", 10, 3.0)];
    let por_hash = HashMap::from([("b", exigente())]);
    let defecto = NormasTracker::default();
    let normas = |x: &Torrent| por_hash.get(x.hash.as_str()).unwrap_or(&defecto);
    assert_eq!(a_pausar_con(&ts, normas), vec!["a".to_string()]);
    // Con las mismas para todos, como siempre.
    assert_eq!(a_pausar_con(&ts, |_| &defecto), a_pausar(&ts, &defecto));
}

#[test]
fn cada_torrent_cumple_o_no_con_sus_normas() {
    let ts = [t("a", 10, 1.5), t("b", 10, 1.5)];
    let reglas = HashMap::from([(
        "peliculas".to_string(),
        ReglasCategoria {
            ratio: Some(1.2),
            ..Default::default()
        },
    )]);
    let por_hash = HashMap::from([("b", exigente())]);
    let defecto = NormasTracker::default();
    let normas = |x: &Torrent| por_hash.get(x.hash.as_str()).unwrap_or(&defecto);
    let vacio = HashMap::new();
    let ex = HashSet::new();
    let c = candidatos_con(&ts, normas, &reglas, &vacio, &ex, 0);
    assert_eq!(
        c,
        vec![Candidato {
            hash: "a".into(),
            regla: TipoRegla::Ratio
        }]
    );
    assert_eq!(
        candidatos_con(&ts, |_| &defecto, &reglas, &vacio, &ex, 0),
        candidatos(&ts, &defecto, &reglas, &vacio, &ex, 0)
    );
}

#[test]
fn cada_aviso_nuevo_vence_a_las_horas_de_sus_normas() {
    let cands = [
        Candidato {
            hash: "a".into(),
            regla: TipoRegla::Ratio,
        },
        Candidato {
            hash: "b".into(),
            regla: TipoRegla::Ratio,
        },
    ];
    let defecto = NormasTracker::default();
    let e = exigente();
    let normas = |h: &str| if h == "b" { &e } else { &defecto };
    let paso = avanzar_con(&[], &cands, 100, 0, true, normas);
    let vence: Vec<(String, i64)> = paso
        .nuevos
        .iter()
        .map(|a: &Aviso| (a.hash.clone(), a.vence_en))
        .collect();
    assert_eq!(
        vence,
        vec![("a".into(), 100 + 24 * 3600), ("b".into(), 100 + 48 * 3600)]
    );
    assert_eq!(
        avanzar_con(&[], &cands, 100, 0, true, |_| &defecto),
        avanzar(&[], &cands, 100, 0, true, &defecto)
    );
}
