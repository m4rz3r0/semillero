//! `reescanear` contra un Jellyfin falso.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use jellyfin::{Error, Jellyfin};

type Visto = Arc<Mutex<Vec<String>>>;

/// Jellyfin falso: apunta la cabecera `Authorization` de cada petición y
/// contesta `estado`.
async fn falso(estado: StatusCode) -> (String, Visto) {
    let visto: Visto = Arc::default();
    let v = visto.clone();
    let app = Router::new().route(
        "/Library/Refresh",
        post(move |h: HeaderMap| {
            let v = v.clone();
            async move {
                let a = h.get("authorization").and_then(|a| a.to_str().ok());
                v.lock().unwrap().push(a.unwrap_or_default().to_owned());
                estado
            }
        }),
    );
    let oyente = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir = oyente.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(oyente, app).await.unwrap() });
    (format!("http://{dir}"), visto)
}

#[tokio::test]
async fn pide_reescanear_con_la_clave() {
    let (url, visto) = falso(StatusCode::NO_CONTENT).await;
    let j = Jellyfin::new(&format!("{url}/"), "abc123\n");
    assert_eq!(j.reescanear().await, Ok(()));
    // Dos veces no hace daño.
    assert_eq!(j.reescanear().await, Ok(()));
    assert_eq!(
        *visto.lock().unwrap(),
        vec!["MediaBrowser Token=\"abc123\"".to_owned(); 2]
    );
}

#[tokio::test]
async fn clave_rechazada_sin_mostrar_la_clave() {
    let (url, _) = falso(StatusCode::UNAUTHORIZED).await;
    let e = Jellyfin::new(&url, "secreto-xyz")
        .reescanear()
        .await
        .unwrap_err();
    assert_eq!(e, Error::ClaveRechazada);
    assert!(!e.to_string().contains("secreto-xyz"));
    assert_eq!(e.to_string(), "Jellyfin no acepta la clave de la API");
}

#[tokio::test]
async fn error_del_servidor() {
    let (url, _) = falso(StatusCode::INTERNAL_SERVER_ERROR).await;
    let e = Jellyfin::new(&url, "k").reescanear().await.unwrap_err();
    assert_eq!(e.to_string(), "Jellyfin contestó con un error (500)");
}

#[tokio::test]
async fn no_responde() {
    // Puerto 1: cerrado.
    let e = Jellyfin::new("http://127.0.0.1:1", "k")
        .reescanear()
        .await
        .unwrap_err();
    assert_eq!(e, Error::NoResponde);
    assert_eq!(e.to_string(), "Jellyfin no responde");
}
