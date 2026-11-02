//! El servidor termina por sí solo con SIGTERM (`service stop`, systemd,
//! podman sin init): no se queda colgado hasta que lo matan.

use std::net::{TcpListener, TcpStream};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn sigterm_para_el_servidor() {
    let dir = tempfile::tempdir().unwrap();
    let puerto = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let escucha = format!("127.0.0.1:{puerto}");
    let mut hijo = Command::new(env!("CARGO_BIN_EXE_semillero"))
        .env("SEMILLERO_LISTEN", &escucha)
        .env("SEMILLERO_DB", dir.path().join("semillero.db"))
        .env("SEMILLERO_QBT", "http://127.0.0.1:9")
        .env("LEPTOS_SITE_ROOT", dir.path())
        .env("LEPTOS_OUTPUT_NAME", "semillero")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    let inicio = Instant::now();
    while TcpStream::connect(&escucha).is_err() {
        if let Some(estado) = hijo.try_wait().unwrap() {
            panic!("el servidor terminó antes de escuchar: {estado}");
        }
        assert!(
            inicio.elapsed() < Duration::from_secs(20),
            "no llega a escuchar"
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    let kill = Command::new("kill")
        .args(["-TERM", &hijo.id().to_string()])
        .status()
        .unwrap();
    assert!(kill.success());

    let inicio = Instant::now();
    let estado = loop {
        if let Some(estado) = hijo.try_wait().unwrap() {
            break estado;
        }
        if inicio.elapsed() > Duration::from_secs(5) {
            hijo.kill().unwrap();
            panic!("sigue vivo 5 s después de SIGTERM");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(estado.success(), "termina con {estado}");
}
