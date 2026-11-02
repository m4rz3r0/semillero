//! Freno de intentos de acceso por la IP real del cliente: la que manda
//! el proxy en `X-Forwarded-For`, creída solo si la conexión viene del proxy.

use axum::http::StatusCode;

use crate::comun::*;

/// Login desde la conexión `peer` con `X-Forwarded-For: xff`.
async fn login_desde(p: &Prueba, peer: &str, xff: Option<&str>, clave: &str) -> StatusCode {
    let req = post_desde("/api/login", &cuerpo_login(USUARIO, clave), peer, xff);
    enviar(&p.app, req).await.status()
}

fn por_el_proxy() -> String {
    format!("{PROXY}:443")
}

#[tokio::test]
async fn x_forwarded_for_falso_de_otra_ip_no_cuenta() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    // Un cliente que no es el proxy falla diciendo ser 203.0.113.5...
    assert_eq!(
        login_desde(&p, "198.51.100.7:4000", Some("203.0.113.5"), "mala").await,
        StatusCode::UNAUTHORIZED
    );
    // ...pero el freno es para su IP de verdad, no para la que dijo.
    assert_eq!(
        login_desde(&p, &por_el_proxy(), Some("203.0.113.5"), CLAVE).await,
        StatusCode::OK
    );
    assert_eq!(
        login_desde(&p, "198.51.100.7:4001", Some("203.0.113.9"), CLAVE).await,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn detras_del_proxy_cada_ip_real_tiene_su_freno() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    let proxy = por_el_proxy();
    assert_eq!(
        login_desde(&p, &proxy, Some("203.0.113.40"), "mala").await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login_desde(&p, &proxy, Some("203.0.113.40"), CLAVE).await,
        StatusCode::TOO_MANY_REQUESTS
    );
    // Otra IP real detrás del mismo proxy entra a la primera.
    assert_eq!(
        login_desde(&p, &proxy, Some("203.0.113.41"), CLAVE).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn se_toma_la_ultima_ip_que_puso_el_proxy() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    let proxy = por_el_proxy();
    // Lo de la izquierda lo pudo escribir el cliente; lo último, el proxy.
    assert_eq!(
        login_desde(&p, &proxy, Some("198.51.100.1, 203.0.113.40"), "mala").await,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login_desde(&p, &proxy, Some("203.0.113.40"), CLAVE).await,
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        login_desde(&p, &proxy, Some("198.51.100.1"), CLAVE).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn una_ip_mala_no_bloquea_a_otra() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    let proxy = por_el_proxy();
    for _ in 0..3 {
        let s = login_desde(&p, &proxy, Some("203.0.113.66"), "mala").await;
        assert!(
            s == StatusCode::UNAUTHORIZED || s == StatusCode::TOO_MANY_REQUESTS,
            "{s}"
        );
    }
    assert_eq!(
        login_desde(&p, &proxy, Some("203.0.113.40"), CLAVE).await,
        StatusCode::OK
    );
    assert_eq!(
        login_desde(&p, "127.0.0.1:5000", None, CLAVE).await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn ya_no_hay_freno_por_usuario() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    // Un fallo con el usuario del dueño desde una IP...
    assert_eq!(
        login_desde(&p, "198.51.100.7:4000", None, "mala").await,
        StatusCode::UNAUTHORIZED
    );
    // ...no impide que el dueño entre desde la suya.
    assert_eq!(
        login_desde(&p, "198.51.100.8:4000", None, CLAVE).await,
        StatusCode::OK
    );
    let filas: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM freno WHERE tipo = 'usuario'")
        .fetch_one(&p.state.db)
        .await
        .unwrap();
    assert_eq!(filas, 0);
}

#[tokio::test]
async fn en_ipv6_el_freno_va_por_la_red_64() {
    let p = con_sim(qbt_sim::Conf::default()).await;
    assert_eq!(
        login_desde(&p, "[2001:db8:1:2::5]:4000", None, "mala").await,
        StatusCode::UNAUTHORIZED
    );
    // Otra dirección de la misma /64 (el mismo equipo o la misma red) espera.
    assert_eq!(
        login_desde(&p, "[2001:db8:1:2:aaaa::9]:4000", None, CLAVE).await,
        StatusCode::TOO_MANY_REQUESTS
    );
    // Otra /64 entra a la primera.
    assert_eq!(
        login_desde(&p, "[2001:db8:1:3::5]:4000", None, CLAVE).await,
        StatusCode::OK
    );
}
