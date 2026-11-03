//! Añadir torrents desde semillero: archivos `.torrent` y enlaces magnet,
//! con una categoría de qBittorrent. Solo con sesión y token; si algo no
//! vale, no se añade nada; nunca una dirección web; los límites de tamaño
//! y de número; y el error nunca repite el enlace entero.

use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use semillero::api::{MAX_ARCHIVOS, MAX_BYTES_TORRENT, MAX_MAGNETS};
use semillero::db::{self, Evento, Modo};
use serde_json::{Value, json};
use sha1::{Digest, Sha1};

use crate::comun::{enviar, get};
use crate::interfaz::{Ui, api, ui};

const H1: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const H2: &str = "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";
/// Lo que nunca sale en una respuesta: la clave del anuncio.
const CLAVE_DEL_ANUNCIO: &str = "clavesecreta0123";

fn magnet(hash: &str, nombre: &str) -> String {
    format!(
        "magnet:?xt=urn:btih:{hash}&dn={nombre}&tr=https%3A%2F%2Ftracker.ejemplo.example%2F\
         {CLAVE_DEL_ANUNCIO}%2Fannounce"
    )
}

/// El diccionario `info` de un torrent pequeño y el `.torrent` entero.
fn torrent(nombre: &str) -> (Vec<u8>, Vec<u8>) {
    let mut info = format!(
        "d6:lengthi1000e4:name{}:{nombre}12:piece lengthi16384e6:pieces20:",
        nombre.len()
    )
    .into_bytes();
    info.extend_from_slice(&[7u8; 20]);
    info.push(b'e');
    let mut t = b"d8:announce35:https://tracker.ejemplo.example/ann4:info".to_vec();
    t.extend_from_slice(&info);
    t.push(b'e');
    (info, t)
}

fn archivo(nombre: &str, datos: &[u8]) -> Value {
    json!({ "nombre": nombre, "datos": STANDARD.encode(datos) })
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

async fn preparada() -> Ui {
    let u = ui().await;
    u.p.sim()
        .control
        .fijar_categorias(vec!["peliculas".into(), "series".into()]);
    u
}

impl Ui {
    async fn anadir(
        &self,
        archivos: Vec<Value>,
        magnets: &str,
        categoria: &str,
    ) -> (StatusCode, String) {
        self.llamar(
            "/api/anadir_torrents",
            json!({ "archivos": archivos, "magnets": magnets, "categoria": categoria }),
        )
        .await
    }

    /// Cuántas veces se pidió a qBittorrent que añadiera algo.
    fn anadidos_anadido_en_qbt(&self) -> Vec<String> {
        self.p
            .sim()
            .control
            .llamadas()
            .into_iter()
            .filter(|l| l.ruta == "/api/v2/torrents/add")
            .map(|l| l.cuerpo)
            .collect()
    }

    fn anadido_en_qbt(&self, hash: &str) -> Option<Value> {
        self.p
            .sim()
            .control
            .torrents()
            .into_iter()
            .find(|t| t["hash"] == hash)
    }

    async fn apuntes_anadido(&self) -> Vec<db::Apunte> {
        db::historial(&self.p.state.db, 100)
            .await
            .unwrap()
            .into_iter()
            .filter(|a| a.evento == Evento::Anadido)
            .collect()
    }
}

#[tokio::test]
async fn anade_un_magnet_con_su_categoria_y_lo_apunta() {
    let u = preparada().await;
    let (s, cuerpo) = u.anadir(vec![], &magnet(H1, "Una+serie"), "series").await;
    assert_eq!(s, StatusCode::OK, "{cuerpo}");
    let r: Value = serde_json::from_str(&cuerpo).unwrap();
    assert_eq!(r["anadidos"], json!(["Una serie"]));
    let t = u.anadido_en_qbt(H1).expect("añadido en qBittorrent");
    assert_eq!(t["category"], "series");
    let enviado = u.anadidos_anadido_en_qbt();
    assert_eq!(enviado.len(), 1);
    assert!(enviado[0].contains("category=series"), "{}", enviado[0]);
    // El enlace llega entero a qBittorrent (con su tracker).
    assert!(enviado[0].contains(CLAVE_DEL_ANUNCIO), "{}", enviado[0]);
    // Y no en la respuesta.
    assert!(!cuerpo.contains(CLAVE_DEL_ANUNCIO), "{cuerpo}");
    let apuntes = u.apuntes_anadido().await;
    assert_eq!(apuntes.len(), 1, "{apuntes:?}");
    assert_eq!(apuntes[0].hash, H1);
    assert_eq!(apuntes[0].nombre, "Una serie");
    assert!(
        apuntes[0].detalle.contains("series"),
        "{}",
        apuntes[0].detalle
    );
    assert!(!apuntes[0].detalle.contains(CLAVE_DEL_ANUNCIO));
    // El historial lo enseña como «Añadido».
    let (_, h) = u.llamar("/api/historial", json!({})).await;
    assert!(h.contains("\"Anadido\""), "{h}");
}

#[tokio::test]
async fn anade_un_torrent_sin_categoria() {
    let u = preparada().await;
    let (info, datos) = torrent("Mi película");
    let (s, cuerpo) = u
        .anadir(vec![archivo("mi-pelicula.torrent", &datos)], "", "")
        .await;
    assert_eq!(s, StatusCode::OK, "{cuerpo}");
    let r: Value = serde_json::from_str(&cuerpo).unwrap();
    assert_eq!(r["anadidos"], json!(["mi-pelicula.torrent"]));
    let enviado = u.anadidos_anadido_en_qbt();
    assert_eq!(enviado.len(), 1);
    assert!(
        enviado[0].contains("torrents=mi-pelicula.torrent"),
        "{}",
        enviado[0]
    );
    assert!(!enviado[0].contains("category="), "{}", enviado[0]);
    let t =
        u.p.sim()
            .control
            .torrents()
            .into_iter()
            .find(|t| t["name"] == "Mi película")
            .expect("añadido en qBittorrent");
    assert_eq!(t["category"], "");
    // En el historial, con su hash de verdad (SHA-1 de `info`).
    let apuntes = u.apuntes_anadido().await;
    assert_eq!(apuntes.len(), 1);
    assert_eq!(apuntes[0].hash, hex(&Sha1::digest(&info)));
    assert_eq!(apuntes[0].nombre, "Mi película");
    assert!(
        apuntes[0].detalle.contains("sin categoría"),
        "{}",
        apuntes[0].detalle
    );
}

#[tokio::test]
async fn varios_a_la_vez() {
    let u = preparada().await;
    let (_, d1) = torrent("Uno");
    let (_, d2) = torrent("Dos");
    let magnets = format!("{}\n\n  {}  \r\n", magnet(H1, "Tres"), magnet(H2, "Cuatro"));
    let (s, cuerpo) = u
        .anadir(
            vec![archivo("uno.torrent", &d1), archivo("dos.torrent", &d2)],
            &magnets,
            "peliculas",
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{cuerpo}");
    let r: Value = serde_json::from_str(&cuerpo).unwrap();
    assert_eq!(r["anadidos"].as_array().unwrap().len(), 4, "{cuerpo}");
    // Una sola petición a qBittorrent, con todo.
    assert_eq!(u.anadidos_anadido_en_qbt().len(), 1);
    assert_eq!(u.p.sim().control.torrents().len(), 5 + 4);
    assert_eq!(u.apuntes_anadido().await.len(), 4);
}

#[tokio::test]
async fn si_uno_no_vale_no_se_anade_ninguno() {
    let u = preparada().await;
    let (_, d1) = torrent("Uno");
    let magnets = format!(
        "{}\nmagnet:?dn=sin+hash&tr=https://tracker.ejemplo.example/{CLAVE_DEL_ANUNCIO}/announce\n{}",
        magnet(H1, "Uno"),
        magnet(H2, "Dos")
    );
    let (s, cuerpo) = u
        .anadir(
            vec![
                archivo("uno.torrent", &d1),
                archivo("roto.torrent", b"hola"),
            ],
            &magnets,
            "",
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{cuerpo}");
    assert!(cuerpo.contains("No se ha añadido nada"), "{cuerpo}");
    assert!(cuerpo.contains("línea 2"), "{cuerpo}");
    assert!(cuerpo.contains("«roto.torrent»"), "{cuerpo}");
    assert!(!cuerpo.contains("línea 1"), "{cuerpo}");
    assert!(!cuerpo.contains(CLAVE_DEL_ANUNCIO), "{cuerpo}");
    assert!(!cuerpo.contains("tracker.ejemplo.example"), "{cuerpo}");
    assert!(u.anadidos_anadido_en_qbt().is_empty());
    assert!(u.anadido_en_qbt(H1).is_none());
    assert!(u.apuntes_anadido().await.is_empty());
}

#[tokio::test]
async fn un_magnet_malo_dice_solo_su_linea_o_su_hash() {
    let u = preparada().await;
    let casos = [
        // Sin `xt`.
        format!("magnet:?dn=x&tr=https://tracker.ejemplo.example/{CLAVE_DEL_ANUNCIO}/a"),
        // Hash corto.
        format!("magnet:?xt=urn:btih:abcd&tr=https://tracker.ejemplo.example/{CLAVE_DEL_ANUNCIO}"),
        // Hash con letras que no son hexadecimales ni base32.
        format!(
            "magnet:?xt=urn:btih:{}&tr=udp://tracker.ejemplo.example/{CLAVE_DEL_ANUNCIO}",
            "z".repeat(40)
        ),
        // No es un magnet.
        format!("ftp://ejemplo.example/{CLAVE_DEL_ANUNCIO}"),
    ];
    for m in &casos {
        let (s, cuerpo) = u.anadir(vec![], m, "").await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{m}: {cuerpo}");
        assert!(cuerpo.contains("línea 1"), "{cuerpo}");
        assert!(!cuerpo.contains(CLAVE_DEL_ANUNCIO), "{cuerpo}");
        assert!(!cuerpo.contains("ejemplo.example"), "{cuerpo}");
    }
    // Base32 y v2 (`btmh`) sí valen.
    let base32 = format!("magnet:?xt=urn:btih:{}", "A".repeat(32));
    let v2 = format!("magnet:?xt=urn:btmh:1220{}", "c".repeat(64));
    let (s, cuerpo) = u.anadir(vec![], &format!("{base32}\n{v2}"), "").await;
    assert_eq!(s, StatusCode::OK, "{cuerpo}");
    assert!(u.anadido_en_qbt(&"0".repeat(40)).is_some());
    assert!(u.anadidos_anadido_en_qbt().len() == 1);
}

#[tokio::test]
async fn no_acepta_direcciones_web() {
    let u = preparada().await;
    for url in [
        "http://192.0.2.1/admin".to_owned(),
        format!("https://ejemplo.example/{CLAVE_DEL_ANUNCIO}/un.torrent"),
        "HTTPS://ejemplo.example/otro.torrent".to_owned(),
    ] {
        let (s, cuerpo) = u.anadir(vec![], &url, "").await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{url}: {cuerpo}");
        assert!(cuerpo.contains("dirección web"), "{cuerpo}");
        assert!(!cuerpo.contains("ejemplo.example"), "{cuerpo}");
        assert!(!cuerpo.contains("192.0.2"), "{cuerpo}");
    }
    assert!(u.anadidos_anadido_en_qbt().is_empty());
}

#[tokio::test]
async fn la_categoria_tiene_que_ser_de_qbittorrent() {
    let u = preparada().await;
    let (s, cuerpo) = u.anadir(vec![], &magnet(H1, "x"), "musica").await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{cuerpo}");
    assert!(cuerpo.contains("categoría"), "{cuerpo}");
    assert!(u.anadidos_anadido_en_qbt().is_empty());
}

#[tokio::test]
async fn un_torrent_que_no_lo_es_se_rechaza() {
    let u = preparada().await;
    for (nombre, datos) in [
        ("texto.torrent", b"hola".to_vec()),
        ("lista.torrent", b"l4:infoe".to_vec()),
        ("sin-info.torrent", b"d4:name3:abce".to_vec()),
        ("info-no-dicc.torrent", b"d4:info3:abce".to_vec()),
        ("cortado.torrent", b"d4:infod4:name3:ab".to_vec()),
    ] {
        let (s, cuerpo) = u.anadir(vec![archivo(nombre, &datos)], "", "").await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{nombre}: {cuerpo}");
        assert!(cuerpo.contains(&format!("«{nombre}»")), "{cuerpo}");
    }
    // Base64 roto.
    let (s, cuerpo) = u
        .anadir(
            vec![json!({ "nombre": "a.torrent", "datos": "%%%" })],
            "",
            "",
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{cuerpo}");
    assert!(u.anadidos_anadido_en_qbt().is_empty());
}

#[tokio::test]
async fn sin_nada_que_anadir() {
    let u = preparada().await;
    let (s, cuerpo) = u.anadir(vec![], " \n \n", "series").await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{cuerpo}");
    assert!(cuerpo.contains("magnet"), "{cuerpo}");
    assert!(u.anadidos_anadido_en_qbt().is_empty());
}

#[tokio::test]
async fn limites_de_tamano() {
    let u = preparada().await;
    // Un archivo de más de 4 MiB llega (cabe en el límite de la ruta), pero
    // la server function lo rechaza.
    let (_, mut grande) = torrent("Grande");
    grande.resize(MAX_BYTES_TORRENT + 1, b'0');
    let (s, cuerpo) = u
        .anadir(vec![archivo("grande.torrent", &grande)], "", "")
        .await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE, "{cuerpo}");
    assert!(cuerpo.contains("«grande.torrent»"), "{cuerpo}");
    assert!(cuerpo.contains("4 MiB"), "{cuerpo}");
    // Justo 4 MiB, sí (relleno tras el diccionario no vale: el relleno va
    // dentro, en una cadena).
    let (_, mut justo) = torrent("Justo");
    justo.pop();
    let relleno = MAX_BYTES_TORRENT - justo.len() - "5:extra:e".len() - 8;
    justo.extend_from_slice(format!("5:extra{relleno}:").as_bytes());
    justo.resize(justo.len() + relleno, b'x');
    justo.push(b'e');
    assert!(justo.len() <= MAX_BYTES_TORRENT);
    let (s, cuerpo) = u
        .anadir(vec![archivo("justo.torrent", &justo)], "", "")
        .await;
    assert_eq!(s, StatusCode::OK, "{cuerpo}");
    // Una petición más grande que el límite de la ruta ni llega.
    let enorme = "A".repeat(semillero::seguridad::LIMITE_CUERPO_ANADIR + 1);
    let (s, _) = u
        .anadir(
            vec![json!({ "nombre": "a.torrent", "datos": enorme })],
            "",
            "",
        )
        .await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    // Y el límite mayor es solo para esta ruta.
    let (s, _) = u
        .llamar(
            "/api/guardar_preset",
            json!({ "nombre": "x".repeat(200 * 1024), "descripcion": "" }),
        )
        .await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(u.anadidos_anadido_en_qbt().len(), 1);
}

#[tokio::test]
async fn limites_de_numero() {
    let u = preparada().await;
    let (_, d) = torrent("Uno");
    let archivos: Vec<Value> = (0..=MAX_ARCHIVOS)
        .map(|i| archivo(&format!("{i}.torrent"), &d))
        .collect();
    let (s, cuerpo) = u.anadir(archivos, "", "").await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{cuerpo}");
    assert!(cuerpo.contains(&MAX_ARCHIVOS.to_string()), "{cuerpo}");
    let magnets: Vec<String> = (0..=MAX_MAGNETS)
        .map(|i| format!("magnet:?xt=urn:btih:{i:040x}"))
        .collect();
    let (s, cuerpo) = u.anadir(vec![], &magnets.join("\n"), "").await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{cuerpo}");
    assert!(cuerpo.contains(&MAX_MAGNETS.to_string()), "{cuerpo}");
    assert!(u.anadidos_anadido_en_qbt().is_empty());
    // Justo en el límite, sí.
    let (s, cuerpo) = u
        .anadir(vec![], &magnets[..MAX_MAGNETS].join("\n"), "")
        .await;
    assert_eq!(s, StatusCode::OK, "{cuerpo}");
}

#[tokio::test]
async fn exige_sesion_y_token() {
    let u = preparada().await;
    let cuerpo = json!({ "archivos": [], "magnets": magnet(H1, "x"), "categoria": "" });
    let sin_sesion = enviar(
        &u.p.app,
        api("/api/anadir_torrents", cuerpo.clone(), None, None),
    )
    .await;
    assert_eq!(sin_sesion.status(), StatusCode::UNAUTHORIZED);
    let sin_token = enviar(
        &u.p.app,
        api("/api/anadir_torrents", cuerpo, Some(&u.cookie), None),
    )
    .await;
    assert_eq!(sin_token.status(), StatusCode::FORBIDDEN);
    let sin_sesion = enviar(&u.p.app, api("/api/anadir", json!({}), None, None)).await;
    assert_eq!(sin_sesion.status(), StatusCode::UNAUTHORIZED);
    assert!(u.anadidos_anadido_en_qbt().is_empty());
    assert!(u.anadido_en_qbt(H1).is_none());
    // La página, sin sesión, lleva al acceso.
    let r = enviar(&u.p.app, get("/anadir", None)).await;
    assert_eq!(r.status(), StatusCode::SEE_OTHER);
}

#[tokio::test]
async fn si_qbittorrent_no_lo_anade_lo_dice() {
    let u = preparada().await;
    u.p.sim().control.fallar_al_anadir(true);
    let (s, cuerpo) = u.anadir(vec![], &magnet(H1, "x"), "").await;
    assert_eq!(s, StatusCode::BAD_GATEWAY, "{cuerpo}");
    assert!(cuerpo.contains("qBittorrent no lo ha añadido"), "{cuerpo}");
    assert!(!cuerpo.contains(CLAVE_DEL_ANUNCIO), "{cuerpo}");
    assert!(u.apuntes_anadido().await.is_empty());
}

#[tokio::test]
async fn con_borrado_real_tambien_y_sin_contrasena() {
    let u = preparada().await;
    db::fijar_modo(&u.p.state.db, Modo::BorradoReal)
        .await
        .unwrap();
    let (s, cuerpo) = u.anadir(vec![], &magnet(H1, "x"), "").await;
    assert_eq!(s, StatusCode::OK, "{cuerpo}");
    assert!(u.anadido_en_qbt(H1).is_some());
}

#[tokio::test]
async fn la_pagina_trae_las_categorias_de_qbittorrent() {
    let u = preparada().await;
    let (s, cuerpo) = u.llamar("/api/anadir", json!({})).await;
    assert_eq!(s, StatusCode::OK, "{cuerpo}");
    let d: Value = serde_json::from_str(&cuerpo).unwrap();
    assert_eq!(d["categorias"], json!(["peliculas", "series"]));
    let html = u.pagina("/anadir").await;
    for texto in [
        "Añadir torrents",
        "accept=\".torrent",
        "multiple",
        "<textarea",
        "Sin categoría",
        ">peliculas<",
        ">series<",
    ] {
        assert!(html.contains(texto), "falta {texto:?} en la página");
    }
    // Y se llega desde la cabecera de cualquier página.
    assert!(u.pagina("/").await.contains("href=\"/anadir\""));
}
