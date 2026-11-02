//! Esquema y accesos de la base de datos.

use std::collections::HashSet;

use reglas::{Aviso, ReglasCategoria, TipoRegla};
use semillero::db::{self, Apunte, Evento, Modo};

async fn base() -> (sqlx::SqlitePool, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::abrir(&dir.path().join("s.db")).await.unwrap();
    (pool, dir)
}

#[tokio::test]
async fn empieza_en_solo_aviso_y_sin_reglas() {
    let (pool, _d) = base().await;
    assert_eq!(db::modo(&pool).await.unwrap(), Modo::SoloAviso);
    assert!(db::reglas(&pool).await.unwrap().is_empty());
    assert!(db::avisos(&pool).await.unwrap().is_empty());
    db::fijar_modo(&pool, Modo::BorradoReal).await.unwrap();
    assert_eq!(db::modo(&pool).await.unwrap(), Modo::BorradoReal);
    db::fijar_modo(&pool, Modo::SoloAviso).await.unwrap();
    assert_eq!(db::modo(&pool).await.unwrap(), Modo::SoloAviso);
}

#[tokio::test]
async fn reglas_por_categoria() {
    let (pool, _d) = base().await;
    let r = ReglasCategoria {
        ratio: Some(2.0),
        siembra_dias: None,
        inactividad_dias: Some(14),
        disco_libre_pct: Some(10.0),
    };
    db::guardar_reglas(&pool, "peliculas", &r).await.unwrap();
    db::guardar_reglas(&pool, "", &ReglasCategoria::default())
        .await
        .unwrap();
    let todas = db::reglas(&pool).await.unwrap();
    assert_eq!(todas["peliculas"], r);
    assert_eq!(todas[""], ReglasCategoria::default());
    let r2 = ReglasCategoria { ratio: None, ..r };
    db::guardar_reglas(&pool, "peliculas", &r2).await.unwrap();
    assert_eq!(db::reglas(&pool).await.unwrap()["peliculas"], r2);
}

#[tokio::test]
async fn avisos_y_exenciones() {
    let (pool, _d) = base().await;
    let a = |h: &str, r| Aviso {
        hash: h.into(),
        regla: r,
        creado_en: 10,
        vence_en: 100,
    };
    db::guardar_avisos(
        &pool,
        &[a("aa", TipoRegla::Ratio), a("bb", TipoRegla::Disco)],
    )
    .await
    .unwrap();
    assert_eq!(db::avisos(&pool).await.unwrap().len(), 2);
    db::guardar_avisos(&pool, &[a("bb", TipoRegla::Siembra)])
        .await
        .unwrap();
    assert_eq!(
        db::avisos(&pool).await.unwrap(),
        vec![a("bb", TipoRegla::Siembra)]
    );

    db::eximir(&pool, "aa", TipoRegla::Ratio, 5).await.unwrap();
    db::eximir(&pool, "aa", TipoRegla::Ratio, 6).await.unwrap();
    db::eximir(&pool, "aa", TipoRegla::Inactividad, 6)
        .await
        .unwrap();
    let esperado: HashSet<(String, TipoRegla)> = [
        ("aa".to_owned(), TipoRegla::Ratio),
        ("aa".to_owned(), TipoRegla::Inactividad),
    ]
    .into();
    assert_eq!(db::exenciones(&pool).await.unwrap(), esperado);
    db::quitar_exencion(&pool, "aa", TipoRegla::Ratio)
        .await
        .unwrap();
    assert_eq!(db::exenciones(&pool).await.unwrap().len(), 1);
}

#[tokio::test]
async fn historial_de_lo_mas_nuevo_a_lo_mas_viejo() {
    let (pool, _d) = base().await;
    let ap = |ts, evento, regla| Apunte {
        ts,
        hash: "aa".into(),
        nombre: "Una película".into(),
        evento,
        regla,
        detalle: String::new(),
    };
    db::apuntar(&pool, &ap(1, Evento::Aviso, Some(TipoRegla::Ratio)))
        .await
        .unwrap();
    db::apuntar(
        &pool,
        &ap(2, Evento::SeHabriaBorrado, Some(TipoRegla::Ratio)),
    )
    .await
    .unwrap();
    db::apuntar(&pool, &ap(3, Evento::Pausado, None))
        .await
        .unwrap();
    let h = db::historial(&pool, 2).await.unwrap();
    assert_eq!(
        h,
        vec![
            ap(3, Evento::Pausado, None),
            ap(2, Evento::SeHabriaBorrado, Some(TipoRegla::Ratio))
        ]
    );
}

#[tokio::test]
async fn ultima_subida_segun_las_muestras() {
    let (pool, _d) = base().await;
    assert_eq!(db::ultima_subida_en(&pool, "aa").await.unwrap(), None);
    db::apuntar_muestra(&pool, "aa", 100, 5).await.unwrap();
    db::apuntar_muestra(&pool, "aa", 200, 5).await.unwrap();
    assert_eq!(db::ultima_subida_en(&pool, "aa").await.unwrap(), Some(100));
    db::apuntar_muestra(&pool, "aa", 300, 9).await.unwrap();
    db::apuntar_muestra(&pool, "aa", 400, 9).await.unwrap();
    assert_eq!(db::ultima_subida_en(&pool, "aa").await.unwrap(), Some(300));
    db::apuntar_muestra(&pool, "bb", 50, 1).await.unwrap();
    assert_eq!(db::ultima_subida_en(&pool, "aa").await.unwrap(), Some(300));
    db::olvidar_muestras(&pool, 350).await.unwrap();
    assert_eq!(db::ultima_subida_en(&pool, "aa").await.unwrap(), Some(400));
    assert_eq!(db::ultima_subida_en(&pool, "bb").await.unwrap(), None);
}

#[tokio::test]
async fn el_historial_guarda_textos_de_largo_acotado() {
    let dir = tempfile::tempdir().unwrap();
    let pool = db::abrir(&dir.path().join("h.db")).await.unwrap();
    // Un nombre de torrent lo pone quien hizo el torrent, no semillero.
    let largo = "ñ".repeat(db::MAX_NOMBRE + 50);
    db::apuntar(
        &pool,
        &db::Apunte {
            ts: 1,
            hash: "1111111111111111111111111111111111111111".into(),
            nombre: largo,
            evento: db::Evento::Aviso,
            regla: None,
            detalle: "x".repeat(db::MAX_DETALLE + 10),
        },
    )
    .await
    .unwrap();
    let a = &db::historial(&pool, 10).await.unwrap()[0];
    assert_eq!(a.nombre.chars().count(), db::MAX_NOMBRE);
    assert_eq!(a.detalle.chars().count(), db::MAX_DETALLE);
    assert!(a.nombre.chars().all(|c| c == 'ñ'));
}

#[cfg(unix)]
#[tokio::test]
async fn la_base_solo_la_lee_su_dueno() {
    use std::os::unix::fs::PermissionsExt;
    let modo = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    let dir = tempfile::tempdir().unwrap();
    // Nueva, sea cual sea la umask del proceso.
    let nueva = dir.path().join("nueva.db");
    let pool = db::abrir(&nueva).await.unwrap();
    db::historial(&pool, 1).await.unwrap();
    assert_eq!(modo(&nueva), 0o600);
    for extra in ["nueva.db-wal", "nueva.db-shm"] {
        let p = dir.path().join(extra);
        if p.exists() {
            assert_eq!(modo(&p) & 0o077, 0, "{extra}");
        }
    }
    // Una que ya estaba legible por todos se cierra.
    let vieja = dir.path().join("vieja.db");
    std::fs::write(&vieja, b"").unwrap();
    std::fs::set_permissions(&vieja, std::fs::Permissions::from_mode(0o644)).unwrap();
    db::abrir(&vieja).await.unwrap();
    assert_eq!(modo(&vieja), 0o600);
}
