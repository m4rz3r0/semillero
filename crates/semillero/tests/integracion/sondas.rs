//! Probes from the independent verification of T3 (S8).

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use semillero::auth::{Canario, comprobar_canario};

use crate::comun::*;

fn con_cookie(mut req: Request<Body>, cookie: &str) -> Request<Body> {
    req.headers_mut().insert("cookie", cookie.parse().unwrap());
    req
}

fn set_cookie_id(resp: &axum::http::Response<Body>) -> String {
    resp.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}

#[tokio::test]
async fn login_ignores_attacker_chosen_cookie_and_closes_previous_session() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    // Fixation: an attacker-chosen id is never adopted.
    let fijada = "__Host-sm_session=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
    let resp = enviar(
        &p.app,
        con_cookie(post("/api/login", &cuerpo_login(USUARIO, CLAVE)), fijada),
    )
    .await;
    assert_eq!(resp.status(), 200);
    assert_ne!(set_cookie_id(&resp), fijada);
    assert!(
        enviar(&p.app, get("/", Some(fijada)))
            .await
            .status()
            .is_redirection()
    );
    // A second login from the same browser closes the first session.
    let vieja = set_cookie_id(&resp);
    let resp = enviar(
        &p.app,
        con_cookie(post("/api/login", &cuerpo_login(USUARIO, CLAVE)), &vieja),
    )
    .await;
    let nueva = set_cookie_id(&resp);
    assert_ne!(nueva, vieja);
    assert!(
        enviar(&p.app, get("/", Some(&vieja)))
            .await
            .status()
            .is_redirection()
    );
    assert_eq!(enviar(&p.app, get("/", Some(&nueva))).await.status(), 200);
}

#[tokio::test]
async fn csrf_token_of_another_session_is_rejected() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    let a = sesion(&p).await;
    let b = sesion(&p).await;
    let token_b = token(&p, &b).await;
    let mut req = con_cookie(post("/api/logout", ""), &a);
    req.headers_mut()
        .insert("x-csrf-token", token_b.parse().unwrap());
    assert_eq!(enviar(&p.app, req).await.status(), StatusCode::FORBIDDEN);
    // Two copies of the header (one good) are rejected too.
    let token_a = token(&p, &a).await;
    let mut req = con_cookie(post("/api/logout", ""), &a);
    req.headers_mut()
        .append("x-csrf-token", token_a.parse().unwrap());
    req.headers_mut()
        .append("x-csrf-token", "otro".parse().unwrap());
    assert_eq!(enviar(&p.app, req).await.status(), StatusCode::FORBIDDEN);
    assert_eq!(enviar(&p.app, get("/", Some(&a))).await.status(), 200);
}

#[tokio::test]
async fn post_without_session_is_401_and_get_login_does_not_log_in() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    assert_eq!(
        enviar(&p.app, post("/api/logout", "")).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let uri = format!("/api/login?{}", cuerpo_login(USUARIO, CLAVE));
    let resp = enviar(&p.app, get(&uri, None)).await;
    assert!(!resp.status().is_success(), "{}", resp.status());
    assert!(resp.headers().get("set-cookie").is_none());
    // Same-site (not same-origin) is not enough for a POST.
    let mut req = post("/api/login", &cuerpo_login(USUARIO, CLAVE));
    req.headers_mut()
        .insert("sec-fetch-site", "same-site".parse().unwrap());
    assert_eq!(enviar(&p.app, req).await.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn chunked_body_over_limit_is_413() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    let trozos: Vec<Result<axum::body::Bytes, std::io::Error>> = (0..20)
        .map(|_| Ok(axum::body::Bytes::from(vec![b'a'; 1024])))
        .collect();
    let cuerpo = Body::from_stream(futures_util::stream::iter(trozos));
    let mut req = post("/api/login", "");
    *req.body_mut() = cuerpo;
    req.headers_mut().remove("content-length");
    req.headers_mut()
        .insert("transfer-encoding", "chunked".parse().unwrap());
    assert_eq!(
        enviar(&p.app, req).await.status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(p.logins_en_qbt(), 0);
}

#[tokio::test]
async fn brake_is_not_bypassed_by_username_case_or_spaces() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    assert_eq!(
        login(&p, USUARIO, "mala").await.status(),
        StatusCode::UNAUTHORIZED
    );
    for u in ["ADMIN", "%20admin", "admin%20"] {
        assert_eq!(
            login(&p, u, CLAVE).await.status(),
            StatusCode::TOO_MANY_REQUESTS,
            "{u}"
        );
    }
}

#[tokio::test]
async fn canary_uses_random_user_and_password() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    assert_eq!(comprobar_canario(&p.state).await, Canario::Bueno);
    let ll = p.sim().control.llamadas();
    let l = ll.iter().find(|l| l.ruta == "/api/v2/auth/login").unwrap();
    assert!(
        !l.cuerpo.contains(USUARIO) && !l.cuerpo.contains(CLAVE),
        "{}",
        l.cuerpo
    );
}

#[tokio::test]
async fn canary_bad_blocks_even_the_right_password_without_asking_again() {
    let p = con_sim(qbt_sim::Conf {
        lista_blanca: vec![[127, 0, 0, 1].into()],
        ..Default::default()
    })
    .await;
    assert_eq!(comprobar_canario(&p.state).await, Canario::Malo);
    let antes = p.logins_en_qbt();
    let resp = login(&p, USUARIO, CLAVE).await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(resp.headers().get("set-cookie").is_none());
    assert_eq!(p.logins_en_qbt(), antes);
}

#[tokio::test]
async fn banned_ip_is_never_reported_as_wrong_password_and_canary_retries() {
    let p = con_sim(qbt_sim::Conf {
        baneado: true,
        ..Default::default()
    })
    .await;
    assert_eq!(comprobar_canario(&p.state).await, Canario::Pendiente);
    let txt = texto(login(&p, USUARIO, CLAVE).await).await;
    assert!(!txt.contains("incorrect"), "{txt}");
    p.sim().control.banear(false);
    assert_eq!(login(&p, USUARIO, CLAVE).await.status(), 200);
    assert_eq!(*p.state.acceso.lock().await, Canario::Bueno);
}

#[derive(Clone, Default)]
struct Buf(Arc<Mutex<Vec<u8>>>);
impl std::io::Write for Buf {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn no_password_or_session_id_in_sqlite_or_logs() {
    let buf = Buf::default();
    let b2 = buf.clone();
    let sub = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_writer(move || b2.clone())
        .with_ansi(false)
        .finish();
    tracing::subscriber::set_global_default(sub).unwrap();
    let p = con_sim(qbt_sim::Conf::default()).await;
    comprobar_canario(&p.state).await;
    let _ = login(&p, USUARIO, "ClaveMalaUnica123").await;
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let cookie = sesion(&p).await;
    let id = cookie.split_once('=').unwrap().1.to_owned();
    // An action that logs, done with the session and its CSRF token.
    let csrf = token(&p, &cookie).await;
    use crate::interfaz::api;
    use serde_json::json;
    let resp = enviar(
        &p.app,
        api(
            "/api/cambiar_modo",
            json!({ "real": true, "clave": null }),
            Some(&cookie),
            Some(&csrf),
        ),
    )
    .await;
    assert_eq!(resp.status(), 403, "no recent password");
    let resp = enviar(
        &p.app,
        api(
            "/api/cambiar_modo",
            json!({ "real": true, "clave": CLAVE }),
            Some(&cookie),
            Some(&csrf),
        ),
    )
    .await;
    assert_eq!(resp.status(), 200);
    let logs = String::from_utf8(buf.0.lock().unwrap().clone()).unwrap();
    assert!(
        logs.contains("sesión iniciada")
            && logs.contains("login rechazado")
            && logs.contains("modo cambiado"),
        "logs not captured: {logs}"
    );
    for s in [CLAVE, "ClaveMalaUnica123", id.as_str(), csrf.as_str()] {
        assert!(!logs.contains(s), "{s} in logs");
    }
    // No user names or client addresses (personal data) either.
    for campo in ["usuario=", "desde="] {
        assert!(!logs.contains(campo), "{campo} in logs");
    }
    let ruta: String =
        sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name='main'")
            .fetch_one(&p.state.db)
            .await
            .unwrap();
    sqlx::query("PRAGMA wal_checkpoint(FULL)")
        .execute(&p.state.db)
        .await
        .unwrap();
    let mut todo = Vec::new();
    for e in std::fs::read_dir(std::path::Path::new(&ruta).parent().unwrap()).unwrap() {
        let e = e.unwrap().path();
        if e.is_file() {
            todo.extend(std::fs::read(e).unwrap());
        }
    }
    let todo = String::from_utf8_lossy(&todo);
    for s in [CLAVE, "ClaveMalaUnica123", id.as_str()] {
        assert!(!todo.contains(s), "{s} in sqlite files");
    }
}

#[tokio::test]
async fn idle_session_expires() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    let c = sesion(&p).await;
    sqlx::query("UPDATE sesiones SET vista = vista - 1801")
        .execute(&p.state.db)
        .await
        .unwrap();
    assert!(
        enviar(&p.app, get("/", Some(&c)))
            .await
            .status()
            .is_redirection()
    );
    let c = sesion(&p).await;
    sqlx::query("UPDATE sesiones SET creada = creada - 43201")
        .execute(&p.state.db)
        .await
        .unwrap();
    assert!(
        enviar(&p.app, get("/", Some(&c)))
            .await
            .status()
            .is_redirection()
    );
}

#[tokio::test]
async fn protected_page_not_served_by_fallback_without_session() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    for uri in [
        "//",
        "/index.html",
        "/./",
        "/%2F",
        "/?x=1",
        "/login/..",
        "/LOGIN",
    ] {
        let resp = enviar(&p.app, get(uri, None)).await;
        let st = resp.status();
        let html = texto(resp).await;
        assert!(!html.contains("csrf-token"), "{uri}");
        assert!(
            !(st == 200 && html.contains("<h1>Semillero</h1>") && !html.contains("contrasena")),
            "{uri} -> {st} renders the home page"
        );
    }
}

#[tokio::test]
async fn csp_nonce_changes_per_page() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    let a = enviar(&p.app, get("/login", None)).await;
    let b = enviar(&p.app, get("/login", None)).await;
    let ca = a.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .to_owned();
    let cb = b.headers()["content-security-policy"]
        .to_str()
        .unwrap()
        .to_owned();
    assert!(ca.contains("'nonce-"), "{ca}");
    assert_ne!(ca, cb);
    assert!(!ca.contains("unsafe-inline"));
}
