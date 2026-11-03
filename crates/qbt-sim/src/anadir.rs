//! `torrents/categories` y `torrents/add` (en `multipart/form-data`).
//!
//! Lo añadido entra en la lista de torrents con lo que semillero enseña: el
//! hash (el `btih` del magnet, o una huella fija de los bytes del
//! `.torrent`), el nombre (`dn` del magnet o `name` del `.torrent`), la
//! categoría y el estado. Un `.torrent` que no parece un diccionario con
//! `info` recibe 415, como en qBittorrent 5; si no se añade ninguno,
//! «Fails.».

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::body::Bytes;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::{Value, json};

use crate::{Estado, apuntar, autorizado};

/// Una parte del formulario: nombre del campo, nombre del archivo y datos.
#[derive(Debug, PartialEq)]
pub(crate) struct Parte {
    pub nombre: String,
    pub archivo: Option<String>,
    pub datos: Vec<u8>,
}

fn buscar(pajar: &[u8], aguja: &[u8], desde: usize) -> Option<usize> {
    pajar
        .get(desde..)?
        .windows(aguja.len())
        .position(|w| w == aguja)
        .map(|i| i + desde)
}

/// El valor de `clave="…"` en una cabecera `Content-Disposition`.
fn atributo(cabecera: &str, clave: &str) -> Option<String> {
    let patron = format!("{clave}=\"");
    let i = cabecera
        .match_indices(&patron)
        .find(|(i, _)| {
            *i == 0 || cabecera.as_bytes()[i - 1] == b' ' || cabecera.as_bytes()[i - 1] == b';'
        })?
        .0
        + patron.len();
    let fin = cabecera[i..].find('"')?;
    Some(cabecera[i..i + fin].to_owned())
}

/// Las partes de un cuerpo `multipart/form-data` con separador `sep`.
pub(crate) fn partes(cuerpo: &[u8], sep: &str) -> Option<Vec<Parte>> {
    let marca = format!("--{sep}");
    let mut i = buscar(cuerpo, marca.as_bytes(), 0)? + marca.len();
    let mut v = Vec::new();
    loop {
        if cuerpo.get(i..i + 2) == Some(b"--") {
            return Some(v);
        }
        i += 2; // \r\n
        let fin_cab = buscar(cuerpo, b"\r\n\r\n", i)?;
        let cabeceras = std::str::from_utf8(&cuerpo[i..fin_cab]).ok()?;
        let disposicion = cabeceras
            .split("\r\n")
            .find(|l| l.to_ascii_lowercase().starts_with("content-disposition:"))?;
        let siguiente = buscar(cuerpo, format!("\r\n{marca}").as_bytes(), fin_cab + 4)?;
        v.push(Parte {
            nombre: atributo(disposicion, "name")?,
            archivo: atributo(disposicion, "filename"),
            datos: cuerpo[fin_cab + 4..siguiente].to_vec(),
        });
        i = siguiente + 2 + marca.len();
    }
}

/// 32 caracteres en base32 (RFC 4648) a 40 cifras hexadecimales.
fn base32_a_hex(s: &str) -> Option<String> {
    let mut bits = 0u64;
    let mut n = 0;
    let mut hex = String::new();
    for c in s.bytes() {
        let v = match c.to_ascii_uppercase() {
            b @ b'A'..=b'Z' => b - b'A',
            b @ b'2'..=b'7' => b - b'2' + 26,
            _ => return None,
        };
        bits = (bits << 5) | u64::from(v);
        n += 5;
        while n >= 4 {
            n -= 4;
            hex.push(char::from_digit(((bits >> n) & 0xf) as u32, 16)?);
        }
    }
    Some(hex)
}

fn decodificar(s: &str) -> String {
    let s = s.replace('+', " ");
    let b = s.as_bytes();
    let mut v = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(x) = s
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            v.push(x);
            i += 3;
            continue;
        }
        v.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&v).into_owned()
}

/// Hash, nombre y tracker de un enlace magnet.
fn de_magnet(m: &str) -> Option<(String, String, String)> {
    let consulta = m.strip_prefix("magnet:?")?;
    let mut hash = None;
    let mut nombre = None;
    let mut tracker = None;
    for par in consulta.split('&') {
        let (k, v) = par.split_once('=').unwrap_or((par, ""));
        let v = decodificar(v);
        match k {
            "xt" => {
                if let Some(h) = v.strip_prefix("urn:btih:") {
                    hash = match h.len() {
                        40 => Some(h.to_ascii_lowercase()),
                        32 => base32_a_hex(h),
                        _ => None,
                    };
                } else if let Some(h) = v.strip_prefix("urn:btmh:1220") {
                    hash = h.get(..40).map(str::to_ascii_lowercase);
                }
            }
            "dn" => nombre = Some(v),
            "tr" if tracker.is_none() => tracker = Some(v),
            _ => {}
        }
    }
    let hash = hash?;
    Some((
        nombre.unwrap_or_else(|| hash.clone()),
        hash,
        tracker.unwrap_or_default(),
    ))
}

/// Una huella fija de 40 cifras hexadecimales (FNV-1a, tres veces).
fn huella(datos: &[u8]) -> String {
    let mut s = String::new();
    for semilla in 0..3u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64 ^ semilla;
        for b in datos {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        s.push_str(&format!("{h:016x}"));
    }
    s.truncate(40);
    s
}

/// El `name` de un `.torrent` (`4:name<n>:<nombre>`), si se encuentra.
fn nombre_de_torrent(datos: &[u8]) -> Option<String> {
    let i = buscar(datos, b"4:name", 0)? + 6;
    let dos = buscar(datos, b":", i)?;
    let n: usize = std::str::from_utf8(&datos[i..dos]).ok()?.parse().ok()?;
    let nombre = datos.get(dos + 1..dos + 1 + n)?;
    Some(String::from_utf8_lossy(nombre).into_owned())
}

fn parece_torrent(datos: &[u8]) -> bool {
    datos.first() == Some(&b'd') && buscar(datos, b"4:infod", 0).is_some()
}

fn nuevo(hash: String, nombre: String, categoria: &str, estado: &str, tracker: String) -> Value {
    let ahora = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    json!({
        "hash": hash, "name": nombre, "category": categoria, "state": estado,
        "downloaded": 0, "total_size": 0, "uploaded": 0, "seeding_time": 0,
        "ratio": 0.0, "added_on": ahora, "save_path": "/descargas",
        "upspeed": 0, "dlspeed": 0, "progress": 0.0, "tracker": tracker
    })
}

type Est = State<Arc<Estado>>;
type Desde = ConnectInfo<SocketAddr>;

pub(crate) async fn categorias(State(e): Est, ConnectInfo(desde): Desde, h: HeaderMap) -> Response {
    apuntar(&e, "/api/v2/torrents/categories", String::new(), desde);
    if !autorizado(&e, &h, desde) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let i = e.interior.lock().unwrap();
    let mapa: serde_json::Map<String, Value> = i
        .categorias
        .iter()
        .map(|c| (c.clone(), json!({ "name": c, "savePath": "" })))
        .collect();
    Json(Value::Object(mapa)).into_response()
}

pub(crate) async fn anadir(
    State(e): Est,
    ConnectInfo(desde): Desde,
    h: HeaderMap,
    cuerpo: Bytes,
) -> Response {
    let sep = h
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split_once("boundary="))
        .map(|(_, s)| s.trim_matches('"').to_owned());
    let lista = sep.as_deref().and_then(|s| partes(&cuerpo, s));
    // Lo que llegó, legible: `torrents=<archivo>`, `urls=…` y `category=…`.
    let resumen = lista
        .iter()
        .flatten()
        .map(|p| match &p.archivo {
            Some(a) => format!("{}={a}", p.nombre),
            None => format!("{}={}", p.nombre, String::from_utf8_lossy(&p.datos)),
        })
        .collect::<Vec<_>>()
        .join("&");
    apuntar(&e, "/api/v2/torrents/add", resumen, desde);
    if !autorizado(&e, &h, desde) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(lista) = lista else {
        return (StatusCode::BAD_REQUEST, "Fails.").into_response();
    };
    let mut i = e.interior.lock().unwrap();
    if i.fallar_al_anadir {
        return (StatusCode::OK, "Fails.").into_response();
    }
    let categoria = lista
        .iter()
        .find(|p| p.nombre == "category")
        .map(|p| String::from_utf8_lossy(&p.datos).into_owned())
        .unwrap_or_default();
    let mut nuevos = Vec::new();
    let mut fallos = 0;
    for p in &lista {
        match p.nombre.as_str() {
            "torrents" => {
                if !parece_torrent(&p.datos) {
                    return (StatusCode::UNSUPPORTED_MEDIA_TYPE, "Fails.").into_response();
                }
                let nombre = nombre_de_torrent(&p.datos).unwrap_or_else(|| {
                    let a = p.archivo.clone().unwrap_or_default();
                    a.strip_suffix(".torrent").unwrap_or(&a).to_owned()
                });
                nuevos.push(nuevo(
                    huella(&p.datos),
                    nombre,
                    &categoria,
                    "stalledDL",
                    String::new(),
                ));
            }
            "urls" => {
                for l in String::from_utf8_lossy(&p.datos).lines() {
                    match de_magnet(l.trim()) {
                        Some((nombre, hash, tracker)) => {
                            nuevos.push(nuevo(hash, nombre, &categoria, "metaDL", tracker))
                        }
                        None => fallos += 1,
                    }
                }
            }
            _ => {}
        }
    }
    let mut anadidos = 0;
    for t in nuevos {
        if i.torrents.iter().any(|x| x["hash"] == t["hash"]) {
            fallos += 1;
        } else {
            i.torrents.push(t);
            anadidos += 1;
        }
    }
    if anadidos == 0 && fallos > 0 {
        return (StatusCode::OK, "Fails.").into_response();
    }
    (StatusCode::OK, "Ok.").into_response()
}

#[cfg(test)]
mod pruebas {
    use super::*;

    #[test]
    fn lee_las_partes_de_un_formulario() {
        let cuerpo = b"--sep\r\nContent-Disposition: form-data; name=\"torrents\"; \
            filename=\"a.torrent\"\r\nContent-Type: application/x-bittorrent\r\n\r\n\
            d4:infodee\r\n--sep\r\nContent-Disposition: form-data; name=\"category\"\r\n\r\n\
            series\r\n--sep--\r\n";
        let v = partes(cuerpo, "sep").unwrap();
        assert_eq!(
            v,
            [
                Parte {
                    nombre: "torrents".into(),
                    archivo: Some("a.torrent".into()),
                    datos: b"d4:infodee".to_vec(),
                },
                Parte {
                    nombre: "category".into(),
                    archivo: None,
                    datos: b"series".to_vec(),
                },
            ]
        );
        assert!(partes(b"nada", "sep").is_none());
    }

    #[test]
    fn hash_nombre_y_tracker_del_magnet() {
        let (n, h, t) = de_magnet(
            "magnet:?xt=urn:btih:ABCDEFABCDEFABCDEFABCDEFABCDEFABCDEFABCD&dn=Una+cosa\
             &tr=https%3A%2F%2Ftracker.ejemplo.example%2Fanuncio",
        )
        .unwrap();
        assert_eq!(h, "abcdefabcdefabcdefabcdefabcdefabcdefabcd");
        assert_eq!(n, "Una cosa");
        assert_eq!(t, "https://tracker.ejemplo.example/anuncio");
        // Base32: 32 caracteres, los mismos 20 bytes.
        let (_, h, _) = de_magnet("magnet:?xt=urn:btih:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").unwrap();
        assert_eq!(h, "0".repeat(40));
        assert!(de_magnet("magnet:?dn=sin+hash").is_none());
    }

    #[test]
    fn la_huella_es_fija_y_tiene_40_cifras() {
        assert_eq!(huella(b"uno"), huella(b"uno"));
        assert_ne!(huella(b"uno"), huella(b"dos"));
        assert_eq!(huella(b"uno").len(), 40);
    }
}
