//! Sondas del verificador independiente de T4: nada se borra cuando la
//! especificación lo prohíbe.

use std::path::Path;
use std::sync::Mutex;

use qbt::TorrentQbt;
use reglas::{Disco, ReglasCategoria, TipoRegla};
use semillero::db::{self, Modo};
use semillero::estado_red::EstadoRed;
use semillero::mediateca::Mediateca;
use semillero::tarea;
use serde_json::{Value, json};
use sqlx::SqlitePool;

use crate::comun::{self, Prueba};

const T0: i64 = 1_800_000_000;
const DIA: i64 = 86_400;
const H1: &str = "1111111111111111111111111111111111111111";
const H2: &str = "2222222222222222222222222222222222222222";
const H3: &str = "3333333333333333333333333333333333333333";
const H4: &str = "4444444444444444444444444444444444444444";
const H5: &str = "5555555555555555555555555555555555555555";
const RED_OK: EstadoRed = EstadoRed {
    vigilada: true,
    vpn: true,
    puerto: Some(51413),
};
const RED_CAIDA: EstadoRed = EstadoRed {
    vigilada: true,
    vpn: false,
    puerto: None,
};

fn cumplido(hash: &str, cat: &str) -> Value {
    json!({
        "hash": hash, "name": format!("T {}", &hash[..1]), "category": cat,
        "state": "stoppedUP", "downloaded": 1000, "total_size": 1000, "uploaded": 1500,
        "seeding_time": 40 * DIA, "ratio": 5.0, "added_on": T0 - 50 * DIA,
        "save_path": "/media/x", "upspeed": 0, "dlspeed": 0, "progress": 1.0
    })
}

fn con(mut t: Value, campo: &str, v: Value) -> Value {
    t[campo] = v;
    t
}

/// Obligado (3 días), En espera (8 días, ratio 0.5) y tamaño 0 con descarga
/// y 3 días (Obligado).
fn protegidos() -> Vec<Value> {
    vec![
        con(
            con(cumplido(H3, "peliculas"), "seeding_time", json!(3 * DIA)),
            "ratio",
            json!(9.0),
        ),
        con(
            con(cumplido(H4, "peliculas"), "seeding_time", json!(8 * DIA)),
            "ratio",
            json!(0.5),
        ),
        con(
            con(cumplido(H5, ""), "total_size", json!(0)),
            "seeding_time",
            json!(3 * DIA),
        ),
    ]
}

struct Nada;
impl Mediateca for Nada {
    async fn olvidar(&self, _t: &TorrentQbt) -> Result<(), String> {
        Ok(())
    }
}

/// Mediateca que, al olvidar `tras`, simula un clic en «No borrar» de `hash`
/// para `regla` (un clic del usuario mientras la pasada sigue en marcha).
struct ClicDuranteLaPasada {
    db: SqlitePool,
    tras: &'static str,
    hash: &'static str,
    regla: TipoRegla,
    hecho: Mutex<bool>,
}
impl Mediateca for ClicDuranteLaPasada {
    async fn olvidar(&self, t: &TorrentQbt) -> Result<(), String> {
        let toca = t.hash == self.tras && !*self.hecho.lock().unwrap();
        if toca {
            *self.hecho.lock().unwrap() = true;
            tarea::no_borrar(&self.db, self.hash, self.regla, T0)
                .await
                .unwrap();
        }
        Ok(())
    }
}

fn agresivas() -> ReglasCategoria {
    ReglasCategoria {
        ratio: Some(0.0),
        siembra_dias: Some(0),
        inactividad_dias: Some(0),
        disco_libre_pct: Some(100.0),
    }
}

fn disco_lleno(_: &Path) -> Option<Disco> {
    Some(Disco {
        libre: 0,
        total: 1_000_000,
    })
}
fn sin_disco(_: &Path) -> Option<Disco> {
    None
}

async fn prueba(ts: Vec<Value>) -> Prueba {
    let p = comun::con_sim(qbt_sim::Conf {
        lista_blanca: vec![[127, 0, 0, 1].into()],
        ..Default::default()
    })
    .await;
    p.sim().control.fijar_torrents(ts);
    p
}

fn borrados(p: &Prueba) -> Vec<String> {
    p.sim()
        .control
        .llamadas()
        .into_iter()
        .filter(|l| l.ruta == "/api/v2/torrents/delete")
        .map(|l| l.cuerpo)
        .collect()
}

#[tokio::test]
async fn sonda_base_nueva_nunca_borra_ni_avisa() {
    let mut ts = vec![cumplido(H1, "peliculas"), cumplido(H2, "")];
    ts.extend(protegidos());
    let p = prueba(ts).await;
    for i in 0..200 {
        let inf = tarea::pasada(&p.state, &Nada, &disco_lleno, T0 + i * 7 * DIA, &RED_OK, 0).await;
        assert!(
            inf.avisados.is_empty() && inf.se_habrian_borrado.is_empty(),
            "{inf:?}"
        );
    }
    assert!(borrados(&p).is_empty());
    assert_eq!(db::modo(&p.state.db).await.unwrap(), Modo::SoloAviso);
}

#[tokio::test]
async fn sonda_solo_aviso_con_todas_las_reglas_nunca_borra() {
    let p = prueba(vec![cumplido(H1, "peliculas"), cumplido(H2, "")]).await;
    for c in ["peliculas", ""] {
        db::guardar_reglas(&p.state.db, c, &agresivas())
            .await
            .unwrap();
    }
    let mut habrian = 0;
    for i in 0..100 {
        let inf = tarea::pasada(&p.state, &Nada, &disco_lleno, T0 + i * DIA, &RED_OK, 0).await;
        habrian += inf.se_habrian_borrado.len();
        assert!(inf.borrados.is_empty());
    }
    assert!(habrian > 0);
    assert!(borrados(&p).is_empty());
    assert_eq!(p.sim().control.torrents().len(), 2);
}

#[tokio::test]
async fn sonda_obligados_y_en_espera_nunca_se_avisan_ni_borran() {
    let p = prueba(protegidos()).await;
    for c in ["peliculas", ""] {
        db::guardar_reglas(&p.state.db, c, &agresivas())
            .await
            .unwrap();
    }
    db::fijar_modo(&p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    for i in 0..60 {
        let inf = tarea::pasada(&p.state, &Nada, &disco_lleno, T0 + i * DIA, &RED_OK, 0).await;
        assert!(inf.avisados.is_empty(), "{inf:?}");
    }
    assert!(borrados(&p).is_empty());
    assert!(db::avisos(&p.state.db).await.unwrap().is_empty());
}

#[tokio::test]
async fn sonda_categoria_sin_reglas_y_sin_categoria() {
    // Reglas solo en «sin categoría»: H1 (peliculas, sin fila) no se toca.
    let p = prueba(vec![cumplido(H1, "peliculas"), cumplido(H2, "")]).await;
    db::guardar_reglas(&p.state.db, "", &agresivas())
        .await
        .unwrap();
    db::fijar_modo(&p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    tarea::pasada(&p.state, &Nada, &disco_lleno, T0, &RED_OK, 0).await;
    let inf = tarea::pasada(&p.state, &Nada, &disco_lleno, T0 + DIA, &RED_OK, 0).await;
    assert_eq!(inf.borrados, vec![H2.to_string()]);
    assert_eq!(borrados(&p), vec![format!("hashes={H2}&deleteFiles=true")]);
}

#[tokio::test]
async fn sonda_red_caida_dias_solo_vence_tras_24h_efectivas() {
    let p = prueba(vec![cumplido(H1, "peliculas")]).await;
    let r = ReglasCategoria {
        ratio: Some(1.0),
        ..Default::default()
    };
    db::guardar_reglas(&p.state.db, "peliculas", &r)
        .await
        .unwrap();
    db::fijar_modo(&p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    // Aviso a T0; 2 h buenas, luego 3 días caída, luego vuelve.
    tarea::pasada(&p.state, &Nada, &sin_disco, T0, &RED_OK, 0).await;
    let mut t = T0;
    for _ in 0..2 {
        t += 3600;
        tarea::pasada(&p.state, &Nada, &sin_disco, t, &RED_OK, 0).await;
    }
    for _ in 0..72 {
        t += 3600;
        let inf = tarea::pasada(&p.state, &Nada, &sin_disco, t, &RED_CAIDA, 3600).await;
        assert!(inf.borrados.is_empty());
    }
    // 2 h + 72 h; quedan 22 h efectivas.
    for _ in 0..21 {
        t += 3600;
        let inf = tarea::pasada(&p.state, &Nada, &sin_disco, t, &RED_OK, 0).await;
        assert!(
            inf.borrados.is_empty(),
            "venció a las {} h",
            (t - T0) / 3600
        );
    }
    t += 3600;
    let inf = tarea::pasada(&p.state, &Nada, &sin_disco, t, &RED_OK, 0).await;
    assert_eq!(inf.borrados, vec![H1.to_string()]);
}

#[tokio::test]
async fn sonda_caido_negativo_no_adelanta() {
    let p = prueba(vec![cumplido(H1, "peliculas")]).await;
    let r = ReglasCategoria {
        ratio: Some(1.0),
        ..Default::default()
    };
    db::guardar_reglas(&p.state.db, "peliculas", &r)
        .await
        .unwrap();
    db::fijar_modo(&p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    tarea::pasada(&p.state, &Nada, &sin_disco, T0, &RED_OK, 0).await;
    let inf = tarea::pasada(&p.state, &Nada, &sin_disco, T0 + 3600, &RED_OK, -10 * DIA).await;
    assert!(inf.borrados.is_empty());
    let inf = tarea::pasada(
        &p.state,
        &Nada,
        &sin_disco,
        T0 + DIA - 1,
        &RED_OK,
        -10 * DIA,
    )
    .await;
    assert!(inf.borrados.is_empty());
}

#[tokio::test]
async fn sonda_relectura_fallida_u_otro_hash_no_borra() {
    let p = prueba(vec![cumplido(H1, "peliculas"), cumplido(H2, "peliculas")]).await;
    let r = ReglasCategoria {
        ratio: Some(1.0),
        ..Default::default()
    };
    db::guardar_reglas(&p.state.db, "peliculas", &r)
        .await
        .unwrap();
    db::fijar_modo(&p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    tarea::pasada(&p.state, &Nada, &sin_disco, T0, &RED_OK, 0).await;
    let c = &p.sim().control;
    // H1: respuesta rota (qBittorrent falla al releer); H2: contesta con otro hash.
    c.cambiar_al_releer(H1, Some(json!({"roto": true})));
    c.cambiar_al_releer(H2, Some(cumplido(H3, "peliculas")));
    let inf = tarea::pasada(&p.state, &Nada, &sin_disco, T0 + DIA, &RED_OK, 0).await;
    assert!(inf.borrados.is_empty(), "{inf:?}");
    assert!(borrados(&p).is_empty());
    // H1 se reintenta: el aviso sigue.
    let av: Vec<String> = db::avisos(&p.state.db)
        .await
        .unwrap()
        .into_iter()
        .map(|a| a.hash)
        .collect();
    assert_eq!(av, vec![H1.to_string()]);
}

#[tokio::test]
async fn sonda_disco_sin_medida_no_avisa() {
    let p = prueba(vec![cumplido(H1, "peliculas")]).await;
    let r = ReglasCategoria {
        disco_libre_pct: Some(100.0),
        ..Default::default()
    };
    db::guardar_reglas(&p.state.db, "peliculas", &r)
        .await
        .unwrap();
    db::fijar_modo(&p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    for i in 0..5 {
        let inf = tarea::pasada(&p.state, &Nada, &sin_disco, T0 + i * DIA, &RED_OK, 0).await;
        assert!(inf.avisados.is_empty());
    }
    assert!(borrados(&p).is_empty());
}

/// Clic en «No borrar» de H2 (aviso por ratio) mientras la pasada borra H1.
/// H2 solo cumple ratio: no debe borrarse.
#[tokio::test]
async fn sonda_no_borrar_durante_la_pasada_misma_regla() {
    let p = prueba(vec![cumplido(H1, "peliculas"), cumplido(H2, "peliculas")]).await;
    let r = ReglasCategoria {
        ratio: Some(1.0),
        ..Default::default()
    };
    db::guardar_reglas(&p.state.db, "peliculas", &r)
        .await
        .unwrap();
    db::fijar_modo(&p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    tarea::pasada(&p.state, &Nada, &sin_disco, T0, &RED_OK, 0).await;
    let m = ClicDuranteLaPasada {
        db: p.state.db.clone(),
        tras: H1,
        hash: H2,
        regla: TipoRegla::Ratio,
        hecho: Mutex::new(false),
    };
    let inf = tarea::pasada(&p.state, &m, &sin_disco, T0 + DIA, &RED_OK, 0).await;
    assert_eq!(inf.borrados, vec![H1.to_string()], "{inf:?}");
}

/// Igual, pero H2 también cumple «tiempo sembrando»: el aviso visible era por
/// ratio; tras «No borrar» no debe borrarse sin un aviso de 24 h por la otra
/// regla.
#[tokio::test]
async fn sonda_no_borrar_durante_la_pasada_otra_regla() {
    let p = prueba(vec![cumplido(H1, "peliculas"), cumplido(H2, "peliculas")]).await;
    let r = ReglasCategoria {
        ratio: Some(1.0),
        siembra_dias: Some(30),
        ..Default::default()
    };
    db::guardar_reglas(&p.state.db, "peliculas", &r)
        .await
        .unwrap();
    db::fijar_modo(&p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    tarea::pasada(&p.state, &Nada, &sin_disco, T0, &RED_OK, 0).await;
    let m = ClicDuranteLaPasada {
        db: p.state.db.clone(),
        tras: H1,
        hash: H2,
        regla: TipoRegla::Ratio,
        hecho: Mutex::new(false),
    };
    let inf = tarea::pasada(&p.state, &m, &sin_disco, T0 + DIA, &RED_OK, 0).await;
    assert_eq!(inf.borrados, vec![H1.to_string()], "{inf:?}");
    assert_eq!(borrados(&p), vec![format!("hashes={H1}&deleteFiles=true")]);
}

#[tokio::test]
async fn sonda_dos_pasadas_a_la_vez() {
    let p = prueba(vec![cumplido(H1, "peliculas")]).await;
    let r = ReglasCategoria {
        ratio: Some(1.0),
        ..Default::default()
    };
    db::guardar_reglas(&p.state.db, "peliculas", &r)
        .await
        .unwrap();
    db::fijar_modo(&p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    tarea::pasada(&p.state, &Nada, &sin_disco, T0, &RED_OK, 0).await;
    let (a, b) = tokio::join!(
        tarea::pasada(&p.state, &Nada, &sin_disco, T0 + DIA, &RED_OK, 0),
        tarea::pasada(&p.state, &Nada, &sin_disco, T0 + DIA, &RED_OK, 0),
    );
    eprintln!("a={a:?}\nb={b:?}\ndeletes={:?}", borrados(&p));
    // Las dos pasadas no se pisan: un solo borrado, apuntado una vez.
    assert_eq!(borrados(&p), vec![format!("hashes={H1}&deleteFiles=true")]);
    assert_eq!(a.borrados.len() + b.borrados.len(), 1);
    let veces = db::historial(&p.state.db, 100)
        .await
        .unwrap()
        .iter()
        .filter(|x| x.evento == db::Evento::Borrado)
        .count();
    assert_eq!(veces, 1);
}

#[tokio::test]
async fn sonda_hash_no_valido_nunca_se_borra() {
    let malo = "../../etc";
    let p = prueba(vec![
        cumplido(H1, "peliculas"),
        con(cumplido(H1, "peliculas"), "hash", json!(malo)),
    ])
    .await;
    let r = ReglasCategoria {
        ratio: Some(1.0),
        ..Default::default()
    };
    db::guardar_reglas(&p.state.db, "peliculas", &r)
        .await
        .unwrap();
    db::fijar_modo(&p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    tarea::pasada(&p.state, &Nada, &sin_disco, T0, &RED_OK, 0).await;
    let inf = tarea::pasada(&p.state, &Nada, &sin_disco, T0 + DIA, &RED_OK, 0).await;
    assert_eq!(inf.borrados, vec![H1.to_string()], "{inf:?}");
    assert!(borrados(&p).iter().all(|b| !b.contains("etc")));
}
