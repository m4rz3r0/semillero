//! Propiedades de las reglas, con proptest: torrents y normas al azar (con
//! los bordes: cero, máximos, justo por debajo y por encima de cada umbral),
//! contrastados con un oráculo escrito aquí, aparte del código del crate.
//!
//! - Ningún torrent que no haya cumplido sale nunca como «Listo para
//!   recoger» (`Estado::Cumplido`), ni es borrable, ni candidato a borrar.
//! - Todo torrent que obliga y ha cumplido sale como «Listo para recoger».
//! - Con normas por tracker, cada torrent se juzga con las de su tracker (el
//!   host igual o un subdominio, el más concreto) o con las de por defecto.
//! - Nada vence (se recoge) antes de sus horas de aviso.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::net::IpAddr;

use proptest::prelude::*;
use proptest::test_runner::Config;
use reglas::trackers::JuegoNormas;
use reglas::{
    Aviso, Candidato, Disco, Estado, NormasTracker, ReglasCategoria, TipoRegla, Torrent,
    avanzar_con, candidatos_con, clasificar,
};

const DIA: i64 = 86_400;
const HORA: i64 = 3600;
const CATEGORIAS: [&str; 3] = ["", "series", "pelis"];
const ESTADOS_QBT: [&str; 5] = [
    "uploading",
    "stalledUP",
    "stoppedUP",
    "stoppedDL",
    "queuedUP",
];

/// 256 casos por propiedad (lo de proptest) y sin guardar los fallos en
/// ficheros junto a las pruebas.
fn config() -> Config {
    Config {
        failure_persistence: None,
        ..Config::default()
    }
}

// --- El oráculo: qué es haber cumplido, escrito a mano. ---

/// El torrent obliga a sembrar: se ha bajado al menos el porcentaje de las
/// normas. Sin tamaño (magnet sin metadatos), obliga si se bajó algo.
fn obliga(t: &Torrent, n: &NormasTracker) -> bool {
    if t.tamano <= 0 {
        t.descargado > 0
    } else {
        t.descargado as f64 >= n.umbral_pct / 100.0 * t.tamano as f64
    }
}

/// Obliga y ya ha cumplido: los días de siembra y, además, la ratio mínima
/// o el plazo largo sembrando.
fn ha_cumplido(t: &Torrent, n: &NormasTracker) -> bool {
    let dias_siembra = i64::from(n.siembra_dias) * DIA;
    let plazo_largo = i64::from(n.plazo_largo_dias) * DIA;
    obliga(t, n)
        && t.siembra_s >= dias_siembra
        && (t.ratio >= n.ratio_min || t.siembra_s >= plazo_largo)
}

fn es_ip(h: &str) -> bool {
    h.parse::<IpAddr>().is_ok()
}

/// Las normas que valen para `host`: las del host configurado más largo
/// que es el mismo o del que `host` es subdominio (una IP, solo ella
/// misma); si no hay ninguno, las de por defecto.
fn normas_de<'a>(juego: &'a JuegoNormas, host: &str) -> &'a NormasTracker {
    let mut mejor: Option<(&String, &NormasTracker)> = None;
    for (configurado, normas) in &juego.trackers {
        let vale = !host.is_empty()
            && !configurado.is_empty()
            && (host == configurado
                || (!es_ip(host)
                    && !es_ip(configurado)
                    && host.ends_with(&format!(".{configurado}"))));
        if vale && mejor.is_none_or(|(m, _)| configurado.len() > m.len()) {
            mejor = Some((configurado, normas));
        }
    }
    mejor.map_or(&juego.defecto, |(_, n)| n)
}

// --- Estrategias. ---

fn dias() -> impl Strategy<Value = u32> {
    prop_oneof![
        Just(0),
        Just(1),
        Just(7),
        Just(NormasTracker::MAX_DIAS),
        Just(u32::MAX),
        0..=NormasTracker::MAX_DIAS + 5,
    ]
}

fn ratio_de_norma() -> impl Strategy<Value = f64> {
    prop_oneof![
        Just(0.0),
        Just(1.0),
        Just(NormasTracker::MAX_RATIO),
        0.0..=NormasTracker::MAX_RATIO,
    ]
}

/// Normas cualesquiera, también fuera de lo que admite `validar` (plazo
/// largo menor que los días de siembra, ratio de pausa menor que la
/// mínima…): `clasificar` no debe fiarse de que estén validadas.
fn normas() -> impl Strategy<Value = NormasTracker> {
    (
        prop_oneof![Just(0.0), Just(10.0), Just(100.0), 0.0..=100.0f64],
        dias(),
        ratio_de_norma(),
        dias(),
        ratio_de_norma(),
        prop_oneof![Just(1), Just(NormasTracker::MAX_AVISO_HORAS), 1..=72u32],
    )
        .prop_map(
            |(umbral_pct, siembra_dias, ratio_min, plazo_largo_dias, ratio_pausa, aviso_horas)| {
                NormasTracker {
                    umbral_pct,
                    siembra_dias,
                    ratio_min,
                    plazo_largo_dias,
                    ratio_pausa,
                    aviso_horas,
                }
            },
        )
}

/// El `f64` justo por debajo de `x` (para `x > 0`) o el mismo.
fn justo_debajo(x: f64) -> f64 {
    if x > 0.0 {
        f64::from_bits(x.to_bits() - 1)
    } else {
        x
    }
}

fn justo_encima(x: f64) -> f64 {
    f64::from_bits(x.to_bits() + 1)
}

/// Un torrent pensado para las normas `n`: lo descargado, lo sembrado y la
/// ratio caen a menudo justo en sus umbrales o a un paso de ellos.
fn torrent_para(n: NormasTracker) -> impl Strategy<Value = Torrent> {
    let tamano = prop_oneof![
        Just(-1i64),
        Just(0),
        Just(1),
        Just(100),
        Just(i64::MAX),
        1..=1i64 << 40,
    ];
    let siembra = n.siembra_s();
    let plazo = n.plazo_largo_s();
    let siembra_s = prop_oneof![
        Just(0i64),
        Just(siembra),
        Just(siembra.saturating_sub(1).max(0)),
        Just(siembra.saturating_add(1)),
        Just(plazo),
        Just(plazo.saturating_sub(1).max(0)),
        Just(plazo.saturating_add(1)),
        Just(i64::MAX),
        0..=400 * DIA,
    ];
    let ratio = prop_oneof![
        Just(0.0),
        Just(n.ratio_min),
        Just(justo_debajo(n.ratio_min)),
        Just(justo_encima(n.ratio_min)),
        Just(n.ratio_pausa),
        0.0..=200.0f64,
    ];
    (
        tamano,
        siembra_s,
        ratio,
        0..CATEGORIAS.len(),
        0..ESTADOS_QBT.len(),
        any::<u32>(),
        prop::option::of(0..=1_000 * DIA),
    )
        .prop_flat_map(move |(tamano, siembra_s, ratio, cat, est, hash, subida)| {
            // Lo descargado, alrededor de lo que hace que obligue.
            let umbral = (n.umbral_pct / 100.0 * tamano.max(0) as f64) as i64;
            let descargado = prop_oneof![
                Just(-1i64),
                Just(0),
                Just(1),
                Just(umbral.saturating_sub(1)),
                Just(umbral),
                Just(umbral.saturating_add(1)),
                Just(tamano),
                0..=tamano.max(1),
            ];
            descargado.prop_map(move |descargado| Torrent {
                hash: format!("{hash:08x}"),
                categoria: CATEGORIAS[cat].to_string(),
                estado_qbt: ESTADOS_QBT[est].to_string(),
                descargado,
                tamano,
                siembra_s,
                ratio,
                anadido_en: i64::from(hash % 1000) * DIA,
                ultima_subida_en: subida,
            })
        })
}

fn normas_y_torrent() -> impl Strategy<Value = (NormasTracker, Torrent)> {
    normas().prop_flat_map(|n| (Just(n), torrent_para(n)))
}

fn regla_de_categoria() -> impl Strategy<Value = ReglasCategoria> {
    (
        prop::option::of(prop_oneof![Just(0.1), Just(1.0), 0.1..=100.0f64]),
        prop::option::of(prop_oneof![Just(1u32), 1..=60u32]),
        prop::option::of(prop_oneof![Just(1u32), 1..=60u32]),
        prop::option::of(prop_oneof![Just(1.0), Just(90.0), 1.0..=90.0f64]),
    )
        .prop_map(
            |(ratio, siembra_dias, inactividad_dias, disco_libre_pct)| ReglasCategoria {
                ratio,
                siembra_dias,
                inactividad_dias,
                disco_libre_pct,
            },
        )
}

/// Hosts configurados posibles: un dominio, sus subdominios, uno que
/// solo comparte el final (`malejemplo`) y dos IP.
const CONFIGURABLES: [&str; 6] = [
    "ejemplo.example",
    "tracker.ejemplo.example",
    "a.tracker.ejemplo.example",
    "malejemplo.example",
    "192.0.2.1",
    "2001:db8::1",
];

fn juego() -> impl Strategy<Value = JuegoNormas> {
    (
        normas(),
        prop::collection::btree_map(
            prop::sample::select(&CONFIGURABLES[..]).prop_map(str::to_string),
            normas(),
            0..=CONFIGURABLES.len(),
        ),
    )
        .prop_map(|(defecto, trackers): (_, BTreeMap<String, _>)| JuegoNormas { defecto, trackers })
}

/// El host de un torrent: vacío, uno de los configurables con hasta dos
/// etiquetas delante, o uno que no se parece a ninguno.
fn host() -> impl Strategy<Value = String> {
    let prefijo = prop::collection::vec(
        prop::sample::select(vec!["a", "b", "tracker", "x-1"]),
        0..=2,
    )
    .prop_map(|v| v.iter().map(|e| format!("{e}.")).collect::<String>());
    prop_oneof![
        Just(String::new()),
        Just("otro.example".to_string()),
        Just("example".to_string()),
        (prefijo, prop::sample::select(&CONFIGURABLES[..])).prop_map(|(p, c)| format!("{p}{c}")),
    ]
}

proptest! {
    #![proptest_config(config())]

    /// P1, la principal: ningún torrent que no haya cumplido sale nunca
    /// como «Listo para recoger».
    #[test]
    fn ningun_torrent_que_no_haya_cumplido_sale_como_listo_para_recoger(
        (n, t) in normas_y_torrent()
    ) {
        let c = clasificar(&t, &n);
        if c.estado == Estado::Cumplido {
            prop_assert!(ha_cumplido(&t, &n), "listo sin haber cumplido: {c:?}");
        }
        // Y nada que obligue sin haber cumplido es borrable.
        if c.borrable {
            prop_assert!(!obliga(&t, &n) || ha_cumplido(&t, &n), "borrable sin cumplir: {c:?}");
        }
    }

    /// P1 en el camino del borrado: los candidatos de las reglas de
    /// categoría son solo torrents que no obligan o que ya han cumplido,
    /// cada uno según sus normas.
    #[test]
    fn ningun_candidato_a_recoger_obliga_sin_haber_cumplido(
        lista in prop::collection::vec(
            // Con tamaños de cerca de `i64::MAX` también: la suma del
            // espacio que liberan no puede desbordarse.
            normas_y_torrent(),
            0..8,
        ),
        reglas_cat in prop::collection::hash_map(
            prop::sample::select(&CATEGORIAS[..]).prop_map(str::to_string),
            regla_de_categoria(),
            0..=CATEGORIAS.len(),
        ),
        libre in 0..=1_000i64,
        ahora in 0..=2_000 * DIA,
    ) {
        // Hashes distintos, cada uno con sus normas.
        let torrents: Vec<Torrent> = lista
            .iter()
            .enumerate()
            .map(|(i, (_, t))| Torrent { hash: format!("t{i}"), ..t.clone() })
            .collect();
        let normas: HashMap<String, NormasTracker> = torrents
            .iter()
            .zip(&lista)
            .map(|(t, (n, _))| (t.hash.clone(), *n))
            .collect();
        let discos: HashMap<String, Disco> = CATEGORIAS
            .iter()
            .map(|c| (c.to_string(), Disco { libre, total: 1_000 }))
            .collect();
        let cands = candidatos_con(
            &torrents,
            |t| &normas[&t.hash],
            &reglas_cat,
            &discos,
            &HashSet::new(),
            ahora,
        );
        for c in &cands {
            let t = torrents.iter().find(|t| t.hash == c.hash).unwrap();
            let n = &normas[&t.hash];
            prop_assert!(
                !obliga(t, n) || ha_cumplido(t, n),
                "candidato sin cumplir: {c:?} {t:?} {n:?}"
            );
        }
    }

    /// P2: todo torrent que obliga y ha cumplido sale como «Listo para
    /// recoger» (y borrable); el que no obliga, «Libre».
    #[test]
    fn todo_torrent_que_ha_cumplido_sale_como_listo_para_recoger(
        (n, t) in normas_y_torrent()
    ) {
        let c = clasificar(&t, &n);
        if ha_cumplido(&t, &n) {
            prop_assert_eq!(c.estado, Estado::Cumplido);
            prop_assert!(c.borrable);
        }
        if !obliga(&t, &n) {
            prop_assert_eq!(c.estado, Estado::Libre);
        }
    }

    /// P3: con normas por tracker, cada torrent se clasifica como con las
    /// normas de su tracker (host igual o subdominio, el más concreto) o,
    /// si ninguno encaja, con las de por defecto.
    #[test]
    fn cada_torrent_se_juzga_con_las_normas_de_su_tracker(
        juego in juego(),
        host in host(),
        t in normas().prop_flat_map(torrent_para),
    ) {
        let esperadas = normas_de(&juego, &host);
        prop_assert_eq!(juego.para(&host), esperadas, "host {:?}", host);
        prop_assert_eq!(clasificar(&t, juego.para(&host)), clasificar(&t, esperadas));
    }

    /// P4: nada vence (se recoge) antes de sus horas de aviso, contadas
    /// desde que pasó a candidato y alargadas con lo que la red estuvo
    /// caída; con la red caída, nada vence.
    #[test]
    fn nada_se_recoge_antes_de_sus_horas_de_aviso(
        avisos_h in prop::collection::vec(
            prop_oneof![Just(1u32), Just(NormasTracker::MAX_AVISO_HORAS), 1..=72u32],
            4,
        ),
        pasos in prop::collection::vec(
            (
                prop_oneof![Just(0i64), Just(1), Just(HORA), Just(DIA), 0..=40 * DIA],
                any::<u8>(),
                prop_oneof![Just(-5i64), Just(0), Just(1), Just(HORA), 0..=DIA],
                prop::bool::weighted(0.8),
            ),
            1..30,
        ),
        inicio in 0..=1_000 * DIA,
    ) {
        let normas: HashMap<String, NormasTracker> = avisos_h
            .iter()
            .enumerate()
            .map(|(i, &h)| {
                (format!("t{i}"), NormasTracker { aviso_horas: h, ..NormasTracker::default() })
            })
            .collect();
        let mut avisos: Vec<Aviso> = Vec::new();
        // Hora de recogida esperada de cada aviso pendiente.
        let mut vence: HashMap<String, i64> = HashMap::new();
        let mut ahora = inicio;
        for (dt, mascara, caido_s, red_ok) in pasos {
            ahora += dt;
            let cands: Vec<Candidato> = (0..4)
                .filter(|i| mascara & (1 << i) != 0)
                .map(|i| Candidato { hash: format!("t{i}"), regla: TipoRegla::Ratio })
                .collect();
            for v in vence.values_mut() {
                *v = v.saturating_add(caido_s.max(0));
            }
            let paso = avanzar_con(&avisos, &cands, ahora, caido_s, red_ok, |h| &normas[h]);
            for a in &paso.vencidos {
                prop_assert!(red_ok, "vence con la red caída: {a:?}");
                prop_assert!(cands.iter().any(|c| c.hash == a.hash), "vence sin ser candidato: {a:?}");
                let esperada = vence[&a.hash];
                prop_assert!(ahora >= esperada, "vence antes de hora: {a:?}, esperada {esperada}");
                prop_assert!(
                    ahora >= a.creado_en + normas[&a.hash].aviso_s(),
                    "vence antes de sus horas de aviso: {a:?}"
                );
            }
            for a in paso.vencidos.iter().chain(&paso.retirados) {
                vence.remove(&a.hash);
            }
            for a in &paso.nuevos {
                prop_assert!(!paso.vencidos.iter().any(|v| v.hash == a.hash));
                vence.insert(a.hash.clone(), ahora + normas[&a.hash].aviso_s());
            }
            for a in &paso.vigentes {
                prop_assert_eq!(a.vence_en, vence[&a.hash], "{:?}", a);
            }
            avisos = paso.vigentes;
        }
    }
}
