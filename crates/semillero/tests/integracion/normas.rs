//! Normas del tracker guardadas en la base de datos: por defecto, las de
//! siempre (todo sigue igual); cambiándolas, cambian la tarea, el borrado a
//! mano y lo que dicen las páginas.

use std::borrow::Cow;
use std::path::Path;

use reglas::{Disco, NormasTracker, ReglasCategoria};
use semillero::db::{self, Evento};
use semillero::estado_red::EstadoRed;
use semillero::tarea;
use serde_json::{Value, json};

use crate::comun::{self, Prueba};

const T0: i64 = 1_800_000_000;
const DIA: i64 = 86_400;
const H1: &str = "1111111111111111111111111111111111111111";
const H2: &str = "2222222222222222222222222222222222222222";
const RED_OK: EstadoRed = EstadoRed {
    vigilada: true,
    vpn: true,
    puerto: Some(51413),
};

fn torrent(hash: &str, dias: i64, ratio: f64, estado: &str) -> Value {
    json!({
        "hash": hash, "name": format!("Película {}", &hash[..1]), "category": "peliculas",
        "state": estado, "downloaded": 1000, "total_size": 1000, "uploaded": 1500,
        "seeding_time": dias * DIA, "ratio": ratio, "added_on": T0 - 10 * DIA,
        "save_path": "/media/peliculas", "upspeed": 0, "dlspeed": 0, "progress": 1.0
    })
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

async fn detalles(p: &Prueba) -> Vec<(Evento, String)> {
    let mut v: Vec<_> = db::historial(&p.state.db, 100)
        .await
        .unwrap()
        .into_iter()
        .map(|a| (a.evento, a.detalle))
        .collect();
    v.reverse();
    v
}

#[tokio::test]
async fn una_base_nueva_empieza_con_las_normas_por_defecto() {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::abrir(&dir.path().join("s.db")).await.unwrap();
    assert_eq!(db::normas(&pool).await.unwrap(), NormasTracker::default());
}

#[tokio::test]
async fn una_base_ya_en_uso_sigue_igual_tras_la_migracion() {
    let dir = tempfile::tempdir().unwrap();
    let ruta = dir.path().join("s.db");
    // La base como estaba antes de las normas: solo las dos primeras
    // migraciones, con reglas guardadas.
    {
        let opciones = sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&ruta)
            .create_if_missing(true);
        let pool = sqlx::SqlitePool::connect_with(opciones).await.unwrap();
        let mut antes = sqlx::migrate!("./migrations");
        antes.migrations = Cow::Owned(antes.migrations[..2].to_vec());
        antes.run(&pool).await.unwrap();
        let r = ReglasCategoria {
            ratio: Some(2.0),
            ..Default::default()
        };
        db::guardar_reglas(&pool, "peliculas", &r).await.unwrap();
        pool.close().await;
    }
    let pool = db::abrir(&ruta).await.unwrap();
    assert_eq!(db::normas(&pool).await.unwrap(), NormasTracker::default());
    assert_eq!(
        db::reglas(&pool).await.unwrap()["peliculas"].ratio,
        Some(2.0)
    );
}

#[tokio::test]
async fn las_normas_fuera_de_rango_no_se_guardan() {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::abrir(&dir.path().join("s.db")).await.unwrap();
    let buenas = NormasTracker {
        siembra_dias: 3,
        ..NormasTracker::default()
    };
    db::guardar_normas(&pool, &buenas).await.unwrap();
    assert_eq!(db::normas(&pool).await.unwrap(), buenas);
    // La capa de la base de datos las comprueba...
    let malas = NormasTracker {
        umbral_pct: 150.0,
        ..buenas
    };
    assert!(db::guardar_normas(&pool, &malas).await.is_err());
    // ...y la tabla tampoco las admite escritas a mano.
    let r = sqlx::query("UPDATE normas SET aviso_horas = 0")
        .execute(&pool)
        .await;
    assert!(r.is_err());
    let r = sqlx::query("UPDATE normas SET plazo_largo_dias = 1")
        .execute(&pool)
        .await;
    assert!(r.is_err());
    assert_eq!(db::normas(&pool).await.unwrap(), buenas);
}

#[tokio::test]
async fn con_las_normas_por_defecto_la_tarea_dice_lo_de_siempre() {
    let p = prueba(vec![
        torrent(H1, 8, 2.0, "uploading"),
        torrent(H2, 8, 1.5, "stoppedUP"),
    ])
    .await;
    let r = ReglasCategoria {
        ratio: Some(1.2),
        ..Default::default()
    };
    db::guardar_reglas(&p.state.db, "peliculas", &r)
        .await
        .unwrap();
    let inf = tarea::pasada(&p.state, &Nada, &sin_discos, T0, &RED_OK, 0).await;
    assert_eq!(inf.pausados, vec![H1.to_string()]);
    let d = detalles(&p).await;
    assert!(
        d.contains(&(
            Evento::Pausado,
            "Parado: 7 días sembrando y ratio 2.0 o más".into()
        )),
        "{d:?}"
    );
    assert!(
        d.iter()
            .any(|(e, t)| *e == Evento::Aviso && t.starts_with("Se borrará en 24 horas: ")),
        "{d:?}"
    );
    let avisos = db::avisos(&p.state.db).await.unwrap();
    assert!(avisos.iter().all(|a| a.vence_en == T0 + DIA), "{avisos:?}");
}

#[tokio::test]
async fn otras_normas_cambian_la_pausa_y_los_avisos() {
    // 4 días sembrando: con las normas por defecto, recién sembrados.
    let p = prueba(vec![
        torrent(H1, 4, 2.0, "uploading"),
        torrent(H2, 4, 1.5, "stoppedUP"),
    ])
    .await;
    let r = ReglasCategoria {
        ratio: Some(1.2),
        ..Default::default()
    };
    db::guardar_reglas(&p.state.db, "peliculas", &r)
        .await
        .unwrap();
    let inf = tarea::pasada(&p.state, &Nada, &sin_discos, T0, &RED_OK, 0).await;
    assert!(
        inf.pausados.is_empty() && inf.avisados.is_empty(),
        "{inf:?}"
    );

    let n = NormasTracker {
        siembra_dias: 3,
        ratio_pausa: 1.8,
        aviso_horas: 48,
        ..NormasTracker::default()
    };
    db::guardar_normas(&p.state.db, &n).await.unwrap();
    let inf = tarea::pasada(&p.state, &Nada, &sin_discos, T0 + 3600, &RED_OK, 0).await;
    assert_eq!(inf.pausados, vec![H1.to_string()]);
    assert_eq!(inf.avisados.len(), 2, "{inf:?}");
    let d = detalles(&p).await;
    assert!(
        d.contains(&(
            Evento::Pausado,
            "Parado: 3 días sembrando y ratio 1.8 o más".into()
        )),
        "{d:?}"
    );
    assert!(
        d.iter()
            .any(|(e, t)| *e == Evento::Aviso && t.starts_with("Se borrará en 48 horas: ")),
        "{d:?}"
    );
    let avisos = db::avisos(&p.state.db).await.unwrap();
    assert!(
        avisos.iter().all(|a| a.vence_en == T0 + 3600 + 2 * DIA),
        "{avisos:?}"
    );
}

#[tokio::test]
async fn el_borrado_a_mano_sigue_las_normas() {
    let p = prueba(vec![torrent(H1, 4, 1.5, "stoppedUP")]).await;
    let r = tarea::quitar_a_mano(&p.state, &Nada, H1, false, T0).await;
    assert!(matches!(r, Err(tarea::NoQuitado::Protegido(_))), "{r:?}");
    let n = NormasTracker {
        siembra_dias: 3,
        ..NormasTracker::default()
    };
    db::guardar_normas(&p.state.db, &n).await.unwrap();
    tarea::quitar_a_mano(&p.state, &Nada, H1, false, T0)
        .await
        .unwrap();
}
