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

// ------------------------------------------- Bases de versiones publicadas

/// Hasta qué migración deja la base de datos cada versión publicada desde la
/// 0.3.0. Una versión nueva que añada migraciones es una línea más aquí (y,
/// si trae tablas nuevas, sus filas en [`FILAS_DE_PRUEBA`]).
const NIVELES_PUBLICADOS: &[(&str, i64)] = &[("0.3.0 a 0.5.0", 4), ("0.6.0", 5)];

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// Filas de prueba de cada tabla, con la migración que la crea: a una base de
/// un nivel se le meten las de las migraciones que tiene.
const FILAS_DE_PRUEBA: &[(i64, &str)] = &[
    (
        1,
        "INSERT INTO sesiones (id, usuario, creada, vista, reautenticado_hasta)
         VALUES ('5e5104', 'admin', 1800000000, 1800000100, NULL),
                ('5e5105', 'admin', 1800000200, 1800000300, 1800000900)",
    ),
    (
        1,
        "INSERT INTO freno (tipo, clave, fallos, ultimo_fallo_ms)
         VALUES ('ip', '192.0.2.7', 3, 1800000000123), ('usuario', 'admin', 1, 1800000000456)",
    ),
    (1, "INSERT INTO ajustes (clave, valor) VALUES ('modo', 'borrado_real')"),
    (
        1,
        "INSERT INTO reglas (categoria, ratio, siembra_dias, inactividad_dias, disco_libre_pct)
         VALUES ('series', 2.5, 10, NULL, NULL), ('', NULL, NULL, 30, 5.0)",
    ),
    (
        1,
        "INSERT INTO avisos (hash, regla, creado_en, vence_en)
         VALUES ('aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'ratio', 1800000000, 1800086400)",
    ),
    (
        1,
        "INSERT INTO exenciones (hash, regla, creada)
         VALUES ('bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 'siembra', 1800000000)",
    ),
    (
        1,
        "INSERT INTO historial (ts, hash, nombre, evento, regla, detalle)
         VALUES (1800000000, 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 'Una serie', 'Aviso', 'ratio',
                 'ratio 2,50 de 2,50'),
                (1800000500, 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb', 'Otra', 'Pausado', NULL, '')",
    ),
    (
        1,
        "INSERT INTO muestras_subida (hash, ts, subido)
         VALUES ('aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 1800000000, 123456789)",
    ),
    (
        2,
        "INSERT INTO borrados_pedidos (hash, regla, pedido_en, torrent)
         VALUES ('cccccccccccccccccccccccccccccccccccccccc', 'disco', 1800000000,
                 '{\"hash\":\"cccccccccccccccccccccccccccccccccccccccc\",\"name\":\"Una película\"}')",
    ),
    (
        3,
        "UPDATE normas SET umbral_pct = 5, siembra_dias = 2, ratio_min = 0.8,
           plazo_largo_dias = 9, ratio_pausa = 1.5, aviso_horas = 12 WHERE id = 1",
    ),
    (
        4,
        "INSERT INTO presets (nombre, descripcion, archivo, cambiado)
         VALUES ('De una versión anterior', 'Siembra 2 días.', 'formato = 1
nombre = \"De una versión anterior\"
descripcion = \"Siembra 2 días.\"

[normas]
umbral_pct = 5.0
siembra_dias = 2
ratio_min = 0.8
plazo_largo_dias = 9
ratio_pausa = 1.5
aviso_horas = 12
', 1800000000)",
    ),
    (
        5,
        "INSERT INTO normas_tracker (host, umbral_pct, siembra_dias, ratio_min, plazo_largo_dias,
                                     ratio_pausa, aviso_horas)
         VALUES ('tracker.example', 20.0, 3, 0.5, 12, 4.0, 6)",
    ),
    (
        5,
        "INSERT INTO trackers_vistos (host, hash, visto)
         VALUES ('tracker.example', 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa', 1800000000)",
    ),
];

/// Las tablas de la base (sin las de SQLite ni la de sqlx).
async fn tablas(pool: &sqlx::SqlitePool) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table'
           AND name NOT LIKE 'sqlite_%' AND name <> '_sqlx_migrations' ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

/// Las filas de cada tabla, cada una como el texto SQL de sus valores
/// (`quote()`), con solo las columnas que tenía la tabla en la base de antes.
async fn foto(
    pool: &sqlx::SqlitePool,
    columnas: &std::collections::BTreeMap<String, Vec<String>>,
) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut foto = std::collections::BTreeMap::new();
    for (tabla, cols) in columnas {
        let valores = cols
            .iter()
            .map(|c| format!("quote(\"{c}\")"))
            .collect::<Vec<_>>()
            .join(" || ',' || ");
        let filas: Vec<String> =
            sqlx::query_scalar(&format!("SELECT {valores} FROM \"{tabla}\" ORDER BY rowid"))
                .fetch_all(pool)
                .await
                .unwrap_or_else(|e| panic!("{tabla}: {e}"));
        foto.insert(tabla.clone(), filas);
    }
    foto
}

#[tokio::test]
async fn las_bases_de_cada_version_publicada_abren_con_todas_sus_filas() {
    let todas = sqlx::migrate!("./migrations").migrations.len();
    for &(versiones, nivel) in NIVELES_PUBLICADOS {
        let dir = tempfile::tempdir().unwrap();
        let ruta = dir.path().join("antigua.db");
        // La base como la deja esa versión, con filas en todas sus tablas.
        let vieja = crate::comun::base_hasta_migracion(&ruta, nivel).await;
        for &(_, sql) in FILAS_DE_PRUEBA.iter().filter(|(m, _)| *m <= nivel) {
            sqlx::query(sql)
                .execute(&vieja)
                .await
                .unwrap_or_else(|e| panic!("{versiones}: {e}\n{sql}"));
        }
        let mut columnas = std::collections::BTreeMap::new();
        for tabla in tablas(&vieja).await {
            let cols: Vec<String> =
                sqlx::query_scalar(&format!("SELECT name FROM pragma_table_info('{tabla}')"))
                    .fetch_all(&vieja)
                    .await
                    .unwrap();
            columnas.insert(tabla, cols);
        }
        let antes = foto(&vieja, &columnas).await;
        for (tabla, filas) in &antes {
            assert!(
                !filas.is_empty(),
                "{versiones}: la tabla {tabla} no tiene filas de prueba (añádelas a FILAS_DE_PRUEBA)"
            );
        }
        vieja.close().await;

        // La abre el código de ahora, como al arrancar.
        let pool = db::abrir(&ruta).await.unwrap();
        let aplicadas: i64 =
            sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success = 1")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(aplicadas as usize, todas, "{versiones}");
        let despues = foto(&pool, &columnas).await;
        for (tabla, filas) in &antes {
            for fila in filas {
                assert!(
                    despues[tabla].contains(fila),
                    "{versiones}: en {tabla} falta o ha cambiado la fila {fila}\ndespués: {:?}",
                    despues[tabla]
                );
            }
        }
        // Y se lee con sus valores.
        assert_eq!(
            db::modo(&pool).await.unwrap(),
            Modo::BorradoReal,
            "{versiones}"
        );
        assert_eq!(db::reglas(&pool).await.unwrap()["series"].ratio, Some(2.5));
        assert_eq!(
            db::normas(&pool).await.unwrap().aviso_horas,
            12,
            "{versiones}"
        );
        assert_eq!(db::presets(&pool).await.unwrap().len(), 1, "{versiones}");
        assert_eq!(
            db::avisos(&pool).await.unwrap(),
            vec![Aviso {
                hash: HASH_A.into(),
                regla: TipoRegla::Ratio,
                creado_en: 1_800_000_000,
                vence_en: 1_800_086_400,
            }],
            "{versiones}"
        );
        pool.close().await;
    }
}
