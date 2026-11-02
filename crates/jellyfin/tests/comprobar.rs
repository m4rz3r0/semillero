//! `comprobar` contra un Jellyfin falso: una lectura con la clave, sin
//! reescanear nada.

use std::sync::{Arc, Mutex};

use axum::Router;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use jellyfin::{Error, Jellyfin};

type Visto = Arc<Mutex<Vec<String>>>;

/// Jellyfin falso con la clave `clave`: apunta la ruta de cada petición.
async fn falso(clave: &'static str) -> (String, Visto) {
    let visto: Visto = Arc::default();
    let (v1, v2) = (visto.clone(), visto.clone());
    let app = Router::new()
        .route(
            "/System/Info",
            get(move |h: HeaderMap| {
                let v = v1.clone();
                async move {
                    v.lock().unwrap().push("info".into());
                    let a = h.get("authorization").and_then(|a| a.to_str().ok());
                    if a == Some(&format!("MediaBrowser Token=\"{clave}\"")) {
                        StatusCode::OK
                    } else {
                        StatusCode::UNAUTHORIZED
                    }
                }
            }),
        )
        .route(
            "/Library/Refresh",
            post(move || {
                let v = v2.clone();
                async move {
                    v.lock().unwrap().push("refresh".into());
                    StatusCode::NO_CONTENT
                }
            }),
        );
    let oyente = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir = oyente.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(oyente, app).await.unwrap() });
    (format!("http://{dir}"), visto)
}

#[tokio::test]
async fn comprueba_la_clave_sin_reescanear() {
    let (url, visto) = falso("abc123").await;
    assert_eq!(Jellyfin::new(&url, "abc123\n").comprobar().await, Ok(()));
    assert_eq!(
        Jellyfin::new(&url, "otra").comprobar().await,
        Err(Error::ClaveRechazada)
    );
    assert_eq!(*visto.lock().unwrap(), vec!["info", "info"]);
}

#[tokio::test]
async fn comprobar_sin_jellyfin() {
    let e = Jellyfin::new("http://127.0.0.1:1", "k")
        .comprobar()
        .await
        .unwrap_err();
    assert_eq!(e, Error::NoResponde);
}
