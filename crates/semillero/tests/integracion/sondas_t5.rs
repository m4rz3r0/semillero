//! Sondas del verificador independiente de T5 (borrado a mano, reautenticación,
//! freno por IP real, guardián). Todas: lo rechazado no cambia nada.

use std::net::IpAddr;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use reglas::{Aviso, ReglasCategoria, TipoRegla};
use semillero::api::ERROR_REAUTH;
use semillero::db::{self, BorradoPedido, Modo};
use semillero::estado_red::EstadoRed;
use semillero::mediateca::SinMediateca;
use serde_json::{Value, json};

use crate::comun::*;

const DIA: i64 = 86_400;
const CUMPLIDO: &str = "1111111111111111111111111111111111111111";
const OBLIGADO: &str = "2222222222222222222222222222222222222222";
const ESPERA: &str = "3333333333333333333333333333333333333333";
const LIBRE: &str = "4444444444444444444444444444444444444444";
const NO_ESTA: &str = "9999999999999999999999999999999999999999";

fn torrent(hash: &str, estado: &str, descargado: i64, dias: i64, ratio: f64) -> Value {
    json!({
        "hash": hash, "name": format!("T {hash}"), "category": "peliculas", "state": estado,
        "downloaded": descargado, "total_size": 1000, "uploaded": 0,
        "seeding_time": dias * DIA, "ratio": ratio, "added_on": 1_700_000_000,
        "save_path": "/media/peliculas", "upspeed": 0, "dlspeed": 0, "progress": 1.0
    })
}

fn torrents() -> Vec<Value> {
    vec![
        torrent(CUMPLIDO, "stalledUP", 1000, 8, 1.5),
        torrent(OBLIGADO, "uploading", 1000, 2, 0.3),
        torrent(ESPERA, "stoppedUP", 1000, 10, 0.5),
        torrent(LIBRE, "stalledDL", 0, 0, 0.0),
    ]
}

async fn prueba() -> Prueba {
    let p = con_sim(qbt_sim::Conf {
        api_abierta: true,
        ..Default::default()
    })
    .await;
    p.sim().control.fijar_torrents(torrents());
    p
}

fn api(uri: &str, cuerpo: &str, cookie: Option<&str>, token: Option<&str>) -> Request<Body> {
    let mut req = post(uri, cuerpo);
    let h = req.headers_mut();
    h.insert("content-type", "application/json".parse().unwrap());
    if let Some(c) = cookie {
        h.insert("cookie", c.parse().unwrap());
    }
    if let Some(t) = token {
        h.insert("x-csrf-token", t.parse().unwrap());
    }
    req
}

async fn llamar(p: &Prueba, c: &str, t: &str, uri: &str, cuerpo: Value) -> (StatusCode, String) {
    let r = enviar(&p.app, api(uri, &cuerpo.to_string(), Some(c), Some(t))).await;
    (r.status(), texto(r).await)
}

fn quitar(hash: &str, ficheros: bool, clave: Option<&str>) -> Value {
    json!({ "hash": hash, "ficheros": ficheros, "clave": clave })
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

fn en_qbt(p: &Prueba, hash: &str) -> bool {
    p.sim().control.torrents().iter().any(|t| t["hash"] == hash)
}

/// Foto de lo que un rechazo no debe tocar.
async fn foto(p: &Prueba) -> String {
    let db = &p.state.db;
    let avisos = db::avisos(db).await.unwrap();
    let pedidos = db::borrados_pedidos(db).await.unwrap();
    let hist = db::historial(db, 1000).await.unwrap().len();
    let modo = db::modo(db).await.unwrap();
    let reauth: Vec<Option<i64>> = sqlx::query_scalar("SELECT reautenticado_hasta FROM sesiones")
        .fetch_all(db)
        .await
        .unwrap();
    let mut reglas: Vec<String> = db::reglas(db)
        .await
        .unwrap()
        .into_iter()
        .map(|(k, v)| format!("{k}:{v:?}"))
        .collect();
    reglas.sort();
    format!(
        "{avisos:?}|{pedidos:?}|{hist}|{modo:?}|{reauth:?}|{}|{reglas:?}",
        p.sim().control.torrents().len(),
    )
}

#[tokio::test]
async fn sonda_t5_ventana_de_reauth_es_de_cada_sesion() {
    let p = prueba().await;
    let a = sesion(&p).await;
    let ta = token(&p, &a).await;
    let b = sesion(&p).await;
    let tb = token(&p, &b).await;
    assert_ne!(a, b);
    let (s, c) = llamar(
        &p,
        &a,
        &ta,
        "/api/quitar",
        quitar(LIBRE, false, Some(CLAVE)),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{c}");
    let antes = foto(&p).await;
    let (s, c) = llamar(&p, &b, &tb, "/api/quitar", quitar(CUMPLIDO, true, None)).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{c}");
    assert!(c.contains(ERROR_REAUTH), "{c}");
    let (s, _) = llamar(
        &p,
        &b,
        &tb,
        "/api/cambiar_modo",
        json!({ "real": true, "clave": null }),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(en_qbt(&p, CUMPLIDO));
    assert_eq!(antes, foto(&p).await);
}

#[tokio::test]
async fn sonda_t5_quitar_protegido_no_toca_avisos_ni_pedidos() {
    let p = prueba().await;
    let c = sesion(&p).await;
    let t = token(&p, &c).await;
    let db = &p.state.db;
    db::guardar_avisos(
        db,
        &[
            Aviso {
                hash: OBLIGADO.into(),
                regla: TipoRegla::Ratio,
                creado_en: 0,
                vence_en: DIA,
            },
            Aviso {
                hash: ESPERA.into(),
                regla: TipoRegla::Siembra,
                creado_en: 0,
                vence_en: DIA,
            },
        ],
    )
    .await
    .unwrap();
    db::pedir_borrado(
        db,
        &BorradoPedido {
            hash: ESPERA.into(),
            regla: TipoRegla::Siembra,
            pedido_en: 5,
            torrent: "{}".into(),
        },
    )
    .await
    .unwrap();
    let antes = foto(&p).await;
    for h in [OBLIGADO, ESPERA] {
        for f in [true, false] {
            let (s, cuerpo) = llamar(&p, &c, &t, "/api/quitar", quitar(h, f, Some(CLAVE))).await;
            assert_eq!(s, StatusCode::CONFLICT, "{cuerpo}");
        }
    }
    // La contraseña buena abrió la ventana: eso es lo único que cambia.
    sqlx::query("UPDATE sesiones SET reautenticado_hasta = NULL")
        .execute(db)
        .await
        .unwrap();
    assert_eq!(antes, foto(&p).await);
    assert!(borrados(&p).is_empty());
}

#[tokio::test]
async fn sonda_t5_contrasena_mala_no_abre_la_ventana_ni_cambia_el_modo() {
    let p = prueba().await;
    let c = sesion(&p).await;
    let t = token(&p, &c).await;
    let antes = foto(&p).await;
    let (s, _) = llamar(
        &p,
        &c,
        &t,
        "/api/quitar",
        quitar(CUMPLIDO, true, Some("mala")),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let (s, _) = llamar(
        &p,
        &c,
        &t,
        "/api/cambiar_modo",
        json!({ "real": true, "clave": "mala2" }),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    let (s, _) = llamar(
        &p,
        &c,
        &t,
        "/api/cambiar_modo",
        json!({ "real": true, "clave": "" }),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(antes, foto(&p).await);
    assert!(borrados(&p).is_empty());
    assert_eq!(db::modo(&p.state.db).await.unwrap(), Modo::SoloAviso);
}

#[tokio::test]
async fn sonda_t5_quitar_limpia_el_borrado_pedido_y_apunta() {
    let p = prueba().await;
    let c = sesion(&p).await;
    let t = token(&p, &c).await;
    let db = &p.state.db;
    db::pedir_borrado(
        db,
        &BorradoPedido {
            hash: CUMPLIDO.into(),
            regla: TipoRegla::Ratio,
            pedido_en: 5,
            torrent: "{}".into(),
        },
    )
    .await
    .unwrap();
    let (s, cuerpo) = llamar(
        &p,
        &c,
        &t,
        "/api/quitar",
        quitar(CUMPLIDO, false, Some(CLAVE)),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{cuerpo}");
    assert!(db::borrados_pedidos(db).await.unwrap().is_empty());
    assert_eq!(
        borrados(&p),
        vec![format!("hashes={CUMPLIDO}&deleteFiles=false")]
    );
    let h = db::historial(db, 10).await.unwrap();
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].evento, db::Evento::Quitado);
}

#[tokio::test]
async fn sonda_t5_hash_raro_o_ausente_no_borra_nada() {
    let p = prueba().await;
    let c = sesion(&p).await;
    let t = token(&p, &c).await;
    let antes = foto(&p).await;
    let mayus = CUMPLIDO.replace('1', "A");
    for (h, esperado) in [
        (mayus.as_str(), StatusCode::BAD_REQUEST),
        ("1111", StatusCode::BAD_REQUEST),
        ("../../x", StatusCode::BAD_REQUEST),
        (NO_ESTA, StatusCode::NOT_FOUND),
    ] {
        let (s, cuerpo) = llamar(&p, &c, &t, "/api/quitar", quitar(h, true, Some(CLAVE))).await;
        assert_eq!(s, esperado, "{h}: {cuerpo}");
    }
    sqlx::query("UPDATE sesiones SET reautenticado_hasta = NULL")
        .execute(&p.state.db)
        .await
        .unwrap();
    assert_eq!(antes, foto(&p).await);
    assert!(borrados(&p).is_empty());
}

#[tokio::test]
async fn sonda_t5_cuerpo_de_mas_de_16_kib_se_rechaza() {
    let p = prueba().await;
    let c = sesion(&p).await;
    let t = token(&p, &c).await;
    let antes = foto(&p).await;
    let grande = "x".repeat(17 * 1024);
    let (s, _) = llamar(
        &p,
        &c,
        &t,
        "/api/quitar",
        quitar(CUMPLIDO, true, Some(&grande)),
    )
    .await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(antes, foto(&p).await);
    assert!(borrados(&p).is_empty());
}

#[tokio::test]
async fn sonda_t5_logout_sin_token_no_cierra_la_sesion() {
    let p = prueba().await;
    let c = sesion(&p).await;
    let t = token(&p, &c).await;
    let salir = |tok: Option<&str>| {
        let mut req = post("/api/logout", "");
        req.headers_mut().insert("cookie", c.parse().unwrap());
        if let Some(tok) = tok {
            req.headers_mut()
                .insert("x-csrf-token", tok.parse().unwrap());
        }
        req
    };
    let r = enviar(&p.app, salir(None)).await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    let (s, _) = llamar(&p, &c, &t, "/api/portada", json!({})).await;
    assert_eq!(s, StatusCode::OK);
    let r = enviar(&p.app, salir(Some(&t))).await;
    assert!(r.status().is_success(), "{}", r.status());
    let (s, _) = llamar(&p, &c, &t, "/api/portada", json!({})).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sonda_t5_reglas_malas_no_se_guardan() {
    let p = prueba().await;
    let c = sesion(&p).await;
    let t = token(&p, &c).await;
    db::guardar_reglas(
        &p.state.db,
        "peliculas",
        &ReglasCategoria {
            ratio: Some(2.0),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let antes = foto(&p).await;
    for cuerpo in [
        json!({ "categoria": "peliculas", "reglas": { "ratio": 0.0 } }),
        json!({ "categoria": "peliculas", "reglas": { "siembra_dias": 0 } }),
        json!({ "categoria": "peliculas", "reglas": { "disco_libre_pct": 95.0 } }),
        json!({ "categoria": "otra", "reglas": { "ratio": 2.0 } }),
    ] {
        let (s, txt) = llamar(&p, &c, &t, "/api/guardar_reglas", cuerpo.clone()).await;
        assert!(s.is_client_error(), "{cuerpo}: {s} {txt}");
    }
    assert_eq!(antes, foto(&p).await);
}

#[test]
fn sonda_t5_ip_cliente() {
    use semillero::auth::ip_cliente;
    let proxy: IpAddr = "192.0.2.99".parse().unwrap();
    let ip = |s: &str| s.parse::<IpAddr>().unwrap();
    let con = |xs: &[&str]| {
        let mut h = HeaderMap::new();
        for x in xs {
            h.append("x-forwarded-for", x.parse().unwrap());
        }
        h
    };
    // Otro peer: la cabecera no cuenta.
    assert_eq!(
        ip_cliente(ip("198.51.100.7"), &con(&["203.0.113.5"]), Some(proxy)),
        ip("198.51.100.7")
    );
    // Del proxy: la última entrada.
    assert_eq!(
        ip_cliente(proxy, &con(&["198.51.100.11, 203.0.113.5"]), Some(proxy)),
        ip("203.0.113.5")
    );
    assert_eq!(
        ip_cliente(proxy, &con(&["198.51.100.11", "203.0.113.6"]), Some(proxy)),
        ip("203.0.113.6")
    );
    // Del proxy por IPv6 mapeada.
    assert_eq!(
        ip_cliente(ip("::ffff:192.0.2.99"), &con(&["203.0.113.7"]), Some(proxy)),
        ip("203.0.113.7")
    );
    // Basura o sin cabecera: la del proxy.
    assert_eq!(ip_cliente(proxy, &con(&["nada"]), Some(proxy)), proxy);
    assert_eq!(ip_cliente(proxy, &HeaderMap::new(), Some(proxy)), proxy);
    // Un cliente que añade una entrada al final de la suya no la cambia si
    // el proxy pone la suya al final (la izquierda no cuenta).
    assert_eq!(
        ip_cliente(proxy, &con(&["198.51.100.66, 203.0.113.8"]), Some(proxy)),
        ip("203.0.113.8")
    ); // Sin proxy configurado nadie es de confianza: vale la IP de la conexión.
    assert_eq!(ip_cliente(proxy, &con(&["203.0.113.9"]), None), proxy);
}

#[tokio::test]
async fn sonda_t5_reauth_mala_frena_a_su_ip_real_no_a_otra() {
    let p = prueba().await;
    let c = sesion(&p).await;
    let t = token(&p, &c).await;
    let proxy = format!("{PROXY}:443");
    let mut req = post_desde(
        "/api/quitar",
        &quitar(CUMPLIDO, true, Some("mala")).to_string(),
        &proxy,
        Some("203.0.113.5"),
    );
    let h = req.headers_mut();
    h.insert("content-type", "application/json".parse().unwrap());
    h.insert("cookie", c.parse().unwrap());
    h.insert("x-csrf-token", t.parse().unwrap());
    assert_eq!(enviar(&p.app, req).await.status(), StatusCode::FORBIDDEN);
    let login = |xff: &'static str| {
        post_desde(
            "/api/login",
            &cuerpo_login(USUARIO, CLAVE),
            &proxy,
            Some(xff),
        )
    };
    assert_eq!(
        enviar(&p.app, login("203.0.113.5")).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        enviar(&p.app, login("203.0.113.6")).await.status(),
        StatusCode::OK
    );
    assert!(borrados(&p).is_empty());
}

#[tokio::test]
async fn sonda_t5_pasada_y_borrado_a_mano_a_la_vez_un_solo_borrado() {
    for _ in 0..10 {
        let p = prueba().await;
        let c = sesion(&p).await;
        let t = token(&p, &c).await;
        let db = &p.state.db;
        db::fijar_modo(db, Modo::BorradoReal).await.unwrap();
        db::guardar_reglas(
            db,
            "peliculas",
            &ReglasCategoria {
                ratio: Some(1.0),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let ahora = semillero::tarea::ahora_unix();
        db::guardar_avisos(
            db,
            &[Aviso {
                hash: CUMPLIDO.into(),
                regla: TipoRegla::Ratio,
                creado_en: ahora - 2 * DIA,
                vence_en: ahora - DIA,
            }],
        )
        .await
        .unwrap();
        let red = EstadoRed {
            vigilada: true,
            vpn: true,
            puerto: Some(1),
        };
        let medir = |_: &std::path::Path| -> Option<reglas::Disco> { None };
        let (_inf, (s, cuerpo)) = tokio::join!(
            semillero::tarea::pasada(&p.state, &SinMediateca, &medir, ahora, &red, 0),
            llamar(
                &p,
                &c,
                &t,
                "/api/quitar",
                quitar(CUMPLIDO, true, Some(CLAVE))
            ),
        );
        let de_este: Vec<_> = borrados(&p)
            .into_iter()
            .filter(|b| b.contains(CUMPLIDO))
            .collect();
        assert_eq!(de_este.len(), 1, "{de_este:?} (quitar: {s} {cuerpo})");
        assert!(
            s == StatusCode::OK || s == StatusCode::NOT_FOUND,
            "{s} {cuerpo}"
        );
        assert!(!en_qbt(&p, CUMPLIDO));
        assert!(
            db::avisos(db)
                .await
                .unwrap()
                .iter()
                .all(|a| a.hash != CUMPLIDO)
        );
    }
}
