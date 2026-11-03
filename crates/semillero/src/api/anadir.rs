//! Lo que se comprueba antes de pedir a qBittorrent que añada algo: que un
//! `.torrent` sea un diccionario bencode con su diccionario `info` y que un
//! enlace sea un magnet con el hash del torrent. Nada de direcciones web.
//!
//! Los mensajes nunca repiten el enlace (sus `tr=` pueden llevar la clave
//! del anuncio): dicen el número de línea o el nombre del archivo.

use sha1::Digest;

/// Niveles de anidación de un `.torrent`, como mucho.
const PROFUNDIDAD: usize = 64;
/// Caracteres de un nombre que se enseñan, como mucho.
const MAX_NOMBRE: usize = 200;

/// Un `.torrent` que vale.
#[derive(Debug, PartialEq, Eq)]
pub struct TorrentBueno {
    /// El hash con que lo conoce qBittorrent: SHA-1 de `info` (v1 o
    /// híbrido) o los 40 primeros caracteres de su SHA-256 (solo v2).
    pub hash: String,
    /// El `name` de `info`, si lo trae.
    pub nombre: Option<String>,
}

/// Un enlace magnet que vale.
#[derive(Debug, PartialEq, Eq)]
pub struct MagnetBueno {
    pub enlace: String,
    /// El hash con que lo conoce qBittorrent (40 caracteres hexadecimales).
    pub hash: String,
    /// Su `dn` o, si no lo trae, el hash.
    pub nombre: String,
}

/// Una cadena bencode en `i` (`<largo>:<bytes>`) y dónde acaba.
fn cadena(d: &[u8], i: usize) -> Option<(&[u8], usize)> {
    let dos = i + d.get(i..)?.iter().take(12).position(|&b| b == b':')?;
    let cifras = &d[i..dos];
    if cifras.is_empty() || !cifras.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let n: usize = std::str::from_utf8(cifras).ok()?.parse().ok()?;
    let fin = (dos + 1).checked_add(n)?;
    Some((d.get(dos + 1..fin)?, fin))
}

/// Salta el valor bencode que empieza en `i`; devuelve dónde acaba.
fn saltar(d: &[u8], i: usize, nivel: usize) -> Option<usize> {
    if nivel > PROFUNDIDAD {
        return None;
    }
    match *d.get(i)? {
        b'i' => {
            let fin = i + 1 + d.get(i + 1..)?.iter().position(|&b| b == b'e')?;
            let n = d[i + 1..fin].strip_prefix(b"-").unwrap_or(&d[i + 1..fin]);
            (!n.is_empty() && n.iter().all(u8::is_ascii_digit)).then_some(fin + 1)
        }
        b'l' => {
            let mut j = i + 1;
            while *d.get(j)? != b'e' {
                j = saltar(d, j, nivel + 1)?;
            }
            Some(j + 1)
        }
        b'd' => {
            let mut j = i + 1;
            while *d.get(j)? != b'e' {
                j = cadena(d, j)?.1;
                j = saltar(d, j, nivel + 1)?;
            }
            Some(j + 1)
        }
        b'0'..=b'9' => cadena(d, i).map(|(_, fin)| fin),
        _ => None,
    }
}

/// Las claves de primer nivel del diccionario de `d` (entero), con el
/// valor de cada una.
fn claves(d: &[u8]) -> Option<Vec<(&[u8], &[u8])>> {
    if d.first() != Some(&b'd') {
        return None;
    }
    let mut v = Vec::new();
    let mut j = 1;
    while *d.get(j)? != b'e' {
        let (clave, ini) = cadena(d, j)?;
        j = saltar(d, ini, 1)?;
        v.push((clave, &d[ini..j]));
    }
    (j + 1 == d.len()).then_some(v)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Sin caracteres de control y con [`MAX_NOMBRE`] caracteres como mucho.
pub fn limpio(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .take(MAX_NOMBRE)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// ¿Es `d` un `.torrent`: un diccionario bencode, sin nada detrás, con un
/// diccionario `info`?
pub fn torrent(d: &[u8]) -> Option<TorrentBueno> {
    let (_, info) = claves(d)?.into_iter().find(|(k, _)| *k == b"info")?;
    let de_info = claves(info)?;
    let v1 = de_info.iter().any(|(k, _)| *k == b"pieces");
    let hash = if v1 {
        hex(&sha1::Sha1::digest(info))
    } else {
        let mut h = hex(&sha2::Sha256::digest(info));
        h.truncate(40);
        h
    };
    let nombre = de_info
        .iter()
        .find(|(k, _)| *k == b"name")
        .and_then(|(_, v)| cadena(v, 0))
        .map(|(n, _)| limpio(&String::from_utf8_lossy(n)))
        .filter(|n| !n.is_empty());
    Some(TorrentBueno { hash, nombre })
}

/// `%xx` y `+` de un valor de la consulta.
fn decodificar(s: &str) -> String {
    let b = s.as_bytes();
    let mut v = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let byte = (b[i] == b'%')
            .then(|| s.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (byte, b[i]) {
            (Some(x), _) => {
                v.push(x);
                i += 3;
                continue;
            }
            (None, b'+') => v.push(b' '),
            (None, c) => v.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&v).into_owned()
}

/// 32 caracteres en base32 (RFC 4648) a 40 hexadecimales.
fn base32_a_hex(s: &str) -> Option<String> {
    let mut bits = 0u32;
    let mut n = 0;
    let mut v = Vec::with_capacity(20);
    for c in s.bytes() {
        let x = match c.to_ascii_uppercase() {
            b @ b'A'..=b'Z' => b - b'A',
            b @ b'2'..=b'7' => b - b'2' + 26,
            _ => return None,
        };
        bits = (bits << 5 | u32::from(x)) & 0xfff;
        n += 5;
        if n >= 8 {
            n -= 8;
            v.push((bits >> n) as u8);
        }
    }
    Some(hex(&v))
}

fn es_hex(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// El hash de un `xt`: `urn:btih:` con 40 hexadecimales o 32 en base32, o
/// `urn:btmh:` con un SHA-256 (`1220` y 64 hexadecimales).
fn hash_de_xt(xt: &str) -> Option<String> {
    let xt = xt.to_ascii_lowercase();
    if let Some(h) = xt.strip_prefix("urn:btih:") {
        return match h.len() {
            40 if es_hex(h) => Some(h.to_owned()),
            32 => base32_a_hex(h),
            _ => None,
        };
    }
    let h = xt.strip_prefix("urn:btmh:1220")?;
    (h.len() == 64 && es_hex(h)).then(|| h[..40].to_owned())
}

/// Comprueba la línea `n` (desde 1) de los enlaces. El error es la frase
/// que lo explica, sin el enlace.
pub fn magnet(linea: &str, n: usize) -> Result<MagnetBueno, String> {
    let minusculas = linea.to_ascii_lowercase();
    if minusculas.starts_with("http://") || minusculas.starts_with("https://") {
        return Err(format!(
            "La línea {n} es una dirección web: semillero solo acepta enlaces magnet y \
             archivos .torrent."
        ));
    }
    if linea.len() > crate::api::MAX_BYTES_MAGNET {
        return Err(format!(
            "La línea {n} es demasiado larga para un enlace magnet."
        ));
    }
    let Some(consulta) = minusculas
        .starts_with("magnet:?")
        .then(|| &linea["magnet:?".len()..])
    else {
        return Err(format!("La línea {n} no es un enlace magnet."));
    };
    let mut hashes = Vec::new();
    let mut nombre = None;
    for par in consulta.split('&') {
        let (k, v) = par.split_once('=').unwrap_or((par, ""));
        match k {
            "xt" => hashes.extend(hash_de_xt(&decodificar(v))),
            "dn" => nombre = Some(limpio(&decodificar(v))),
            _ => {}
        }
    }
    // Con `btih` y `btmh` (híbrido), qBittorrent lo conoce por el v1.
    let Some(hash) = hashes.first().cloned() else {
        return Err(format!(
            "La línea {n} no es un enlace magnet válido: le falta el hash del torrent \
             (xt=urn:btih:…)."
        ));
    };
    Ok(MagnetBueno {
        enlace: linea.to_owned(),
        nombre: nombre
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| hash.clone()),
        hash,
    })
}

#[cfg(test)]
mod pruebas {
    use super::*;

    const H: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn un_torrent_v1_tiene_el_sha1_de_info() {
        let info = b"d4:name3:abc6:pieces0:e";
        let mut t = b"d4:info".to_vec();
        t.extend_from_slice(info);
        t.push(b'e');
        let b = torrent(&t).unwrap();
        assert_eq!(b.hash, hex(&sha1::Sha1::digest(info)));
        assert_eq!(b.nombre.as_deref(), Some("abc"));
    }

    #[test]
    fn un_torrent_solo_v2_tiene_el_sha256_cortado() {
        let info = b"d12:meta versioni2e4:name1:xe";
        let mut t = b"d4:info".to_vec();
        t.extend_from_slice(info);
        t.push(b'e');
        let b = torrent(&t).unwrap();
        assert_eq!(b.hash, hex(&sha2::Sha256::digest(info))[..40]);
    }

    #[test]
    fn lo_que_no_es_un_torrent() {
        for malo in [
            &b""[..],
            b"d",
            b"de",
            b"d4:infoi1ee",
            b"d4:infodee  ",
            b"d4:infod4:name9:corto",
            b"d4:infodi1e1:xee",
            b"d4:infod1:xi-ee",
            b"d99999999999999999999:xe",
        ] {
            assert!(torrent(malo).is_none(), "{}", String::from_utf8_lossy(malo));
        }
        // Anidado sin fin: no se desborda la pila.
        let mut hondo = b"d4:infod1:x".to_vec();
        hondo.extend(std::iter::repeat_n(b'l', 100_000));
        assert!(torrent(&hondo).is_none());
        assert!(torrent(b"d4:infodee").is_some());
    }

    #[test]
    fn magnets_que_valen() {
        let m = magnet(
            &format!("magnet:?xt=urn:btih:{}&dn=Una+cosa%21", H.to_uppercase()),
            1,
        )
        .unwrap();
        assert_eq!(m.hash, H);
        assert_eq!(m.nombre, "Una cosa!");
        let m = magnet(&format!("MAGNET:?xt=urn%3Abtih%3A{H}"), 1).unwrap();
        assert_eq!(m.nombre, H);
        let m = magnet(&format!("magnet:?xt=urn:btih:{}", "a".repeat(32)), 1).unwrap();
        assert_eq!(m.hash, "0".repeat(40));
        // 'b' = 1 en base32: 00001 repetido.
        let m = magnet(&format!("magnet:?xt=urn:btih:{}", "B".repeat(32)), 1).unwrap();
        assert_eq!(m.hash, "0842108421".repeat(4));
        let v2 = "d".repeat(64);
        let m = magnet(&format!("magnet:?xt=urn:btmh:1220{v2}"), 1).unwrap();
        assert_eq!(m.hash, v2[..40]);
    }

    #[test]
    fn magnets_que_no_valen_y_no_se_repiten() {
        for (linea, palabra) in [
            ("https://ejemplo.example/a.torrent", "dirección web"),
            ("HTTP://ejemplo.example/a", "dirección web"),
            ("ejemplo.example", "no es un enlace magnet"),
            ("magnet:?dn=ejemplo.example", "le falta el hash"),
            ("magnet:?xt=urn:btih:ejemplo.example", "le falta el hash"),
            ("magnet:?xt=urn:sha1:0123", "le falta el hash"),
        ] {
            let e = magnet(linea, 7).unwrap_err();
            assert!(e.contains("línea 7"), "{e}");
            assert!(e.contains(palabra), "{linea}: {e}");
            assert!(!e.contains("ejemplo.example"), "{e}");
        }
        let largo = format!("magnet:?xt=urn:btih:{H}&tr={}", "x".repeat(9000));
        assert!(magnet(&largo, 1).unwrap_err().contains("demasiado larga"));
    }
}
