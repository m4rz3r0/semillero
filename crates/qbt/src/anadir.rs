//! Añadir torrents (`torrents/add`) y leer las categorías
//! (`torrents/categories`).
//!
//! `torrents/add` va en `multipart/form-data`: los `.torrent` en campos
//! `torrents`, los enlaces magnet en `urls` (uno por línea) y la categoría
//! en `category`. qBittorrent contesta «Ok.» o «Fails.» (y 415 a un
//! `.torrent` que no entiende); las versiones nuevas pueden contestar JSON
//! con cuántos añadió y cuántos no.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{Error, MAX_RESPUESTA, Qbt, texto_limitado};

/// Un archivo `.torrent` para [`Qbt::anadir`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchivoTorrent {
    /// Nombre del archivo (solo para qBittorrent; no se usa como ruta).
    pub nombre: String,
    pub datos: Vec<u8>,
}

#[derive(Deserialize)]
struct Recuento {
    #[serde(default)]
    failure_count: u64,
}

/// El nombre de archivo dentro de la cabecera de una parte: sin comillas,
/// barras ni caracteres de control, que romperían la cabecera.
fn nombre_seguro(nombre: &str) -> String {
    let s: String = nombre
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '"' | '\\' | '/'))
        .take(200)
        .collect();
    if s.trim().is_empty() {
        "archivo.torrent".to_owned()
    } else {
        s
    }
}

/// Un separador que no aparece en ninguna de las partes.
fn separador(partes: &[&[u8]]) -> String {
    let mut n = 0u32;
    loop {
        let s = format!("semillero-separador-{n:08x}");
        if !partes
            .iter()
            .any(|p| p.windows(s.len()).any(|w| w == s.as_bytes()))
        {
            return s;
        }
        n += 1;
    }
}

/// El cuerpo `multipart/form-data` y su separador.
pub(crate) fn cuerpo_multipart(
    archivos: &[ArchivoTorrent],
    urls: &str,
    categoria: Option<&str>,
) -> (String, Vec<u8>) {
    let mut contenidos: Vec<&[u8]> = archivos.iter().map(|a| a.datos.as_slice()).collect();
    contenidos.push(urls.as_bytes());
    if let Some(c) = categoria {
        contenidos.push(c.as_bytes());
    }
    let sep = separador(&contenidos);
    let mut cuerpo = Vec::new();
    let mut campo = |cabeceras: String, datos: &[u8]| {
        cuerpo.extend_from_slice(format!("--{sep}\r\n{cabeceras}\r\n\r\n").as_bytes());
        cuerpo.extend_from_slice(datos);
        cuerpo.extend_from_slice(b"\r\n");
    };
    for a in archivos {
        campo(
            format!(
                "Content-Disposition: form-data; name=\"torrents\"; filename=\"{}\"\r\n\
                 Content-Type: application/x-bittorrent",
                nombre_seguro(&a.nombre)
            ),
            &a.datos,
        );
    }
    if !urls.is_empty() {
        campo(
            "Content-Disposition: form-data; name=\"urls\"".to_owned(),
            urls.as_bytes(),
        );
    }
    if let Some(c) = categoria {
        campo(
            "Content-Disposition: form-data; name=\"category\"".to_owned(),
            c.as_bytes(),
        );
    }
    cuerpo.extend_from_slice(format!("--{sep}--\r\n").as_bytes());
    (sep, cuerpo)
}

/// ¿Dice la respuesta de `torrents/add` que se añadió todo?
fn anadido(cuerpo: &str) -> bool {
    let cuerpo = cuerpo.trim();
    if cuerpo == "Fails." {
        return false;
    }
    match serde_json::from_str::<Recuento>(cuerpo) {
        Ok(r) => r.failure_count == 0,
        Err(_) => true,
    }
}

impl Qbt {
    /// Los nombres de las categorías de qBittorrent, en orden.
    pub async fn categorias(&self) -> Result<Vec<String>, Error> {
        let texto = self.leer("/api/v2/torrents/categories", &[]).await?;
        let mapa: BTreeMap<String, serde_json::Value> = Self::json(&texto)?;
        Ok(mapa.into_keys().collect())
    }

    /// Añade los `archivos` y los enlaces `magnets` (que quien llama ya ha
    /// comprobado), con la `categoria` si se da. Si qBittorrent no añade
    /// alguno, [`Error::NoAnadido`].
    pub async fn anadir(
        &self,
        archivos: &[ArchivoTorrent],
        magnets: &[String],
        categoria: Option<&str>,
    ) -> Result<(), Error> {
        if archivos.is_empty() && magnets.is_empty() {
            return Ok(());
        }
        let url = format!("{}/api/v2/torrents/add", self.base);
        let (sep, cuerpo) = cuerpo_multipart(archivos, &magnets.join("\n"), categoria);
        let tipo = format!("multipart/form-data; boundary={sep}");
        let r = self
            .pedir(|c| {
                c.post(&url)
                    .header(reqwest::header::CONTENT_TYPE, &tipo)
                    .body(cuerpo.clone())
            })
            .await;
        let r = match r {
            Ok(r) => r,
            // 415: un `.torrent` que qBittorrent no entiende.
            Err(Error::Rechazado(415)) => return Err(Error::NoAnadido),
            Err(e) => return Err(e),
        };
        if anadido(&texto_limitado(r, MAX_RESPUESTA).await?) {
            Ok(())
        } else {
            Err(Error::NoAnadido)
        }
    }
}

#[cfg(test)]
mod pruebas {
    use std::net::{IpAddr, Ipv4Addr};

    use qbt_sim::{Conf, arrancar};

    use super::*;

    const LOCAL: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

    fn magnet() -> String {
        format!("magnet:?xt=urn:btih:{HASH}&dn=Una+pel%C3%ADcula")
    }

    fn archivo() -> ArchivoTorrent {
        ArchivoTorrent {
            nombre: "otra.torrent".into(),
            datos: b"d4:infod4:name4:otra12:piece lengthi16384eee".to_vec(),
        }
    }

    async fn sim() -> qbt_sim::Sim {
        arrancar(Conf {
            lista_blanca: vec![LOCAL],
            ..Default::default()
        })
        .await
    }

    #[test]
    fn respuestas_de_torrents_add() {
        assert!(anadido("Ok."));
        assert!(anadido(""));
        assert!(!anadido("Fails."));
        assert!(!anadido("Fails.\n"));
        assert!(anadido(r#"{"success_count":2,"failure_count":0}"#));
        assert!(!anadido(r#"{"success_count":1,"failure_count":1}"#));
    }

    #[test]
    fn el_separador_no_sale_en_las_partes() {
        let dentro = b"--semillero-separador-00000000 dentro".to_vec();
        let a = ArchivoTorrent {
            nombre: "a\"b\r\n.torrent".into(),
            datos: dentro,
        };
        let (sep, cuerpo) = cuerpo_multipart(&[a], "", None);
        assert_ne!(sep, "semillero-separador-00000000");
        let texto = String::from_utf8_lossy(&cuerpo);
        assert!(texto.contains("filename=\"ab.torrent\""), "{texto}");
        assert!(texto.ends_with(&format!("--{sep}--\r\n")));
    }

    #[tokio::test]
    async fn lee_las_categorias_en_orden() {
        let s = sim().await;
        s.control
            .fijar_categorias(vec!["series".into(), "peliculas".into()]);
        let q = Qbt::new(&s.url, LOCAL);
        assert_eq!(q.categorias().await.unwrap(), ["peliculas", "series"]);
    }

    #[tokio::test]
    async fn anade_un_magnet_y_un_archivo_con_su_categoria() {
        let s = sim().await;
        s.control.fijar_categorias(vec!["peliculas".into()]);
        let q = Qbt::new(&s.url, LOCAL);
        q.anadir(&[archivo()], &[magnet()], Some("peliculas"))
            .await
            .unwrap();
        let ts = q.torrents().await.unwrap();
        assert_eq!(ts.len(), 2, "{ts:?}");
        let m = ts.iter().find(|t| t.hash == HASH).expect("el del magnet");
        assert_eq!(m.name, "Una película");
        assert_eq!(m.category, "peliculas");
        assert!(
            ts.iter()
                .any(|t| t.name == "otra" && t.category == "peliculas")
        );
        let l = s.control.llamadas();
        let add = l.iter().find(|l| l.ruta == "/api/v2/torrents/add").unwrap();
        assert!(add.cuerpo.contains("category=peliculas"), "{}", add.cuerpo);
        assert!(
            add.cuerpo.contains("torrents=otra.torrent"),
            "{}",
            add.cuerpo
        );
    }

    #[tokio::test]
    async fn sin_categoria_no_manda_el_campo() {
        let s = sim().await;
        let q = Qbt::new(&s.url, LOCAL);
        q.anadir(&[], &[magnet()], None).await.unwrap();
        let ts = q.torrents().await.unwrap();
        assert_eq!(ts[0].category, "");
        let l = s.control.llamadas();
        let add = l.iter().find(|l| l.ruta == "/api/v2/torrents/add").unwrap();
        assert!(!add.cuerpo.contains("category="), "{}", add.cuerpo);
    }

    #[tokio::test]
    async fn fails_y_415_son_no_anadido() {
        let s = sim().await;
        let q = Qbt::new(&s.url, LOCAL);
        s.control.fallar_al_anadir(true);
        assert!(matches!(
            q.anadir(&[], &[magnet()], None).await,
            Err(Error::NoAnadido)
        ));
        s.control.fallar_al_anadir(false);
        let malo = ArchivoTorrent {
            nombre: "malo.torrent".into(),
            datos: b"esto no es un torrent".to_vec(),
        };
        assert!(matches!(
            q.anadir(&[malo], &[], None).await,
            Err(Error::NoAnadido)
        ));
        assert!(q.torrents().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn con_contrasena_entra_y_repite() {
        let s = arrancar(Conf::default()).await;
        let q = Qbt::con_cuenta(&s.url, None, crate::Cuenta::new("admin", "secreta"));
        q.anadir(&[], &[magnet()], None).await.unwrap();
        assert_eq!(q.torrents().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn nada_que_anadir_no_pregunta() {
        let s = sim().await;
        let q = Qbt::new(&s.url, LOCAL);
        q.anadir(&[], &[], Some("peliculas")).await.unwrap();
        assert!(s.control.llamadas().is_empty());
    }
}
