//! Tarea horaria de extremo a extremo con `qbt-sim`: hora, red y discos
//! inyectados.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use qbt::TorrentQbt;
use reglas::{Aviso, Disco, ReglasCategoria, TipoRegla};
use semillero::db::{self, Evento, Modo};
use semillero::estado_red::EstadoRed;
use semillero::mediateca::Mediateca;
use semillero::tarea::{self, Informe, Vigia};
use serde_json::{Value, json};

use crate::comun::{self, Prueba};

const T0: i64 = 1_800_000_000;
const DIA: i64 = 86_400;
const H1: &str = "1111111111111111111111111111111111111111";
const H2: &str = "2222222222222222222222222222222222222222";
const H3: &str = "3333333333333333333333333333333333333333";
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

/// Cumplido: completo, 8 días sembrando, ratio 1.5, parado (no se pausa).
fn torrent(hash: &str) -> Value {
    json!({
        "hash": hash, "name": format!("Película {}", &hash[..1]), "category": "peliculas",
        "state": "stoppedUP", "downloaded": 1000, "total_size": 1000, "uploaded": 1500,
        "seeding_time": 8 * DIA, "ratio": 1.5, "added_on": T0 - 10 * DIA,
        "save_path": "/media/peliculas", "upspeed": 0, "dlspeed": 0, "progress": 1.0
    })
}

fn con(mut t: Value, campo: &str, v: Value) -> Value {
    t[campo] = v;
    t
}

/// Mediateca falsa: apunta lo que se le pide olvidar.
#[derive(Default)]
struct Falsa {
    olvidados: Mutex<Vec<String>>,
    falla: bool,
}

impl Mediateca for Falsa {
    async fn olvidar(&self, t: &TorrentQbt) -> Result<(), String> {
        self.olvidados.lock().unwrap().push(t.hash.clone());
        if self.falla {
            Err("Jellyfin no contesta".into())
        } else {
            Ok(())
        }
    }
}

fn sin_discos(_: &Path) -> Option<Disco> {
    None
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

async fn reglas_ratio(p: &Prueba, ratio: f64) {
    let r = ReglasCategoria {
        ratio: Some(ratio),
        ..Default::default()
    };
    db::guardar_reglas(&p.state.db, "peliculas", &r)
        .await
        .unwrap();
}

async fn pasar(p: &Prueba, m: &Falsa, ahora: i64, red: EstadoRed, caido_s: i64) -> Informe {
    tarea::pasada(&p.state, m, &sin_discos, ahora, &red, caido_s).await
}

async fn eventos(p: &Prueba) -> Vec<(String, Evento, Option<TipoRegla>)> {
    let mut v: Vec<_> = db::historial(&p.state.db, 100)
        .await
        .unwrap()
        .into_iter()
        .map(|a| (a.hash, a.evento, a.regla))
        .collect();
    v.reverse();
    v
}

fn rutas_llamadas(p: &Prueba, ruta: &str) -> Vec<String> {
    p.sim()
        .control
        .llamadas()
        .into_iter()
        .filter(|l| l.ruta == ruta)
        .map(|l| l.cuerpo)
        .collect()
}

fn borrados_en_qbt(p: &Prueba) -> Vec<String> {
    rutas_llamadas(p, "/api/v2/torrents/delete")
}

#[tokio::test]
async fn aparece_el_aviso_solo_en_cumplidos() {
    // H2 es Obligado (3 días): no se avisa aunque tenga ratio.
    let p = prueba(vec![
        torrent(H1),
        con(torrent(H2), "seeding_time", json!(3 * DIA)),
    ])
    .await;
    reglas_ratio(&p, 1.2).await;
    let inf = pasar(&p, &Falsa::default(), T0, RED_OK, 0).await;
    assert!(inf.qbt_ok);
    assert_eq!(inf.avisados, vec![H1.to_string()]);
    assert_eq!(
        db::avisos(&p.state.db).await.unwrap(),
        vec![Aviso {
            hash: H1.into(),
            regla: TipoRegla::Ratio,
            creado_en: T0,
            vence_en: T0 + DIA,
        }]
    );
    assert_eq!(
        eventos(&p).await,
        vec![(H1.into(), Evento::Aviso, Some(TipoRegla::Ratio))]
    );
    let h = db::historial(&p.state.db, 1).await.unwrap();
    assert_eq!(h[0].nombre, "Película 1");
}

#[tokio::test]
async fn sin_reglas_no_hay_avisos() {
    let p = prueba(vec![torrent(H1)]).await;
    let inf = pasar(&p, &Falsa::default(), T0, RED_OK, 0).await;
    assert!(inf.qbt_ok);
    assert!(db::avisos(&p.state.db).await.unwrap().is_empty());
}

#[tokio::test]
async fn no_borrar_cancela_y_exime_solo_de_esa_regla() {
    let p = prueba(vec![torrent(H1)]).await;
    let r = ReglasCategoria {
        ratio: Some(1.2),
        siembra_dias: Some(7),
        ..Default::default()
    };
    db::guardar_reglas(&p.state.db, "peliculas", &r)
        .await
        .unwrap();
    let m = Falsa::default();
    pasar(&p, &m, T0, RED_OK, 0).await;
    assert_eq!(
        db::avisos(&p.state.db).await.unwrap()[0].regla,
        TipoRegla::Ratio
    );

    tarea::no_borrar(&p.state.db, H1, TipoRegla::Ratio, T0 + 60)
        .await
        .unwrap();
    assert!(db::avisos(&p.state.db).await.unwrap().is_empty());
    assert_eq!(
        eventos(&p).await.last().unwrap(),
        &(H1.to_string(), Evento::Cancelado, Some(TipoRegla::Ratio))
    );

    // La siguiente pasada lo vuelve a elegir, pero por la otra regla y con
    // un plazo nuevo de 24 h.
    pasar(&p, &m, T0 + 3600, RED_OK, 0).await;
    assert_eq!(
        db::avisos(&p.state.db).await.unwrap(),
        vec![Aviso {
            hash: H1.into(),
            regla: TipoRegla::Siembra,
            creado_en: T0 + 3600,
            vence_en: T0 + 3600 + DIA,
        }]
    );

    // Eximido de las dos: no hay aviso.
    tarea::no_borrar(&p.state.db, H1, TipoRegla::Siembra, T0 + 3700)
        .await
        .unwrap();
    pasar(&p, &m, T0 + 7200, RED_OK, 0).await;
    assert!(db::avisos(&p.state.db).await.unwrap().is_empty());

    // Deshacer el «No borrar» de la ratio: vuelve a avisar por la ratio.
    tarea::quitar_exencion(&p.state.db, H1, TipoRegla::Ratio, T0 + 7300)
        .await
        .unwrap();
    assert_eq!(
        eventos(&p).await.last().unwrap(),
        &(
            H1.to_string(),
            Evento::ExencionQuitada,
            Some(TipoRegla::Ratio)
        )
    );
    pasar(&p, &m, T0 + 10_800, RED_OK, 0).await;
    assert_eq!(
        db::avisos(&p.state.db).await.unwrap()[0].regla,
        TipoRegla::Ratio
    );
}

#[tokio::test]
async fn solo_aviso_nunca_borra_y_apunta_se_habria_borrado() {
    let p = prueba(vec![torrent(H1)]).await;
    reglas_ratio(&p, 1.2).await;
    let m = Falsa::default();
    pasar(&p, &m, T0, RED_OK, 0).await;
    let inf = pasar(&p, &m, T0 + DIA, RED_OK, 0).await;
    assert_eq!(inf.se_habrian_borrado, vec![H1.to_string()]);
    assert!(inf.borrados.is_empty());
    assert!(borrados_en_qbt(&p).is_empty());
    assert_eq!(p.sim().control.torrents().len(), 1);
    assert!(m.olvidados.lock().unwrap().is_empty());
    assert!(db::avisos(&p.state.db).await.unwrap().is_empty());
    assert_eq!(
        eventos(&p).await.last().unwrap(),
        &(
            H1.to_string(),
            Evento::SeHabriaBorrado,
            Some(TipoRegla::Ratio)
        )
    );
}

#[tokio::test]
async fn borrado_real_borra_torrent_y_ficheros_al_vencer() {
    let p = prueba(vec![torrent(H1), con(torrent(H2), "ratio", json!(1.0))]).await;
    reglas_ratio(&p, 1.2).await;
    tarea::cambiar_modo(&p.state.db, Modo::BorradoReal, T0 - 10)
        .await
        .unwrap();
    assert_eq!(db::modo(&p.state.db).await.unwrap(), Modo::BorradoReal);
    let m = Falsa::default();
    pasar(&p, &m, T0, RED_OK, 0).await;
    // Antes de las 24 h no se borra.
    pasar(&p, &m, T0 + DIA - 1, RED_OK, 0).await;
    assert!(borrados_en_qbt(&p).is_empty());

    let inf = pasar(&p, &m, T0 + DIA, RED_OK, 0).await;
    assert_eq!(inf.borrados, vec![H1.to_string()]);
    assert_eq!(
        borrados_en_qbt(&p),
        vec![format!("hashes={H1}&deleteFiles=true")]
    );
    assert_eq!(*m.olvidados.lock().unwrap(), vec![H1.to_string()]);
    let quedan: Vec<_> = p.sim().control.torrents();
    assert_eq!(quedan.len(), 1);
    assert_eq!(quedan[0]["hash"], H2);
    assert!(db::avisos(&p.state.db).await.unwrap().is_empty());
    let ev = eventos(&p).await;
    assert_eq!(ev[0], (String::new(), Evento::ModoCambiado, None));
    assert_eq!(
        ev.last().unwrap(),
        &(H1.to_string(), Evento::Borrado, Some(TipoRegla::Ratio))
    );
}

#[tokio::test]
async fn un_fallo_de_la_mediateca_no_deshace_el_borrado() {
    let p = prueba(vec![torrent(H1)]).await;
    reglas_ratio(&p, 1.2).await;
    db::fijar_modo(&p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    let m = Falsa {
        falla: true,
        ..Default::default()
    };
    pasar(&p, &m, T0, RED_OK, 0).await;
    let inf = pasar(&p, &m, T0 + DIA, RED_OK, 0).await;
    assert_eq!(inf.borrados, vec![H1.to_string()]);
    assert!(p.sim().control.torrents().is_empty());
    let h = db::historial(&p.state.db, 1).await.unwrap();
    assert_eq!(h[0].evento, Evento::Borrado);
    assert!(
        h[0].detalle.contains("Jellyfin no contesta"),
        "{}",
        h[0].detalle
    );
}

#[tokio::test]
async fn antes_de_borrar_se_vuelve_a_leer_y_clasificar() {
    // H1 vuelve a Obligado (se ha vuelto a bajar), H2 desaparece y H3 baja de
    // ratio, justo entre la lista y la comprobación final.
    let p = prueba(vec![torrent(H1), torrent(H2), torrent(H3)]).await;
    reglas_ratio(&p, 1.2).await;
    db::fijar_modo(&p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    let m = Falsa::default();
    pasar(&p, &m, T0, RED_OK, 0).await;
    assert_eq!(db::avisos(&p.state.db).await.unwrap().len(), 3);
    let c = &p.sim().control;
    c.cambiar_al_releer(H1, Some(con(torrent(H1), "seeding_time", json!(DIA))));
    c.cambiar_al_releer(H2, None);
    c.cambiar_al_releer(H3, Some(con(torrent(H3), "ratio", json!(1.1))));
    let inf = pasar(&p, &m, T0 + DIA, RED_OK, 0).await;
    assert!(inf.borrados.is_empty(), "{inf:?}");
    assert!(borrados_en_qbt(&p).is_empty());
    assert!(m.olvidados.lock().unwrap().is_empty());
    let mut retirados = inf.retirados.clone();
    retirados.sort();
    assert_eq!(retirados, vec![H1.to_string(), H2.into(), H3.into()]);
    assert!(db::avisos(&p.state.db).await.unwrap().is_empty());
}

#[tokio::test]
async fn el_aviso_desaparece_si_deja_de_cumplir_la_regla() {
    let p = prueba(vec![torrent(H1)]).await;
    reglas_ratio(&p, 1.2).await;
    db::fijar_modo(&p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    let m = Falsa::default();
    pasar(&p, &m, T0, RED_OK, 0).await;
    // Vuelve a descargarse: Obligado otra vez.
    p.sim()
        .control
        .fijar_torrents(vec![con(torrent(H1), "seeding_time", json!(DIA))]);
    let inf = pasar(&p, &m, T0 + DIA, RED_OK, 0).await;
    assert_eq!(inf.retirados, vec![H1.to_string()]);
    assert!(borrados_en_qbt(&p).is_empty());
    assert!(db::avisos(&p.state.db).await.unwrap().is_empty());
    assert_eq!(
        eventos(&p).await.last().unwrap(),
        &(H1.to_string(), Evento::Retirado, Some(TipoRegla::Ratio))
    );
    // Desaparecido de qBittorrent: también se retira (y se sabe su nombre).
    p.sim().control.fijar_torrents(vec![torrent(H1)]);
    pasar(&p, &m, T0 + 2 * DIA, RED_OK, 0).await;
    p.sim().control.fijar_torrents(vec![]);
    let inf = pasar(&p, &m, T0 + 2 * DIA + 3600, RED_OK, 0).await;
    assert_eq!(inf.retirados, vec![H1.to_string()]);
    let h = db::historial(&p.state.db, 1).await.unwrap();
    assert_eq!(
        (h[0].evento, h[0].nombre.as_str()),
        (Evento::Retirado, "Película 1")
    );
}

#[tokio::test]
async fn con_la_red_caida_nada_vence_y_el_plazo_se_corre() {
    let p = prueba(vec![torrent(H1)]).await;
    reglas_ratio(&p, 1.2).await;
    db::fijar_modo(&p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    let m = Falsa::default();
    pasar(&p, &m, T0, RED_OK, 0).await;
    // Una hora caída, ya pasadas las 24 h: no vence, y el plazo se corre 1 h.
    let inf = pasar(&p, &m, T0 + DIA, RED_CAIDA, 3600).await;
    assert!(inf.borrados.is_empty() && inf.se_habrian_borrado.is_empty());
    assert!(borrados_en_qbt(&p).is_empty());
    assert_eq!(
        db::avisos(&p.state.db).await.unwrap()[0].vence_en,
        T0 + DIA + 3600
    );
    // Red de vuelta, pero aún no han pasado 24 h efectivas.
    pasar(&p, &m, T0 + DIA + 1800, RED_OK, 0).await;
    assert!(borrados_en_qbt(&p).is_empty());
    // Ahora sí.
    let inf = pasar(&p, &m, T0 + DIA + 3600, RED_OK, 0).await;
    assert_eq!(inf.borrados, vec![H1.to_string()]);
}

#[tokio::test]
async fn solo_el_puerto_caido_tambien_congela() {
    let p = prueba(vec![torrent(H1)]).await;
    reglas_ratio(&p, 1.2).await;
    db::fijar_modo(&p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    let m = Falsa::default();
    pasar(&p, &m, T0, RED_OK, 0).await;
    let sin_puerto = EstadoRed {
        vigilada: true,
        vpn: true,
        puerto: None,
    };
    pasar(&p, &m, T0 + 2 * DIA, sin_puerto, 0).await;
    assert!(borrados_en_qbt(&p).is_empty());
}

#[tokio::test]
async fn con_qbittorrent_caido_no_se_hace_nada() {
    let p = comun::sin_qbt().await;
    let r = ReglasCategoria {
        ratio: Some(1.2),
        ..Default::default()
    };
    db::guardar_reglas(&p.state.db, "peliculas", &r)
        .await
        .unwrap();
    db::fijar_modo(&p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    let aviso = Aviso {
        hash: H1.into(),
        regla: TipoRegla::Ratio,
        creado_en: T0 - 2 * DIA,
        vence_en: T0 - DIA,
    };
    db::guardar_avisos(&p.state.db, std::slice::from_ref(&aviso))
        .await
        .unwrap();
    let m = Falsa::default();
    let inf = tarea::pasada(&p.state, &m, &sin_discos, T0, &RED_OK, 0).await;
    assert!(!inf.qbt_ok);
    assert!(!inf.errores.is_empty());
    assert_eq!(db::avisos(&p.state.db).await.unwrap(), vec![aviso]);
    assert!(db::historial(&p.state.db, 10).await.unwrap().is_empty());
    assert!(m.olvidados.lock().unwrap().is_empty());
}

#[tokio::test]
async fn pausa_con_siete_dias_y_ratio_dos() {
    let activo = |h: &str| con(torrent(h), "state", json!("uploading"));
    let p = prueba(vec![
        con(activo(H1), "ratio", json!(2.0)),
        con(
            con(activo(H2), "ratio", json!(2.5)),
            "seeding_time",
            json!(6 * DIA),
        ),
        con(activo(H3), "ratio", json!(1.9)),
    ])
    .await;
    let inf = pasar(&p, &Falsa::default(), T0, RED_OK, 0).await;
    assert_eq!(inf.pausados, vec![H1.to_string()]);
    assert_eq!(
        rutas_llamadas(&p, "/api/v2/torrents/stop"),
        vec![format!("hashes={H1}")]
    );
    assert_eq!(
        eventos(&p).await,
        vec![(H1.to_string(), Evento::Pausado, None)]
    );
    // Nunca borra.
    assert!(borrados_en_qbt(&p).is_empty());
    // Ya parado: no se vuelve a pausar.
    let inf = pasar(&p, &Falsa::default(), T0 + 3600, RED_OK, 0).await;
    assert!(inf.pausados.is_empty());
}

#[tokio::test]
async fn inactividad_con_las_muestras_de_subida() {
    // H1 no sube nada; H2 sí.
    let p = prueba(vec![torrent(H1), torrent(H2)]).await;
    let r = ReglasCategoria {
        inactividad_dias: Some(1),
        ..Default::default()
    };
    db::guardar_reglas(&p.state.db, "peliculas", &r)
        .await
        .unwrap();
    let m = Falsa::default();
    let inf = pasar(&p, &m, T0, RED_OK, 0).await;
    assert!(inf.avisados.is_empty());
    p.sim()
        .control
        .fijar_torrents(vec![torrent(H1), con(torrent(H2), "uploaded", json!(1600))]);
    pasar(&p, &m, T0 + DIA / 2, RED_OK, 0).await;
    let inf = pasar(&p, &m, T0 + DIA, RED_OK, 0).await;
    assert_eq!(inf.avisados, vec![H1.to_string()]);
    assert_eq!(
        db::avisos(&p.state.db).await.unwrap()[0].regla,
        TipoRegla::Inactividad
    );
    assert_eq!(
        db::ultima_subida_en(&p.state.db, H2).await.unwrap(),
        Some(T0 + DIA / 2)
    );
    // Una lista a medias (qBittorrent arrancando) no olvida las muestras de
    // los que faltan; solo cuando llevan 7 días sin aparecer.
    p.sim().control.fijar_torrents(vec![]);
    pasar(&p, &m, T0 + DIA + 1800, RED_OK, 0).await;
    p.sim().control.fijar_torrents(vec![torrent(H1)]);
    pasar(&p, &m, T0 + DIA + 3600, RED_OK, 0).await;
    assert_eq!(
        db::ultima_subida_en(&p.state.db, H2).await.unwrap(),
        Some(T0 + DIA / 2)
    );
    pasar(&p, &m, T0 + 8 * DIA + 1, RED_OK, 0).await;
    assert_eq!(db::ultima_subida_en(&p.state.db, H2).await.unwrap(), None);
    assert_eq!(
        db::ultima_subida_en(&p.state.db, H1).await.unwrap(),
        Some(T0)
    );
}

#[tokio::test]
async fn disco_elige_del_mas_antiguo_al_mas_nuevo() {
    // Tres de 300 bytes; disco de 10 000 con 600 libres y umbral del 10 %:
    // hacen falta 400 más, así que se eligen los dos más antiguos.
    let p = prueba(vec![
        con(
            con(torrent(H1), "added_on", json!(T0 - DIA)),
            "total_size",
            json!(300),
        ),
        con(
            con(torrent(H2), "added_on", json!(T0 - 3 * DIA)),
            "total_size",
            json!(300),
        ),
        con(
            con(torrent(H3), "added_on", json!(T0 - 2 * DIA)),
            "total_size",
            json!(300),
        ),
    ])
    .await;
    // Con descargado 1000 > tamaño 300 siguen siendo Cumplidos.
    let r = ReglasCategoria {
        disco_libre_pct: Some(10.0),
        ..Default::default()
    };
    db::guardar_reglas(&p.state.db, "peliculas", &r)
        .await
        .unwrap();
    let rutas: Mutex<Vec<PathBuf>> = Mutex::default();
    let medir = |r: &Path| {
        rutas.lock().unwrap().push(r.to_path_buf());
        Some(Disco {
            libre: 600,
            total: 10_000,
        })
    };
    let m = Falsa::default();
    let inf = tarea::pasada(&p.state, &m, &medir, T0, &RED_OK, 0).await;
    let mut avisados = inf.avisados.clone();
    avisados.sort();
    assert_eq!(avisados, vec![H2.to_string(), H3.into()]);
    assert!(
        db::avisos(&p.state.db)
            .await
            .unwrap()
            .iter()
            .all(|a| a.regla == TipoRegla::Disco)
    );
    assert_eq!(
        *rutas.lock().unwrap(),
        vec![PathBuf::from("/media/peliculas")]
    );

    // Al vencer, en borrado real, se borran esos dos.
    db::fijar_modo(&p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    let inf = tarea::pasada(&p.state, &m, &medir, T0 + DIA, &RED_OK, 0).await;
    let mut borrados = inf.borrados.clone();
    borrados.sort();
    assert_eq!(borrados, vec![H2.to_string(), H3.into()]);
}

#[tokio::test]
async fn cambiar_modo_lo_apunta() {
    let p = prueba(vec![]).await;
    tarea::cambiar_modo(&p.state.db, Modo::BorradoReal, T0)
        .await
        .unwrap();
    tarea::cambiar_modo(&p.state.db, Modo::BorradoReal, T0 + 1)
        .await
        .unwrap();
    tarea::cambiar_modo(&p.state.db, Modo::SoloAviso, T0 + 2)
        .await
        .unwrap();
    assert_eq!(db::modo(&p.state.db).await.unwrap(), Modo::SoloAviso);
    let h = db::historial(&p.state.db, 10).await.unwrap();
    // Sin cambio, no se apunta.
    assert_eq!(h.len(), 2);
    assert!(h.iter().all(|a| a.evento == Evento::ModoCambiado));
    assert_eq!(h[0].detalle, "Solo aviso");
    assert_eq!(h[1].detalle, "Borrado real");
}

#[test]
fn el_vigia_cuenta_el_tiempo_caido_y_pasa_cada_hora() {
    let mut v = Vigia::default();
    // Primera comprobación: pasada enseguida.
    assert!(v.tic(60, true));
    v.hecha(true);
    for _ in 0..59 {
        assert!(!v.tic(60, true));
    }
    assert_eq!(v.caido_s(), 0);
    assert!(v.tic(60, false));
    assert_eq!(v.caido_s(), 60);
    // qBittorrent no contestó: el tiempo caído se guarda para la siguiente.
    v.hecha(false);
    assert_eq!(v.caido_s(), 60);
    for _ in 0..59 {
        assert!(!v.tic(60, false));
    }
    assert!(v.tic(60, true));
    assert_eq!(v.caido_s(), 60 + 59 * 60);
    v.hecha(true);
    assert_eq!(v.caido_s(), 0);
}

#[test]
fn mide_el_disco_de_una_ruta() {
    let dir = tempfile::tempdir().unwrap();
    let d = semillero::disco::medir(dir.path()).expect("statvfs");
    assert!(d.total > 0 && d.libre >= 0 && d.libre <= d.total, "{d:?}");
    assert_eq!(semillero::disco::medir(&dir.path().join("no-existe")), None);
}

/// Mediateca que, al primer olvido, rompe el historial (la tabla desaparece):
/// un fallo de la base de datos a mitad de los vencimientos.
struct RompeHistorial {
    db: sqlx::SqlitePool,
}

impl Mediateca for RompeHistorial {
    async fn olvidar(&self, _t: &TorrentQbt) -> Result<(), String> {
        let _ = sqlx::query("ALTER TABLE historial RENAME TO historial_roto")
            .execute(&self.db)
            .await;
        Ok(())
    }
}

#[tokio::test]
async fn un_fallo_del_historial_no_pierde_borrados_ni_avisos() {
    let p = prueba(vec![torrent(H1), torrent(H2)]).await;
    reglas_ratio(&p, 1.2).await;
    db::fijar_modo(&p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    pasar(&p, &Falsa::default(), T0, RED_OK, 0).await;

    let rompe = RompeHistorial {
        db: p.state.db.clone(),
    };
    let inf = tarea::pasada(&p.state, &rompe, &sin_discos, T0 + DIA, &RED_OK, 0).await;
    // Los dos se borraron: se dice aunque el historial no se pudiera escribir,
    // y un fallo con el primero no deja sin resolver al segundo.
    assert_eq!(
        inf.borrados,
        vec![H1.to_string(), H2.to_string()],
        "{inf:?}"
    );
    assert!(!inf.errores.is_empty());
    assert!(p.sim().control.torrents().is_empty());
    // Sin el «Borrado» apuntado, los avisos siguen guardados.
    assert_eq!(db::avisos(&p.state.db).await.unwrap().len(), 2);

    sqlx::query("ALTER TABLE historial_roto RENAME TO historial")
        .execute(&p.state.db)
        .await
        .unwrap();
    let m = Falsa::default();
    let inf = pasar(&p, &m, T0 + DIA + 3600, RED_OK, 0).await;
    assert_eq!(
        inf.borrados,
        vec![H1.to_string(), H2.to_string()],
        "{inf:?}"
    );
    assert!(inf.retirados.is_empty(), "{inf:?}");
    let ev = eventos(&p).await;
    let tras_avisos: Vec<_> = ev.iter().filter(|e| e.1 != Evento::Aviso).collect();
    assert_eq!(
        tras_avisos,
        vec![
            &(H1.to_string(), Evento::Borrado, Some(TipoRegla::Ratio)),
            &(H2.to_string(), Evento::Borrado, Some(TipoRegla::Ratio)),
        ]
    );
    assert!(db::avisos(&p.state.db).await.unwrap().is_empty());
    assert!(db::borrados_pedidos(&p.state.db).await.unwrap().is_empty());
    // Ya está todo resuelto: otra pasada no apunta nada más.
    pasar(&p, &m, T0 + DIA + 7200, RED_OK, 0).await;
    assert_eq!(eventos(&p).await.len(), ev.len());
}

#[tokio::test]
async fn un_borrado_sin_respuesta_clara_se_confirma_en_la_pasada_siguiente() {
    let (p, q) = comun::con_qbt_que_falla_al_borrar(vec![torrent(H1), torrent(H2)]).await;
    reglas_ratio(&p, 1.2).await;
    db::fijar_modo(&p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    let m = Falsa::default();
    pasar(&p, &m, T0, RED_OK, 0).await;
    let inf = pasar(&p, &m, T0 + DIA, RED_OK, 0).await;
    // qBittorrent contestó con un error: no se sabe si borró.
    assert!(inf.borrados.is_empty(), "{inf:?}");
    assert_eq!(inf.errores.len(), 2, "{inf:?}");
    assert!(m.olvidados.lock().unwrap().is_empty());
    // H2 sí sigue (se arrepintió a medias); H1 se borró.
    q.torrents.lock().unwrap().push(torrent(H2));

    let inf = pasar(&p, &m, T0 + DIA + 3600, RED_OK, 0).await;
    assert!(inf.retirados.is_empty(), "{inf:?}");
    assert!(inf.borrados.contains(&H1.to_string()), "{inf:?}");
    assert_eq!(m.olvidados.lock().unwrap()[0], H1);
    let h1: Vec<_> = db::historial(&p.state.db, 100)
        .await
        .unwrap()
        .into_iter()
        .filter(|a| a.hash == H1 && a.evento != Evento::Aviso)
        .collect();
    assert_eq!(h1.len(), 1, "{h1:?}");
    assert_eq!(h1[0].evento, Evento::Borrado);
    assert_eq!(h1[0].regla, Some(TipoRegla::Ratio));
    assert_eq!(h1[0].nombre, "Película 1");
    assert!(h1[0].detalle.contains("confirmado"), "{}", h1[0].detalle);
    // H2 seguía: se vuelve a pedir su borrado, sin apuntarlo como borrado.
    assert!(!inf.borrados.contains(&H2.to_string()), "{inf:?}");
    assert_eq!(
        *q.borrados.lock().unwrap(),
        vec![H1.to_string(), H2.to_string(), H2.to_string()]
    );
    assert!(
        db::historial(&p.state.db, 100)
            .await
            .unwrap()
            .iter()
            .all(|a| a.hash != H2 || a.evento == Evento::Aviso)
    );
}

#[tokio::test]
async fn un_borrado_fallido_con_la_regla_ya_retirada_no_deja_un_aviso_atascado() {
    let (p, q) = comun::con_qbt_que_falla_al_borrar(vec![torrent(H1)]).await;
    reglas_ratio(&p, 1.2).await;
    db::fijar_modo(&p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    let m = Falsa::default();
    pasar(&p, &m, T0, RED_OK, 0).await;
    // qBittorrent contesta con error y no borra: H1 sigue en la lista.
    pasar(&p, &m, T0 + DIA, RED_OK, 0).await;
    q.torrents.lock().unwrap().push(torrent(H1));
    // Antes de la pasada siguiente se apaga la regla: el aviso se retira.
    db::guardar_reglas(&p.state.db, "peliculas", &ReglasCategoria::default())
        .await
        .unwrap();
    let inf = pasar(&p, &m, T0 + DIA + 3600, RED_OK, 0).await;
    assert_eq!(inf.retirados, vec![H1.to_string()], "{inf:?}");
    assert!(db::avisos(&p.state.db).await.unwrap().is_empty());
    assert!(db::borrados_pedidos(&p.state.db).await.unwrap().is_empty());
    // Al volver la regla, aviso nuevo de 24 horas: nada se borra en el acto.
    reglas_ratio(&p, 1.2).await;
    let pedidos_antes = q.borrados.lock().unwrap().len();
    let inf = pasar(&p, &m, T0 + 2 * DIA, RED_OK, 0).await;
    assert_eq!(inf.avisados, vec![H1.to_string()], "{inf:?}");
    assert!(inf.borrados.is_empty(), "{inf:?}");
    assert_eq!(q.borrados.lock().unwrap().len(), pedidos_antes);
}
