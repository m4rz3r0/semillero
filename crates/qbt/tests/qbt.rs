use std::net::{IpAddr, Ipv4Addr};

use qbt::{Cuenta, Error, Qbt};
use qbt_sim::{Conf, arrancar};
use serde_json::json;

const H1: &str = "0123456789abcdef0123456789abcdef01234567";
const H2: &str = "89abcdef0123456789abcdef0123456789abcdef";
const LOCAL: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
const OTRA: IpAddr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 2));

fn torrent(hash: &str) -> serde_json::Value {
    json!({
        "hash": hash, "name": "Una película", "category": "peliculas", "state": "uploading",
        "downloaded": 1000, "total_size": 1000, "uploaded": 1500, "seeding_time": 700000,
        "ratio": 1.5, "added_on": 1700000000, "save_path": "/media/peliculas",
        "upspeed": 0, "dlspeed": 0, "progress": 1.0
    })
}

/// 127.0.0.2 no existe en todos los sistemas (FreeBSD solo trae 127.0.0.1).
fn hay_otra_loopback() -> bool {
    std::net::TcpListener::bind((OTRA, 0)).is_ok()
}

/// Sim con 127.0.0.1 en la lista blanca, como semillero en producción.
async fn sim_con_lista_blanca() -> qbt_sim::Sim {
    arrancar(Conf {
        lista_blanca: vec![LOCAL],
        ..Default::default()
    })
    .await
}

#[tokio::test]
async fn lista_los_torrents() {
    let sim = sim_con_lista_blanca().await;
    sim.control.fijar_torrents(vec![torrent(H1)]);
    let ts = Qbt::new(&sim.url, LOCAL).torrents().await.unwrap();
    assert_eq!(ts.len(), 1);
    assert_eq!(ts[0].hash, H1);
    assert_eq!(ts[0].category, "peliculas");
    assert_eq!(ts[0].uploaded, 1500);
    assert_eq!(ts[0].save_path, "/media/peliculas");
}

#[tokio::test]
async fn obtener_valida_el_hash_antes_de_preguntar() {
    let sim = sim_con_lista_blanca().await;
    sim.control.fijar_torrents(vec![torrent(H1), torrent(H2)]);
    let q = Qbt::new(&sim.url, LOCAL);
    // Un hash vacío haría que qBittorrent devolviera todos.
    for malo in ["", "xyz", &H1[..39], &H1.to_uppercase()] {
        assert!(
            matches!(q.obtener(malo).await, Err(Error::HashNoValido)),
            "{malo:?}"
        );
    }
    assert!(sim.control.llamadas().is_empty());
    assert_eq!(q.obtener(H2).await.unwrap().unwrap().hash, H2);
    assert!(q.obtener(&"a".repeat(64)).await.unwrap().is_none());
}

#[tokio::test]
async fn borrar_con_y_sin_ficheros() {
    let sim = sim_con_lista_blanca().await;
    sim.control.fijar_torrents(vec![torrent(H1), torrent(H2)]);
    let q = Qbt::new(&sim.url, LOCAL);
    q.borrar(H1, true).await.unwrap();
    q.borrar(H2, false).await.unwrap();
    let cuerpos: Vec<String> = sim
        .control
        .llamadas()
        .into_iter()
        .map(|l| l.cuerpo)
        .collect();
    assert_eq!(
        cuerpos,
        [
            format!("hashes={H1}&deleteFiles=true"),
            format!("hashes={H2}&deleteFiles=false")
        ]
    );
    assert!(sim.control.torrents().is_empty());
    assert!(matches!(q.borrar("", true).await, Err(Error::HashNoValido)));
}

#[tokio::test]
async fn parar_y_reanudar() {
    let sim = sim_con_lista_blanca().await;
    sim.control.fijar_torrents(vec![torrent(H1), torrent(H2)]);
    let q = Qbt::new(&sim.url, LOCAL);
    q.parar(&[H1.into(), H2.into()]).await.unwrap();
    assert!(
        q.torrents()
            .await
            .unwrap()
            .iter()
            .all(|t| t.state == "stoppedUP")
    );
    q.reanudar(&[H1.into()]).await.unwrap();
    assert_eq!(q.obtener(H1).await.unwrap().unwrap().state, "uploading");
    // Sin hashes no se llama: qBittorrent lo tomaría como «ninguno», pero no hace falta arriesgarse.
    let antes = sim.control.llamadas().len();
    q.parar(&[]).await.unwrap();
    assert_eq!(sim.control.llamadas().len(), antes);
}

#[tokio::test]
async fn credenciales_con_qbittorrent_5_2() {
    let sim = arrancar(Conf::default()).await;
    let q = Qbt::new(&sim.url, LOCAL);
    assert!(q.credenciales_validas("admin", "secreta").await.unwrap());
    assert!(!q.credenciales_validas("admin", "mala").await.unwrap());
    assert!(!q.credenciales_validas("otro", "secreta").await.unwrap());
}

#[tokio::test]
async fn el_login_sale_desde_fuera_de_la_lista_blanca() {
    if !hay_otra_loopback() {
        eprintln!("sin 127.0.0.2: se salta");
        return;
    }
    // 127.0.0.1 en la lista blanca entraría con cualquier contraseña.
    let sim = sim_con_lista_blanca().await;
    let q = Qbt::new(&sim.url, OTRA);
    assert!(!q.credenciales_validas("admin", "mala").await.unwrap());
    assert!(q.credenciales_validas("admin", "secreta").await.unwrap());
    assert!(sim.control.llamadas().iter().all(|l| l.desde == OTRA));
}

#[tokio::test]
async fn sin_qbittorrent_dice_que_no_responde() {
    // El puerto 1 (tcpmux) no escucha; uno libre que se suelta podría
    // ocuparlo otra prueba en paralelo.
    let q = Qbt::new("http://127.0.0.1:1", LOCAL);
    assert!(matches!(q.torrents().await, Err(Error::NoResponde(_))));
    assert!(matches!(
        q.credenciales_validas("a", "b").await,
        Err(Error::NoResponde(_))
    ));
}

#[tokio::test]
async fn un_403_se_distingue_de_una_caida() {
    // Fuera de la lista blanca y sin sesión.
    let sim = arrancar(Conf::default()).await;
    let q = Qbt::new(&sim.url, LOCAL);
    assert!(matches!(q.torrents().await, Err(Error::Rechazado(403))));
}

#[tokio::test]
async fn credenciales_con_qbittorrent_antiguo() {
    // Antes de 5.2: 200 con «Ok.» o «Fails.».
    let sim = arrancar(Conf {
        antiguo: true,
        ..Default::default()
    })
    .await;
    let q = Qbt::new(&sim.url, LOCAL);
    assert!(q.credenciales_validas("admin", "secreta").await.unwrap());
    assert!(!q.credenciales_validas("admin", "mala").await.unwrap());
}

#[tokio::test]
async fn un_baneo_no_se_confunde_con_una_clave_mala() {
    let sim = arrancar(Conf {
        baneado: true,
        ..Default::default()
    })
    .await;
    let q = Qbt::new(&sim.url, LOCAL);
    assert!(matches!(
        q.credenciales_validas("admin", "secreta").await,
        Err(Error::Rechazado(403))
    ));
}

/// qBittorrent falso que a cada conexión contesta `respuesta` tal cual y
/// luego manda `relleno` bytes más (o nada).
async fn crudo(respuesta: &'static str, relleno: usize) -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let oyente = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir = oyente.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut c, _)) = oyente.accept().await {
            tokio::spawn(async move {
                let mut b = [0u8; 4096];
                let _ = c.read(&mut b).await;
                let _ = c.write_all(respuesta.as_bytes()).await;
                let trozo = vec![b'x'; 64 * 1024];
                let mut falta = relleno;
                while falta > 0 {
                    let n = falta.min(trozo.len());
                    if c.write_all(&trozo[..n]).await.is_err() {
                        return;
                    }
                    falta -= n;
                }
            });
        }
    });
    format!("http://{dir}")
}

#[tokio::test]
async fn respuesta_que_dice_ser_enorme_no_se_lee() {
    let url = crudo(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 1000000000000\r\n\r\n[",
        0,
    )
    .await;
    let e = Qbt::new(&url, LOCAL).torrents().await.unwrap_err();
    assert!(matches!(e, Error::Respuesta(_)), "{e:?}");
}

#[tokio::test]
async fn respuesta_sin_longitud_se_corta_en_el_limite() {
    // Sin `content-length`: se lee hasta cerrar la conexión, pero no más
    // allá del límite del login.
    let url = crudo(
        "HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n",
        qbt::MAX_RESPUESTA_LOGIN + 1,
    )
    .await;
    let e = Qbt::new(&url, LOCAL)
        .credenciales_validas("u", "c")
        .await
        .unwrap_err();
    assert!(matches!(e, Error::Respuesta(_)), "{e:?}");
}

// ---------------------------------------------------------------- modo con contraseña

fn cuenta(clave: &str) -> Cuenta {
    Cuenta::new("admin", clave)
}

fn logins(sim: &qbt_sim::Sim) -> usize {
    sim.control
        .llamadas()
        .iter()
        .filter(|l| l.ruta == "/api/v2/auth/login")
        .count()
}

#[tokio::test]
async fn con_contrasena_entra_sin_lista_blanca_y_guarda_la_sesion() {
    // Sin lista blanca: sin sesión, la API contesta 403.
    let sim = arrancar(Conf::default()).await;
    sim.control.fijar_torrents(vec![torrent(H1)]);
    let q = Qbt::con_cuenta(&sim.url, None, cuenta("secreta"));
    assert_eq!(q.torrents().await.unwrap()[0].hash, H1);
    assert_eq!(logins(&sim), 1);
    // La sesión se guarda: no se vuelve a entrar en cada petición.
    q.torrents().await.unwrap();
    q.parar(&[H1.into()]).await.unwrap();
    assert_eq!(logins(&sim), 1);
    assert_eq!(sim.control.torrents()[0]["state"], "stoppedUP");
}

#[tokio::test]
async fn con_contrasena_vuelve_a_entrar_si_la_sesion_caduca() {
    let sim = arrancar(Conf::default()).await;
    sim.control.fijar_torrents(vec![torrent(H1), torrent(H2)]);
    let q = Qbt::con_cuenta(&sim.url, None, cuenta("secreta"));
    q.torrents().await.unwrap();
    sim.control.olvidar_sesiones();
    // El borrado contesta 403, se entra otra vez y se repite una vez.
    q.borrar(H1, false).await.unwrap();
    assert_eq!(logins(&sim), 2);
    let borrados = sim
        .control
        .llamadas()
        .iter()
        .filter(|l| l.ruta == "/api/v2/torrents/delete")
        .count();
    assert_eq!(borrados, 2, "el primero rechazado, el segundo hecho");
    assert_eq!(sim.control.torrents().len(), 1);
}

#[tokio::test]
async fn con_contrasena_mala_falla_sin_insistir() {
    let sim = arrancar(Conf::default()).await;
    let q = Qbt::con_cuenta(&sim.url, None, cuenta("mala"));
    assert!(matches!(q.torrents().await, Err(Error::CuentaRechazada)));
    // Un solo intento por petición: no suma fallos al contador de bloqueos.
    assert_eq!(logins(&sim), 1);
    let antiguo = arrancar(Conf {
        antiguo: true,
        ..Default::default()
    })
    .await;
    let q = Qbt::con_cuenta(&antiguo.url, None, cuenta("mala"));
    assert!(matches!(q.torrents().await, Err(Error::CuentaRechazada)));
}

#[tokio::test]
async fn con_contrasena_y_qbittorrent_antiguo() {
    let sim = arrancar(Conf {
        antiguo: true,
        ..Default::default()
    })
    .await;
    sim.control.fijar_torrents(vec![torrent(H1)]);
    let q = Qbt::con_cuenta(&sim.url, None, cuenta("secreta"));
    assert_eq!(q.torrents().await.unwrap().len(), 1);
}

#[tokio::test]
async fn con_contrasena_y_lista_blanca_no_hace_falta_entrar() {
    let sim = sim_con_lista_blanca().await;
    sim.control.fijar_torrents(vec![torrent(H1)]);
    let q = Qbt::con_cuenta(&sim.url, None, cuenta("secreta"));
    assert_eq!(q.torrents().await.unwrap().len(), 1);
    assert_eq!(logins(&sim), 0);
}

#[tokio::test]
async fn con_contrasena_un_baneo_es_un_rechazo() {
    let sim = arrancar(Conf {
        baneado: true,
        ..Default::default()
    })
    .await;
    let q = Qbt::con_cuenta(&sim.url, None, cuenta("secreta"));
    assert!(matches!(q.torrents().await, Err(Error::Rechazado(403))));
    assert_eq!(logins(&sim), 1);
}

#[tokio::test]
async fn sin_contrasena_no_se_entra_nunca() {
    // Lo de siempre: un 403 es un 403, sin intentar entrar.
    let sim = arrancar(Conf::default()).await;
    let q = Qbt::new(&sim.url, LOCAL);
    assert!(matches!(q.torrents().await, Err(Error::Rechazado(403))));
    assert_eq!(logins(&sim), 0);
}

#[test]
fn la_cuenta_no_ensena_la_contrasena() {
    let texto = format!("{:?}", cuenta("muy-secreta"));
    assert!(!texto.contains("muy-secreta"), "{texto}");
    assert!(texto.contains("admin"), "{texto}");
}
