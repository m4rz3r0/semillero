//! Normas por tracker: cada torrent sigue las de su tracker (o las de por
//! defecto), un tracker caído no las cambia, el passkey del anuncio no sale
//! nunca, la página de reglas las enseña y las cambia, los presets las
//! guardan y una base de datos de la 0.5 abre igual.

use std::collections::BTreeMap;
use std::path::Path;

use axum::http::StatusCode;
use reglas::preset::Preset;
use reglas::{Disco, NormasTracker};
use semillero::db::{self, Evento};
use semillero::estado_red::EstadoRed;
use semillero::tarea;
use serde_json::{Value, json};

use crate::comun::{self, Prueba};
use crate::interfaz::{Ui, ui};

const T0: i64 = 1_800_000_000;
const DIA: i64 = 86_400;
const H1: &str = "1111111111111111111111111111111111111111";
const H2: &str = "2222222222222222222222222222222222222222";
const PASSKEY: &str = "p4ssk3yDeMentira0001";
const RED_OK: EstadoRed = EstadoRed {
    vigilada: true,
    vpn: true,
    puerto: Some(51413),
};

fn anuncio(host: &str) -> String {
    format!("https://{host}:443/{PASSKEY}/announce")
}

fn torrent(hash: &str, dias: i64, ratio: f64, tracker: &str) -> Value {
    json!({
        "hash": hash, "name": format!("Película {}", &hash[..1]), "category": "peliculas",
        "state": "uploading", "downloaded": 1000, "total_size": 1000, "uploaded": 1500,
        "seeding_time": dias * DIA, "ratio": ratio, "added_on": T0 - 10 * DIA,
        "save_path": "/media/peliculas", "upspeed": 0, "dlspeed": 0, "progress": 1.0,
        "tracker": tracker
    })
}

/// Normas de un tracker exigente: 20 días de siembra, ratio 5 para parar y
/// 48 horas de aviso.
fn exigentes() -> NormasTracker {
    NormasTracker {
        siembra_dias: 20,
        plazo_largo_dias: 40,
        ratio_pausa: 5.0,
        aviso_horas: 48,
        ..NormasTracker::default()
    }
}

fn sin_discos(_: &Path) -> Option<Disco> {
    None
}

struct Nada;
impl semillero::mediateca::Mediateca for Nada {
    async fn olvidar(&self, _t: &qbt::TorrentQbt) -> Result<(), String> {
        Ok(())
    }
}

async fn prueba(torrents: Vec<Value>) -> Prueba {
    let p = comun::con_sim(qbt_sim::Conf {
        lista_blanca: vec![[127, 0, 0, 1].into()],
        ..Default::default()
    })
    .await;
    p.sim().control.fijar_torrents(torrents);
    p
}

async fn detalles(p: &Prueba, hash: &str) -> Vec<(Evento, String)> {
    db::historial(&p.state.db, 100)
        .await
        .unwrap()
        .into_iter()
        .filter(|a| a.hash == hash)
        .map(|a| (a.evento, a.detalle))
        .collect()
}

#[tokio::test]
async fn cada_torrent_sigue_las_normas_de_su_tracker() {
    // Los dos con 10 días y ratio 3: con las de por defecto se paran.
    let p = prueba(vec![
        torrent(H1, 10, 3.0, &anuncio("tracker.ejemplo.example")),
        torrent(H2, 10, 3.0, &anuncio("malejemplo.example")),
    ])
    .await;
    db::guardar_normas_tracker(&p.state.db, "ejemplo.example", &exigentes())
        .await
        .unwrap();
    db::guardar_reglas(
        &p.state.db,
        "peliculas",
        &reglas::ReglasCategoria {
            ratio: Some(1.2),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let inf = tarea::pasada(&p.state, &Nada, &sin_discos, T0, &RED_OK, 0).await;
    // H1 (subdominio de ejemplo.example) aún no llega a 20 días: ni se para ni
    // se avisa. H2 no es de ejemplo.example: las de por defecto.
    assert_eq!(inf.pausados, vec![H2.to_string()], "{inf:?}");
    assert_eq!(inf.avisados, vec![H2.to_string()], "{inf:?}");
    let d = detalles(&p, H2).await;
    assert!(
        d.contains(&(
            Evento::Pausado,
            "Parado: 7 días sembrando y ratio 2.0 o más".into()
        )),
        "{d:?}"
    );

    // Con 25 días, H1 se para a su ratio (5) y su aviso vence a las 48 horas.
    p.sim().control.fijar_torrents(vec![
        torrent(H1, 25, 6.0, &anuncio("tracker.ejemplo.example")),
        torrent(H2, 10, 3.0, &anuncio("malejemplo.example")),
    ]);
    let inf = tarea::pasada(&p.state, &Nada, &sin_discos, T0 + 3600, &RED_OK, 0).await;
    assert!(inf.pausados.contains(&H1.to_string()), "{inf:?}");
    let d = detalles(&p, H1).await;
    assert!(
        d.contains(&(
            Evento::Pausado,
            "Parado: 20 días sembrando y ratio 5.0 o más".into()
        )),
        "{d:?}"
    );
    assert!(
        d.iter()
            .any(|(e, t)| *e == Evento::Aviso && t.starts_with("Se borrará en 48 horas: ")),
        "{d:?}"
    );
    let avisos = db::avisos(&p.state.db).await.unwrap();
    let a1 = avisos.iter().find(|a| a.hash == H1).unwrap();
    assert_eq!(a1.vence_en, T0 + 3600 + 2 * DIA);
}

#[tokio::test]
async fn con_el_tracker_caido_sigue_con_las_normas_de_su_ultimo_host() {
    let p = prueba(vec![torrent(
        H1,
        10,
        3.0,
        &anuncio("tracker.ejemplo.example"),
    )])
    .await;
    db::guardar_normas_tracker(&p.state.db, "ejemplo.example", &exigentes())
        .await
        .unwrap();
    let inf = tarea::pasada(&p.state, &Nada, &sin_discos, T0, &RED_OK, 0).await;
    assert!(inf.pausados.is_empty(), "{inf:?}");
    assert_eq!(
        db::trackers_vistos(&p.state.db).await.unwrap()[H1],
        "tracker.ejemplo.example"
    );
    // qBittorrent ya no tiene tracker que conteste: campo vacío.
    p.sim()
        .control
        .fijar_torrents(vec![torrent(H1, 10, 3.0, "")]);
    let inf = tarea::pasada(&p.state, &Nada, &sin_discos, T0 + 3600, &RED_OK, 0).await;
    assert!(inf.pausados.is_empty(), "{inf:?}");
    // A mano tampoco: sigue recién sembrado para su tracker.
    let r = tarea::quitar_a_mano(&p.state, &Nada, H1, false, T0 + 3600).await;
    assert!(matches!(r, Err(tarea::NoQuitado::Protegido(_))), "{r:?}");
    // Sin el último host recordado, las de por defecto.
    let otro = prueba(vec![torrent(H1, 10, 3.0, "")]).await;
    db::guardar_normas_tracker(&otro.state.db, "ejemplo.example", &exigentes())
        .await
        .unwrap();
    let inf = tarea::pasada(&otro.state, &Nada, &sin_discos, T0, &RED_OK, 0).await;
    assert_eq!(inf.pausados, vec![H1.to_string()]);
}

#[tokio::test]
async fn el_host_recordado_se_olvida_cuando_el_torrent_lleva_dias_fuera() {
    let p = prueba(vec![
        torrent(H1, 1, 0.1, &anuncio("ejemplo.example")),
        torrent(H2, 1, 0.1, &anuncio("segundo.example")),
    ])
    .await;
    tarea::pasada(&p.state, &Nada, &sin_discos, T0, &RED_OK, 0).await;
    assert_eq!(db::trackers_vistos(&p.state.db).await.unwrap().len(), 2);
    // H2 desaparece: se recuerda unos días (una lista a medias no lo borra).
    p.sim()
        .control
        .fijar_torrents(vec![torrent(H1, 1, 0.1, &anuncio("ejemplo.example"))]);
    tarea::pasada(&p.state, &Nada, &sin_discos, T0 + DIA, &RED_OK, 0).await;
    assert_eq!(db::trackers_vistos(&p.state.db).await.unwrap().len(), 2);
    tarea::pasada(&p.state, &Nada, &sin_discos, T0 + 8 * DIA, &RED_OK, 0).await;
    let vistos = db::trackers_vistos(&p.state.db).await.unwrap();
    assert_eq!(vistos.len(), 1, "{vistos:?}");
    assert_eq!(vistos[H1], "ejemplo.example");
}

#[tokio::test]
async fn sin_normas_por_tracker_todo_sigue_las_de_por_defecto() {
    let p = prueba(vec![torrent(
        H1,
        10,
        3.0,
        &anuncio("tracker.ejemplo.example"),
    )])
    .await;
    let inf = tarea::pasada(&p.state, &Nada, &sin_discos, T0, &RED_OK, 0).await;
    assert_eq!(inf.pausados, vec![H1.to_string()]);
    assert!(db::normas_trackers(&p.state.db).await.unwrap().is_empty());
}

#[tokio::test]
async fn las_normas_de_un_tracker_se_validan_como_las_de_por_defecto() {
    let p = prueba(vec![]).await;
    let malas = NormasTracker {
        plazo_largo_dias: 3,
        ..NormasTracker::default()
    };
    assert!(
        db::guardar_normas_tracker(&p.state.db, "ejemplo.example", &malas)
            .await
            .is_err()
    );
    for host in [
        "https://ejemplo.example",
        "EJEMPLO.example",
        "",
        "ejemplo.example/x",
    ] {
        assert!(
            db::guardar_normas_tracker(&p.state.db, host, &exigentes())
                .await
                .is_err(),
            "{host:?}"
        );
    }
    // Ni saltándose el código: la tabla tiene las mismas comprobaciones.
    let r = sqlx::query(
        "INSERT INTO normas_tracker
           (host, umbral_pct, siembra_dias, ratio_min, plazo_largo_dias, ratio_pausa, aviso_horas)
         VALUES ('ejemplo.example', 10, 7, 1.0, 30, 0.5, 24)",
    )
    .execute(&p.state.db)
    .await;
    assert!(r.is_err());
    assert!(db::normas_trackers(&p.state.db).await.unwrap().is_empty());
}

// ---------------------------------------------------------------- passkey

/// Todo el texto de la base de datos.
async fn volcado(db: &sqlx::SqlitePool) -> String {
    let tablas: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table'")
            .fetch_all(db)
            .await
            .unwrap();
    let mut todo = String::new();
    for t in tablas {
        let filas: Vec<sqlx::sqlite::SqliteRow> = sqlx::query(&format!("SELECT * FROM \"{t}\""))
            .fetch_all(db)
            .await
            .unwrap();
        use sqlx::{Column, Row, ValueRef};
        for f in filas {
            for c in f.columns() {
                let v = f.try_get_raw(c.ordinal()).unwrap();
                if !v.is_null()
                    && let Ok(s) = f.try_get::<String, _>(c.ordinal())
                {
                    todo.push_str(&s);
                    todo.push('\n');
                }
            }
        }
    }
    todo
}

async fn ui_con(torrents: Vec<Value>) -> Ui {
    let u = ui().await;
    u.p.sim().control.fijar_torrents(torrents);
    u
}

#[tokio::test]
async fn el_passkey_del_anuncio_no_sale_nunca() {
    let u = ui_con(vec![
        torrent(H1, 10, 3.0, &anuncio("tracker.ejemplo.example")),
        torrent(H2, 2, 0.1, &anuncio("127.0.0.1")),
    ])
    .await;
    db::guardar_normas_tracker(&u.p.state.db, "ejemplo.example", &exigentes())
        .await
        .unwrap();
    tarea::pasada(&u.p.state, &Nada, &sin_discos, T0, &RED_OK, 0).await;
    let lista = u.p.state.qbt.torrents().await.unwrap();
    assert!(!format!("{lista:?}").contains(PASSKEY));
    assert_eq!(lista[0].tracker, "tracker.ejemplo.example");
    let mut visto = volcado(&u.p.state.db).await;
    for ruta in ["/", "/reglas", "/historial"] {
        visto.push_str(&u.pagina(ruta).await);
    }
    for api in ["/api/portada", "/api/reglas", "/api/historial"] {
        visto.push_str(&u.llamar(api, json!({})).await.1);
    }
    assert!(!visto.contains(PASSKEY));
    assert!(!visto.contains("announce"));
    assert!(visto.contains("tracker.ejemplo.example"));
}

// ---------------------------------------------------------------- páginas

#[tokio::test]
async fn la_portada_dice_el_tracker_y_usa_sus_normas() {
    let u = ui_con(vec![
        torrent(H1, 10, 3.0, &anuncio("tracker.ejemplo.example")),
        torrent(H2, 10, 3.0, ""),
    ])
    .await;
    db::guardar_normas_tracker(&u.p.state.db, "ejemplo.example", &exigentes())
        .await
        .unwrap();
    let (s, j) = u.llamar("/api/portada", json!({})).await;
    assert_eq!(s, StatusCode::OK);
    let d: semillero::api::DatosPortada = serde_json::from_str(&j).unwrap();
    let t1 = d.torrents.iter().find(|t| t.hash == H1).unwrap();
    assert_eq!(t1.tracker, "tracker.ejemplo.example");
    assert_eq!(t1.normas, exigentes());
    // 10 de 20 días: recién sembrado para su tracker.
    assert_eq!(t1.estado, reglas::Estado::Obligado);
    let t2 = d.torrents.iter().find(|t| t.hash == H2).unwrap();
    assert_eq!(t2.tracker, "");
    assert_eq!(t2.estado, reglas::Estado::Cumplido);
    let ficha = semillero::ui::portada::ficha_en_html(&d, H1).unwrap();
    assert!(ficha.contains("tracker.ejemplo.example"), "{ficha}");
    assert!(ficha.contains("Le faltan 10 días"), "{ficha}");
    assert!(u.pagina("/").await.contains("tracker.ejemplo.example"));
}

#[tokio::test]
async fn la_pagina_de_reglas_lista_los_trackers_y_cambia_sus_normas() {
    let u = ui_con(vec![
        torrent(H1, 10, 3.0, &anuncio("tracker.ejemplo.example")),
        torrent(H2, 10, 3.0, &anuncio("tracker.ejemplo.example")),
    ])
    .await;
    db::guardar_normas_tracker(&u.p.state.db, "segundo.example", &exigentes())
        .await
        .unwrap();
    let reglas_json = |c: &str| -> Value { serde_json::from_str::<Value>(c).unwrap() };
    let (_, c) = u.llamar("/api/reglas", json!({})).await;
    let v = reglas_json(&c);
    let trackers = v["trackers"].as_array().unwrap();
    assert_eq!(trackers.len(), 2, "{trackers:?}");
    let visto = trackers
        .iter()
        .find(|t| t["host"] == "tracker.ejemplo.example")
        .unwrap();
    assert_eq!(visto["torrents"], 2);
    assert!(visto["propias"].is_null());
    let configurado = trackers
        .iter()
        .find(|t| t["host"] == "segundo.example")
        .unwrap();
    assert_eq!(configurado["torrents"], 0);
    let html = u.pagina("/reglas").await;
    assert!(html.contains("tracker.ejemplo.example"));
    assert!(html.contains("Usar normas propias"), "{html}");

    // «Usar normas propias» y guardar.
    let (s, _) = u
        .llamar(
            "/api/guardar_normas_tracker",
            json!({ "tracker": "tracker.ejemplo.example", "normas": exigentes() }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        db::normas_trackers(&u.p.state.db).await.unwrap()["tracker.ejemplo.example"],
        exigentes()
    );
    // Las mismas comprobaciones y frases que las de por defecto.
    let (s, c) = u
        .llamar(
            "/api/guardar_normas_tracker",
            json!({ "tracker": "tracker.ejemplo.example",
                    "normas": NormasTracker { ratio_pausa: 0.5, ..NormasTracker::default() } }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert!(c.contains("La ratio para parar no puede ser menor"), "{c}");
    let (s, _) = u
        .llamar(
            "/api/guardar_normas_tracker",
            json!({ "tracker": "https://ejemplo.example/announce", "normas": exigentes() }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // Quitar la marca: vuelve a las de por defecto.
    let (s, _) = u
        .llamar(
            "/api/quitar_normas_tracker",
            json!({ "tracker": "tracker.ejemplo.example" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let quedan = db::normas_trackers(&u.p.state.db).await.unwrap();
    assert_eq!(quedan.keys().collect::<Vec<_>>(), ["segundo.example"]);
    // Las normas por defecto no se tocaron.
    assert_eq!(
        db::normas(&u.p.state.db).await.unwrap(),
        NormasTracker::default()
    );
}

#[tokio::test]
async fn cambiar_las_normas_de_un_tracker_exige_sesion_y_token() {
    let u = ui().await;
    for (uri, cuerpo) in [
        (
            "/api/guardar_normas_tracker",
            json!({ "tracker": "ejemplo.example", "normas": exigentes() }),
        ),
        (
            "/api/quitar_normas_tracker",
            json!({ "tracker": "ejemplo.example" }),
        ),
    ] {
        let r = comun::enviar(
            &u.p.app,
            crate::interfaz::api(uri, cuerpo.clone(), None, None),
        )
        .await;
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "{uri}");
        let r = comun::enviar(
            &u.p.app,
            crate::interfaz::api(uri, cuerpo, Some(&u.cookie), None),
        )
        .await;
        assert_eq!(r.status(), StatusCode::FORBIDDEN, "{uri}");
    }
    assert!(db::normas_trackers(&u.p.state.db).await.unwrap().is_empty());
}

#[tokio::test]
async fn con_borrado_real_cambiar_las_normas_de_un_tracker_pide_la_contrasena() {
    let u = ui().await;
    db::fijar_modo(&u.p.state.db, db::Modo::BorradoReal)
        .await
        .unwrap();
    // La ventana de 5 minutos se cierra.
    sqlx::query("UPDATE sesiones SET reautenticado_hasta = NULL")
        .execute(&u.p.state.db)
        .await
        .unwrap();
    let (s, c) = u
        .llamar(
            "/api/guardar_normas_tracker",
            json!({ "tracker": "ejemplo.example", "normas": exigentes() }),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(c.contains(semillero::api::ERROR_REAUTH), "{c}");
    assert!(db::normas_trackers(&u.p.state.db).await.unwrap().is_empty());
    let (s, _) = u
        .llamar(
            "/api/guardar_normas_tracker",
            json!({ "tracker": "ejemplo.example", "normas": exigentes(), "clave": comun::CLAVE }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
}

// ---------------------------------------------------------------- presets

#[tokio::test]
async fn un_preset_guarda_y_aplica_las_normas_de_cada_tracker() {
    let u = ui().await;
    db::guardar_normas_tracker(&u.p.state.db, "ejemplo.example", &exigentes())
        .await
        .unwrap();
    let (s, _) = u
        .llamar(
            "/api/guardar_preset",
            json!({ "nombre": "Con tracker", "descripcion": "" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (_, c) = u.llamar("/api/reglas", json!({})).await;
    let v: Value = serde_json::from_str(&c).unwrap();
    let ficha = v["presets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["nombre"] == "Con tracker")
        .unwrap()
        .clone();
    assert_eq!(ficha["trackers"], 1);
    let id = ficha["id"].as_i64().unwrap();
    let guardado = db::preset(&u.p.state.db, id).await.unwrap().unwrap();
    assert_eq!(
        guardado.trackers,
        BTreeMap::from([("ejemplo.example".to_string(), exigentes())])
    );
    assert!(guardado.escribir().starts_with("formato = 2\n"));

    // El incluido no trae normas por tracker: aplicarlo las quita.
    let (s, _) = u.llamar("/api/aplicar_preset", json!({ "id": 0 })).await;
    assert_eq!(s, StatusCode::OK);
    assert!(db::normas_trackers(&u.p.state.db).await.unwrap().is_empty());
    // Y el guardado las vuelve a poner.
    let (s, _) = u.llamar("/api/aplicar_preset", json!({ "id": id })).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        db::normas_trackers(&u.p.state.db).await.unwrap(),
        guardado.trackers
    );
}

#[tokio::test]
async fn un_preset_sin_normas_por_tracker_se_guarda_en_el_formato_1() {
    let u = ui().await;
    let (s, _) = u
        .llamar(
            "/api/guardar_preset",
            json!({ "nombre": "Sin trackers", "descripcion": "" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let archivo: String = sqlx::query_scalar("SELECT archivo FROM presets")
        .fetch_one(&u.p.state.db)
        .await
        .unwrap();
    assert!(archivo.starts_with("formato = 1\n"), "{archivo}");
    assert!(!archivo.contains("[trackers"), "{archivo}");
}

// ---------------------------------------------------------------- 0.5

/// Un preset de la 0.5 (formato 1), tal como lo guardaba.
const PRESET_05: &str = r#"formato = 1
nombre = "De la 0.5"
descripcion = ""

[normas]
umbral_pct = 20.0
siembra_dias = 3
ratio_min = 0.5
plazo_largo_dias = 12
ratio_pausa = 4.0
aviso_horas = 6

[categorias.series]
ratio = 3.0
"#;

/// Una base de datos como la deja la 0.5: migraciones 0001 a 0004, con
/// normas cambiadas, reglas, un aviso y un preset guardado.
async fn base_de_la_0_5(ruta: &Path) {
    let opciones = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(ruta)
        .create_if_missing(true);
    let pool = sqlx::SqlitePool::connect_with(opciones).await.unwrap();
    let mut m = sqlx::migrate!("./migrations");
    m.migrations = m
        .migrations
        .iter()
        .filter(|x| x.version <= 4)
        .cloned()
        .collect::<Vec<_>>()
        .into();
    m.run(&pool).await.unwrap();
    sqlx::query(
        "UPDATE normas SET umbral_pct = 5, siembra_dias = 2, ratio_min = 0.8,
           plazo_largo_dias = 9, ratio_pausa = 1.5, aviso_horas = 12 WHERE id = 1",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO reglas (categoria, ratio) VALUES ('peliculas', 1.2)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO presets (nombre, descripcion, archivo, cambiado) VALUES (?, '', ?, 1)",
    )
    .bind("De la 0.5")
    .bind(PRESET_05)
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;
}

#[tokio::test]
async fn una_base_de_datos_de_la_0_5_abre_igual_y_su_preset_sigue_valiendo() {
    let dir = tempfile::tempdir().unwrap();
    let ruta = dir.path().join("de-la-0.5.db");
    base_de_la_0_5(&ruta).await;
    let ruta2 = ruta.clone();
    let p = comun::con_sim_y(
        qbt_sim::Conf {
            lista_blanca: vec![[127, 0, 0, 1].into()],
            ..Default::default()
        },
        move |c| c.db = ruta2,
    )
    .await;
    let db = &p.state.db;
    let antes = NormasTracker {
        umbral_pct: 5.0,
        siembra_dias: 2,
        ratio_min: 0.8,
        plazo_largo_dias: 9,
        ratio_pausa: 1.5,
        aviso_horas: 12,
    };
    assert_eq!(db::normas(db).await.unwrap(), antes);
    let juego = db::juego_normas(db).await.unwrap();
    assert_eq!(juego.defecto, antes);
    assert!(juego.trackers.is_empty());
    assert_eq!(db::reglas(db).await.unwrap()["peliculas"].ratio, Some(1.2));
    // Su tarea se porta igual: 3 días y ratio 2 se para con las suyas.
    p.sim()
        .control
        .fijar_torrents(vec![torrent(H1, 3, 2.0, &anuncio("ejemplo.example"))]);
    let inf = tarea::pasada(&p.state, &Nada, &sin_discos, T0, &RED_OK, 0).await;
    assert_eq!(inf.pausados, vec![H1.to_string()]);
    // El preset guardado se lee como siempre y aplicarlo pone sus normas
    // como las de por defecto.
    let filas = db::presets(db).await.unwrap();
    assert_eq!(filas.len(), 1);
    let pre = db::preset(db, filas[0].id).await.unwrap().unwrap();
    assert_eq!(pre, Preset::leer(PRESET_05).unwrap());
    assert!(pre.trackers.is_empty());
    db::aplicar_preset(db, &pre, T0).await.unwrap();
    assert_eq!(db::normas(db).await.unwrap(), pre.normas);
    assert!(db::normas_trackers(db).await.unwrap().is_empty());
    // Y se descarga igual que en la 0.5.
    assert_eq!(pre.escribir(), PRESET_05);
}
