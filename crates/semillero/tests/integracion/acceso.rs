//! Acceso con la cuenta de qBittorrent: mensajes, cookie, freno, canario y
//! reautenticación.

use std::net::IpAddr;

use axum::http::{HeaderMap, StatusCode};
use semillero::auth::{
    self, Canario, ErrorLogin, comprobar_canario, reautenticacion_vigente, reautenticar,
    sesion_de_cabeceras,
};

use crate::comun::*;

const LOCAL: IpAddr = IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);

#[tokio::test]
async fn entra_con_la_cuenta_de_qbittorrent() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    let resp = login(&p, USUARIO, CLAVE).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let cookie = resp.headers()["set-cookie"].to_str().unwrap().to_owned();
    assert!(cookie.starts_with("__Host-sm_session="), "{cookie}");
    // La comprobación sale desde 127.0.0.1, no desde la IP de la lista blanca.
    assert!(
        p.sim()
            .control
            .llamadas()
            .iter()
            .filter(|l| l.ruta == "/api/v2/auth/login")
            .all(|l| l.desde == LOCAL)
    );
}

#[tokio::test]
async fn la_cookie_de_sesion_es_segura_y_no_se_guarda_tal_cual() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    let resp = login(&p, USUARIO, CLAVE).await;
    let cookie = resp.headers()["set-cookie"].to_str().unwrap().to_owned();
    let partes: Vec<&str> = cookie.split(';').map(str::trim).collect();
    for atributo in ["HttpOnly", "Secure", "SameSite=Strict", "Path=/"] {
        assert!(partes.contains(&atributo), "falta {atributo}: {cookie}");
    }
    assert!(!partes.iter().any(|p| p.starts_with("Domain")), "{cookie}");
    let id = partes[0].strip_prefix("__Host-sm_session=").unwrap();
    assert!(id.len() >= 43, "id corto: {id}");
    // En la base de datos está solo su SHA-256, y ninguna contraseña.
    let filas: Vec<(String, String)> = sqlx::query_as("SELECT id, usuario FROM sesiones")
        .fetch_all(&p.state.db)
        .await
        .unwrap();
    assert_eq!(filas.len(), 1);
    assert_ne!(filas[0].0, id);
    assert_eq!(filas[0].1, USUARIO);
    let columnas: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('sesiones')")
            .fetch_all(&p.state.db)
            .await
            .unwrap();
    assert!(
        !columnas
            .iter()
            .any(|c| c.contains("clave") || c.contains("pass"))
    );
    // Con la cookie, la portada; sin ella, al formulario.
    let r = enviar(&p.app, get("/", Some(partes[0]))).await;
    assert_eq!(r.status(), StatusCode::OK);
    let r = enviar(&p.app, get("/", None)).await;
    assert!(r.status().is_redirection());
    assert_eq!(r.headers()["location"], "/login");
}

#[tokio::test]
async fn contrasena_mala() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    let resp = login(&p, USUARIO, "mala").await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert!(resp.headers().get("set-cookie").is_none());
    let cuerpo = texto(resp).await;
    assert!(
        cuerpo.contains("Usuario o contraseña incorrectos"),
        "{cuerpo}"
    );
}

#[tokio::test]
async fn qbittorrent_apagado_no_es_contrasena_mala() {
    let p = sin_qbt().await;
    let resp = login(&p, USUARIO, CLAVE).await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    let cuerpo = texto(resp).await;
    assert!(cuerpo.contains("qBittorrent no responde"), "{cuerpo}");
    assert!(!cuerpo.contains("incorrectos"), "{cuerpo}");
    // No cuenta como fallo: el siguiente intento no está frenado.
    let resp = login(&p, USUARIO, CLAVE).await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn ip_bloqueada_por_qbittorrent() {
    let p = con_sim(qbt_sim::Conf {
        baneado: true,
        ..Default::default()
    })
    .await;
    let resp = login(&p, USUARIO, CLAVE).await;
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
    let cuerpo = texto(resp).await;
    assert!(cuerpo.contains("qBittorrent rechazó"), "{cuerpo}");
    assert!(!cuerpo.contains("incorrectos"), "{cuerpo}");
}

#[tokio::test]
async fn el_freno_para_los_intentos_seguidos() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    assert_eq!(login(&p, USUARIO, "mala").await.status(), 401);
    let antes = p.logins_en_qbt();
    // Recién fallado, ni siquiera la buena: hay que esperar, y no se pregunta
    // a qBittorrent.
    let resp = login(&p, USUARIO, CLAVE).await;
    assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
    let cuerpo = texto(resp).await;
    assert!(cuerpo.contains("Demasiados intentos"), "{cuerpo}");
    assert_eq!(p.logins_en_qbt(), antes);
    // Pasada la espera (1 s tras un fallo), entra.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    assert_eq!(login(&p, USUARIO, CLAVE).await.status(), 200);
}

#[test]
fn espera_del_freno() {
    assert_eq!(auth::espera_s(0), 0);
    assert_eq!(auth::espera_s(1), 1);
    assert_eq!(auth::espera_s(3), 4);
    assert_eq!(auth::espera_s(7), 60);
    assert_eq!(auth::espera_s(100), 60);
}

#[tokio::test]
async fn el_canario_cierra_el_acceso_si_qbittorrent_acepta_cualquier_clave() {
    // 127.0.0.1 en la lista blanca: qBittorrent acepta cualquier contraseña.
    let p = con_sim(qbt_sim::Conf {
        lista_blanca: vec![LOCAL],
        ..Default::default()
    })
    .await;
    assert_eq!(comprobar_canario(&p.state).await, Canario::Malo);
    assert_eq!(p.logins_en_qbt(), 1);
    for clave in [CLAVE, "cualquiera"] {
        let resp = login(&p, USUARIO, clave).await;
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(resp.headers().get("set-cookie").is_none());
        let cuerpo = texto(resp).await;
        assert!(cuerpo.contains("mal configurado"), "{cuerpo}");
    }
    // No se repite: solo la comprobación del arranque.
    assert_eq!(p.logins_en_qbt(), 1);
}

#[tokio::test]
async fn el_canario_tambien_se_comprueba_en_el_primer_login() {
    let p = con_sim(qbt_sim::Conf {
        lista_blanca: vec![LOCAL],
        ..Default::default()
    })
    .await;
    let resp = login(&p, USUARIO, "cualquiera").await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(texto(resp).await.contains("mal configurado"));
}

#[tokio::test]
async fn el_canario_bueno_se_comprueba_una_sola_vez() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    assert_eq!(comprobar_canario(&p.state).await, Canario::Bueno);
    assert_eq!(comprobar_canario(&p.state).await, Canario::Bueno);
    assert_eq!(p.logins_en_qbt(), 1);
    sesion(&p).await;
    sesion(&p).await;
    assert_eq!(p.logins_en_qbt(), 3);
}

#[tokio::test]
async fn el_canario_sin_comprobar_se_repite_en_el_siguiente_login() {
    let p = con_sim(qbt_sim::Conf {
        baneado: true,
        ..Default::default()
    })
    .await;
    assert_eq!(comprobar_canario(&p.state).await, Canario::Pendiente);
    assert_eq!(p.logins_en_qbt(), 1);
    p.sim().control.banear(false);
    // El login repite el canario y después comprueba la cuenta.
    sesion(&p).await;
    assert_eq!(p.logins_en_qbt(), 3);
    assert_eq!(*p.state.acceso.lock().await, Canario::Bueno);
}

#[tokio::test]
async fn reautenticacion_para_lo_delicado() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    let cookie = sesion(&p).await;
    let mut cabeceras = HeaderMap::new();
    cabeceras.insert("cookie", cookie.parse().unwrap());
    let s = sesion_de_cabeceras(&p.state.db, &cabeceras)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(s.usuario, USUARIO);
    assert!(!reautenticacion_vigente(&p.state.db, &s).await.unwrap());
    let antes = p.logins_en_qbt();
    reautenticar(&p.state, LOCAL, &s, CLAVE).await.unwrap();
    assert_eq!(p.logins_en_qbt(), antes + 1, "se pregunta a qBittorrent");
    assert!(reautenticacion_vigente(&p.state.db, &s).await.unwrap());
    assert_eq!(
        reautenticar(&p.state, LOCAL, &s, "mala").await,
        Err(ErrorLogin::Credenciales)
    );
}

// ---------------------------------------------------------------- modo con contraseña

/// semillero con la contraseña de qBittorrent en un fichero (en `dir`,
/// fuera de la copia de trabajo) contra un simulador con `conf`, sin
/// `SEMILLERO_LOGIN_DESDE`: las comprobaciones no se atan a ninguna IP (con
/// qBittorrent en otra máquina, atarlas a 127.0.0.1 daba EINVAL y un 503).
async fn con_contrasena(conf: qbt_sim::Conf, dir: &std::path::Path) -> Prueba {
    con_clave_en_fichero(conf, dir, CLAVE).await
}

/// Como [`con_contrasena`], con `clave` en el fichero.
async fn con_clave_en_fichero(conf: qbt_sim::Conf, dir: &std::path::Path, clave: &str) -> Prueba {
    let fichero = dir.join("qbt.clave");
    std::fs::write(&fichero, format!("{clave}\n")).unwrap();
    con_sim_y(conf, |c| {
        c.login_desde = None;
        c.cuenta_qbt = Some(semillero::config::ConfCuentaQbt {
            usuario: USUARIO.into(),
            clave: fichero,
        });
    })
    .await
}

async fn portada(p: &Prueba) -> serde_json::Value {
    let cookie = sesion(p).await;
    let token = token(p, &cookie).await;
    let resp = enviar(
        &p.app,
        crate::interfaz::api(
            "/api/portada",
            serde_json::json!({}),
            Some(&cookie),
            Some(&token),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    serde_json::from_str(&texto(resp).await).unwrap()
}

fn torrent(hash: &str) -> serde_json::Value {
    serde_json::json!({
        "hash": hash, "name": "Una película", "category": "peliculas", "state": "uploading",
        "downloaded": 1000, "total_size": 1000, "uploaded": 1500, "seeding_time": 700000,
        "ratio": 1.5, "added_on": 1700000000, "save_path": "/media/peliculas",
        "upspeed": 0, "dlspeed": 0, "progress": 1.0
    })
}

#[tokio::test]
async fn con_contrasena_funciona_sin_lista_blanca() {
    let dir = tempfile::tempdir().unwrap();
    // Sin lista blanca: la API contesta 403 sin sesión.
    let p = con_contrasena(qbt_sim::Conf::default(), dir.path()).await;
    p.sim()
        .control
        .fijar_torrents(vec![torrent(&"a".repeat(40))]);
    assert_eq!(comprobar_canario(&p.state).await, Canario::Bueno);
    let d = portada(&p).await;
    assert_eq!(d["franja"]["qbt"], true, "{d}");
    assert_eq!(d["torrents"].as_array().unwrap().len(), 1, "{d}");
    // Se entra con la contraseña buena, y la mala sigue sin valer.
    sesion(&p).await;
    assert_eq!(
        login(&p, USUARIO, "mala").await.status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn con_la_contrasena_del_servicio_equivocada_la_portada_lo_dice() {
    let dir = tempfile::tempdir().unwrap();
    let p = con_clave_en_fichero(qbt_sim::Conf::default(), dir.path(), "caducada").await;
    p.sim()
        .control
        .fijar_torrents(vec![torrent(&"a".repeat(40))]);
    // Quien entra en la web lo hace con su propia contraseña, que sí vale.
    let d = portada(&p).await;
    assert_eq!(d["franja"]["qbt"], false, "{d}");
    assert_eq!(d["franja"]["qbt_clave_mala"], true, "{d}");
    let cookie = sesion(&p).await;
    let html = texto(enviar(&p.app, get("/", Some(&cookie))).await).await;
    assert!(
        html.contains("qBittorrent no acepta la contraseña de semillero"),
        "{html}"
    );
    assert!(html.contains("semillero configurar"), "{html}");
    assert!(!html.contains("qBittorrent no responde"), "{html}");
}

#[tokio::test]
async fn si_qbittorrent_falla_por_otra_cosa_la_portada_sigue_diciendo_que_no_responde() {
    let dir = tempfile::tempdir().unwrap();
    let p = con_contrasena(qbt_sim::Conf::default(), dir.path()).await;
    let cookie = sesion(&p).await;
    let tok = token(&p, &cookie).await;
    p.sim().control.olvidar_sesiones();
    p.sim().control.banear(true);
    let resp = enviar(
        &p.app,
        crate::interfaz::api(
            "/api/portada",
            serde_json::json!({}),
            Some(&cookie),
            Some(&tok),
        ),
    )
    .await;
    let d: serde_json::Value = serde_json::from_str(&texto(resp).await).unwrap();
    assert_eq!(d["franja"]["qbt"], false, "{d}");
    assert_eq!(d["franja"]["qbt_clave_mala"], false, "{d}");
    let html = texto(enviar(&p.app, get("/", Some(&cookie))).await).await;
    assert!(html.contains("qBittorrent no responde"), "{html}");
    assert!(!html.contains("no acepta la contraseña"), "{html}");
}

#[tokio::test]
async fn con_contrasena_y_lista_blanca_el_canario_sigue_cerrando_el_acceso() {
    let dir = tempfile::tempdir().unwrap();
    let p = con_contrasena(
        qbt_sim::Conf {
            lista_blanca: vec![LOCAL],
            ..Default::default()
        },
        dir.path(),
    )
    .await;
    assert_eq!(comprobar_canario(&p.state).await, Canario::Malo);
    let resp = login(&p, USUARIO, CLAVE).await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(texto(resp).await.contains("mal configurado"));
}

#[tokio::test]
async fn con_contrasena_un_fichero_que_falta_impide_arrancar() {
    let dir = tempfile::tempdir().unwrap();
    let sim = qbt_sim::arrancar(qbt_sim::Conf::default()).await;
    let mut cfg = semillero::config::Config::desde(|_| None).unwrap();
    cfg.qbt = sim.url.clone();
    cfg.db = dir.path().join("semillero.db");
    cfg.cuenta_qbt = Some(semillero::config::ConfCuentaQbt {
        usuario: USUARIO.into(),
        clave: dir.path().join("no-esta.clave"),
    });
    let leptos = leptos::prelude::LeptosOptions::builder()
        .output_name("semillero")
        .build();
    let e = semillero::estado::AppState::nuevo(cfg, leptos)
        .await
        .err()
        .unwrap();
    assert!(e.contains("no-esta.clave"), "{e}");
}

/// Sin `SEMILLERO_QBT_CLAVE`, como siempre: la API va sin sesión y
/// semillero no entra nunca en qBittorrent (los logins que recibe son solo
/// las comprobaciones de contraseña, desde 127.0.0.1).
#[tokio::test]
async fn sin_contrasena_la_api_va_sin_sesion_como_siempre() {
    let p = con_sim(qbt_sim::Conf {
        api_abierta: true,
        ..Default::default()
    })
    .await;
    p.sim()
        .control
        .fijar_torrents(vec![torrent(&"a".repeat(40))]);
    let d = portada(&p).await;
    assert_eq!(d["franja"]["qbt"], true, "{d}");
    // El canario y el login de `sesion`: nada más.
    assert_eq!(p.logins_en_qbt(), 2);
    assert!(
        p.sim()
            .control
            .llamadas()
            .iter()
            .filter(|l| l.ruta == "/api/v2/auth/login")
            .all(|l| l.desde == LOCAL && l.cuerpo != format!("username={USUARIO}&password"))
    );
    // Con la API cerrada, un 403 se queda en 403: no se intenta entrar.
    let p = con_sim(qbt_sim::Conf::default()).await;
    let d = portada(&p).await;
    assert_eq!(d["franja"]["qbt"], false, "{d}");
    assert_eq!(p.logins_en_qbt(), 2);
}
