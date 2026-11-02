//! Argumentos de la línea de órdenes: sin ninguno arranca el servidor; uno
//! que no conoce sale con error en lugar de arrancarlo.

use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Lanza el binario con `args` y espera hasta 10 s; si sigue vivo (ha
/// arrancado el servidor), lo mata y falla.
fn lanzar(args: &[&str]) -> Output {
    let dir = tempfile::tempdir().unwrap();
    let mut hijo = Command::new(env!("CARGO_BIN_EXE_semillero"))
        .args(args)
        .env("SEMILLERO_LISTEN", "127.0.0.1:0")
        .env("SEMILLERO_DB", dir.path().join("semillero.db"))
        .env("SEMILLERO_QBT", "http://127.0.0.1:9")
        .env("LEPTOS_SITE_ROOT", dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let inicio = Instant::now();
    while hijo.try_wait().unwrap().is_none() {
        if inicio.elapsed() > Duration::from_secs(10) {
            hijo.kill().unwrap();
            panic!("{args:?} arrancó el servidor");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    hijo.wait_with_output().unwrap()
}

#[test]
fn una_orden_desconocida_sale_con_error_y_la_ayuda() {
    let s = lanzar(&["configurr"]);
    assert_eq!(s.status.code(), Some(2));
    let err = String::from_utf8_lossy(&s.stderr);
    assert!(err.contains("configurr"), "{err}");
    assert!(err.contains("semillero configurar"), "{err}");
}

#[test]
fn version_y_ayuda() {
    let v = lanzar(&["--version"]);
    assert!(v.status.success());
    assert_eq!(
        String::from_utf8_lossy(&v.stdout).trim(),
        format!("semillero {}", env!("CARGO_PKG_VERSION"))
    );
    for a in ["--help", "-h"] {
        let h = lanzar(&[a]);
        assert!(h.status.success(), "{a}");
        assert!(String::from_utf8_lossy(&h.stdout).contains("semillero configurar"));
    }
}
