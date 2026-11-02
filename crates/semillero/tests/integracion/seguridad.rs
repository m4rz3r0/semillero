//! CSRF, guardián de `/api`, límite de tamaño, CSP con nonce y `/salud`.

use axum::body::Body;
use axum::http::{Request, StatusCode};

use crate::comun::*;

/// `POST /api/logout` con la sesión `cookie` y las cabeceras `extra`.
fn logout(cookie: Option<&str>, extra: &[(&str, &str)]) -> Request<Body> {
    let mut req = post("/api/logout", "");
    let h = req.headers_mut();
    h.remove("origin");
    h.remove("sec-fetch-site");
    if let Some(c) = cookie {
        h.insert("cookie", c.parse().unwrap());
    }
    for (k, v) in extra {
        h.append(
            axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
            v.parse().unwrap(),
        );
    }
    req
}

#[tokio::test]
async fn csrf_exige_origen_y_token() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    let cookie = sesion(&p).await;
    let token = token(&p, &cookie).await;
    let c = Some(cookie.as_str());
    let casos: [&[(&str, &str)]; 6] = [
        // Sin origen ni Sec-Fetch-Site, aunque traiga el token.
        &[("x-csrf-token", &token)],
        &[("origin", "https://otra.example"), ("x-csrf-token", &token)],
        &[("origin", "null"), ("x-csrf-token", &token)],
        &[("sec-fetch-site", "cross-site"), ("x-csrf-token", &token)],
        // Del propio origen, pero sin token o con uno falso.
        &[("origin", ORIGEN)],
        &[("origin", ORIGEN), ("x-csrf-token", "falso")],
    ];
    for extra in casos {
        let resp = enviar(&p.app, logout(c, extra)).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN, "{extra:?}");
    }
    // La sesión sigue viva.
    assert_eq!(enviar(&p.app, get("/", c)).await.status(), 200);
    // Con origen y token, sí.
    let resp = enviar(
        &p.app,
        logout(c, &[("origin", ORIGEN), ("x-csrf-token", &token)]),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let caducada = resp.headers()["set-cookie"].to_str().unwrap();
    assert!(caducada.contains("Max-Age=0"), "{caducada}");
    assert!(enviar(&p.app, get("/", c)).await.status().is_redirection());
}

#[tokio::test]
async fn el_login_tambien_exige_el_propio_origen() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    for (k, v) in [
        ("origin", "https://otra.example"),
        ("sec-fetch-site", "cross-site"),
    ] {
        let mut req = post("/api/login", &cuerpo_login(USUARIO, CLAVE));
        req.headers_mut().remove("origin");
        req.headers_mut().remove("sec-fetch-site");
        req.headers_mut().insert(k, v.parse().unwrap());
        assert_eq!(enviar(&p.app, req).await.status(), StatusCode::FORBIDDEN);
    }
    let mut req = post("/api/login", &cuerpo_login(USUARIO, CLAVE));
    req.headers_mut().remove("origin");
    req.headers_mut().remove("sec-fetch-site");
    assert_eq!(enviar(&p.app, req).await.status(), StatusCode::FORBIDDEN);
    assert_eq!(p.logins_en_qbt(), 0, "rechazado antes de preguntar");
}

#[tokio::test]
async fn el_guardian_de_api_exige_sesion() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    // Con origen bueno pero sin sesión.
    let resp = enviar(&p.app, post("/api/logout", "")).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    // Cualquier ruta de /api, exista o no, y también GET.
    for ruta in ["/api/nada", "/api/logout"] {
        let resp = enviar(&p.app, get(ruta, None)).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "{ruta}");
    }
    let resp = enviar(
        &p.app,
        get("/api/nada", Some("__Host-sm_session=inventada")),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn limite_de_tamano() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    let grande = format!(
        "{}&relleno={}",
        cuerpo_login(USUARIO, CLAVE),
        "x".repeat(20_000)
    );
    let resp = enviar(&p.app, post("/api/login", &grande)).await;
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(p.logins_en_qbt(), 0);
    // Sin Content-Length (cuerpo por trozos), también.
    let trozos: Vec<Result<Vec<u8>, std::io::Error>> = grande
        .as_bytes()
        .chunks(1024)
        .map(|c| Ok(c.to_vec()))
        .collect();
    let mut req = post("/api/login", "");
    *req.body_mut() = Body::from_stream(futures_util::stream::iter(trozos));
    assert!(req.headers().get("content-length").is_none());
    let resp = enviar(&p.app, req).await;
    assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn las_paginas_llevan_csp_con_nonce() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    let resp = enviar(&p.app, get("/login", None)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let csp = resp.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(resp.headers()["x-frame-options"], "DENY");
    let i = csp.find("'nonce-").expect("CSP sin nonce") + "'nonce-".len();
    let nonce = &csp[i..i + csp[i..].find('\'').unwrap()];
    assert!(nonce.len() >= 16, "{nonce}");
    assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
    let html = texto(resp).await;
    assert!(html.contains(&format!("nonce=\"{nonce}\"")), "{html}");
    // Sin sesión no hay token en la página.
    assert!(!html.contains("csrf-token"));
}

/// `GET /salud` desde la conexión `peer`, con `X-Forwarded-For: xff` si se da.
async fn salud_desde(p: &Prueba, peer: &str, xff: Option<&str>) -> serde_json::Value {
    let mut req = get("/salud", None);
    let peer: std::net::SocketAddr = peer.parse().unwrap();
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(peer));
    if let Some(x) = xff {
        req.headers_mut()
            .insert("x-forwarded-for", x.parse().unwrap());
    }
    let resp = enviar(&p.app, req).await;
    assert_eq!(resp.status(), StatusCode::OK);
    serde_json::from_str(&texto(resp).await).unwrap()
}

#[tokio::test]
async fn salud_dice_la_version_solo_en_local() {
    let p = con_sim_y(qbt_sim::Conf::default(), |c| {
        c.escucha = "192.0.2.10:8090".parse().unwrap();
        c.proxy = Some("192.0.2.20".parse().unwrap());
    })
    .await;
    let version = serde_json::json!({ "ok": true, "version": env!("CARGO_PKG_VERSION") });
    let sin_version = serde_json::json!({ "ok": true });
    // El propio equipo y la dirección de escucha (el actualizador).
    assert_eq!(salud_desde(&p, "127.0.0.1:5000", None).await, version);
    assert_eq!(salud_desde(&p, "[::1]:5000", None).await, version);
    assert_eq!(salud_desde(&p, "192.0.2.10:5000", None).await, version);
    // Cualquier otro, solo que está vivo.
    assert_eq!(
        salud_desde(&p, "198.51.100.7:5000", None).await,
        sin_version
    );
    // Lo que llega por el proxy es de fuera, aunque diga ser local.
    assert_eq!(salud_desde(&p, "192.0.2.20:5000", None).await, sin_version);
    assert_eq!(
        salud_desde(&p, "127.0.0.1:5000", Some("127.0.0.1")).await,
        sin_version
    );
}

#[tokio::test]
async fn hsts_solo_con_origen_https() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    let host = ORIGEN.trim_start_matches("https://");
    let mut req = get("/login", None);
    req.headers_mut().insert("host", host.parse().unwrap());
    let resp = enviar(&p.app, req).await;
    assert_eq!(
        resp.headers()["strict-transport-security"],
        "max-age=31536000"
    );
    // Por otro nombre (el acceso local por http), sin HSTS.
    let mut req = get("/login", None);
    req.headers_mut()
        .insert("host", "127.0.0.1:8090".parse().unwrap());
    let resp = enviar(&p.app, req).await;
    assert!(resp.headers().get("strict-transport-security").is_none());
    // Con los orígenes por defecto (http), nunca.
    let p = con_sim_y(qbt_sim::Conf::default(), |c| {
        c.origenes = vec!["http://127.0.0.1:8090".into()];
    })
    .await;
    let mut req = get("/login", None);
    req.headers_mut()
        .insert("host", "127.0.0.1:8090".parse().unwrap());
    let resp = enviar(&p.app, req).await;
    assert!(resp.headers().get("strict-transport-security").is_none());
}

#[tokio::test]
async fn cabeceras_de_seguridad_en_todo() {
    let p = sin_qbt().await;
    for ruta in ["/login", "/salud", "/pkg/semillero.css", "/no-existe"] {
        let resp = enviar(&p.app, get(ruta, None)).await;
        let h = resp.headers();
        assert_eq!(h["x-content-type-options"], "nosniff", "{ruta}");
        assert_eq!(h["referrer-policy"], "same-origin", "{ruta}");
        assert_eq!(h["x-frame-options"], "DENY", "{ruta}");
        let csp = h["content-security-policy"].to_str().unwrap();
        assert!(csp.contains("frame-ancestors 'none'"), "{ruta}: {csp}");
        assert!(csp.contains("default-src 'self'"), "{ruta}: {csp}");
        assert!(!csp.contains("unsafe-inline"), "{ruta}: {csp}");
    }
}

#[tokio::test]
async fn todos_los_scripts_de_la_pagina_llevan_el_nonce() {
    // Con `script-src` sin `unsafe-inline`, un script en línea sin el nonce
    // no se ejecutaría y la hidratación no arrancaría.
    let p = con_sim(qbt_sim::Conf {
        api_abierta: true,
        ..Default::default()
    })
    .await;
    let cookie = sesion(&p).await;
    for (ruta, cookie) in [("/login", None), ("/", Some(cookie.as_str()))] {
        let resp = enviar(&p.app, get(ruta, cookie)).await;
        assert_eq!(resp.status(), StatusCode::OK, "{ruta}");
        let csp = resp.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .to_owned();
        let i = csp.find("'nonce-").unwrap() + "'nonce-".len();
        let nonce = &csp[i..i + csp[i..].find('\'').unwrap()];
        let html = texto(resp).await;
        let scripts = html.matches("<script").count();
        assert!(scripts > 0, "{ruta}: sin scripts de hidratación");
        assert_eq!(
            html.matches(&format!("nonce=\"{nonce}\"")).count(),
            scripts,
            "{ruta}: {html}"
        );
        // `style-src 'self'` no admite estilos en línea.
        assert!(!html.contains(" style=\""), "{ruta}");
    }
}

#[tokio::test]
async fn salud_sin_login() {
    let p = sin_qbt().await;
    let resp = enviar(&p.app, get("/salud", None)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let v: serde_json::Value = serde_json::from_str(&texto(resp).await).unwrap();
    assert_eq!(v["ok"], true);
    // `get` llega desde 127.0.0.1: el propio equipo ve la versión.
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn sin_sesion_ninguna_variante_de_ruta_ensena_datos() {
    // Rutas que el router de axum no reconoce pero el de Leptos sí: antes
    // caían en el `fallback`, sin el control de sesión de las páginas.
    let p = con_sim(qbt_sim::Conf {
        api_abierta: true,
        ..Default::default()
    })
    .await;
    p.sim().control.fijar_torrents(vec![serde_json::json!({
        "hash": "abababababababababababababababababababab",
        "name": "NombreQueNoDebeVerse", "category": "peliculas", "state": "stalledUP",
        "downloaded": 1000, "total_size": 1000, "uploaded": 0, "seeding_time": 0,
        "ratio": 0.0, "added_on": 1_700_000_000, "save_path": "/media/peliculas",
        "upspeed": 0, "dlspeed": 0, "progress": 1.0
    })]);
    for ruta in [
        "/",
        "/./",
        "/x/../",
        "/reglas",
        "/reglas/",
        "/historial/",
        "/historial//",
        "/pkg/../",
        "/fuentes/../reglas",
        "/no-existe",
    ] {
        let resp = enviar(&p.app, get(ruta, None)).await;
        let estado = resp.status();
        let html = texto(resp).await;
        assert!(
            !html.contains("NombreQueNoDebeVerse") && !html.contains("abababab"),
            "{ruta} enseña un torrent sin sesión ({estado})"
        );
        assert!(
            !html.contains("Normas del tracker"),
            "{ruta} enseña las reglas"
        );
    }
    // La web compilada y las fuentes siguen públicas (las necesita /login).
    for ruta in ["/pkg/semillero.js", "/fuentes/a.woff2"] {
        let resp = enviar(&p.app, get(ruta, None)).await;
        assert_ne!(resp.status(), StatusCode::SEE_OTHER, "{ruta}");
    }
    // Con sesión, la portada sigue enseñándolo.
    let cookie = sesion(&p).await;
    let html = texto(enviar(&p.app, get("/", Some(&cookie))).await).await;
    assert!(html.contains("NombreQueNoDebeVerse"));
}
