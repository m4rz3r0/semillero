//! De punta a punta con un qBittorrent de verdad (no el simulador): entrar,
//! clasificar, avisar, cancelar un aviso y borrar en modo real, por las
//! server functions de la web y la tarea horaria.
//!
//! Solo con `SEMILLERO_QBT_REAL` (la WebUI, p. ej. `http://127.0.0.1:18080`);
//! sin ella cada prueba se salta y `cargo test` queda como siempre. Además:
//!
//! | Variable | Qué es | Por defecto |
//! |---|---|---|
//! | `SEMILLERO_QBT_REAL_CLAVE` | fichero con la contraseña de la WebUI | (obligatoria) |
//! | `SEMILLERO_QBT_REAL_USUARIO` | usuario de la WebUI | `admin` |
//! | `SEMILLERO_QBT_REAL_DATOS` | directorio de este equipo donde se dejan los datos que siembra | (obligatoria) |
//! | `SEMILLERO_QBT_REAL_RUTA` | ese mismo directorio visto desde qBittorrent | `/downloads` |
//!
//! `tools/e2e/qbittorrent.sh` arranca uno en un contenedor y escribe esas
//! variables. Tiene que ser uno de usar y tirar: la prueba borra de verdad,
//! y no empieza si qBittorrent tiene torrents que no son suyos.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::http::StatusCode;
use reglas::preset::Preset;
use reglas::{Disco, Estado, TipoRegla};
use semillero::api::DatosPortada;
use semillero::auth::{Canario, comprobar_canario};
use semillero::config::ConfCuentaQbt;
use semillero::db::{self, Evento, Modo};
use semillero::estado_red::EstadoRed;
use semillero::tarea;
use serde_json::{Value, json};
use sha1::{Digest, Sha1};

use crate::comun::{self, Prueba};
use crate::interfaz::Ui;

/// Las pruebas que ponen torrents en qBittorrent van de una en una: cada
/// una empieza quitando lo que dejó la anterior.
static UNA_A_UNA: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Prefijo de los torrents de la prueba en qBittorrent.
const PREFIJO: &str = "semillero-e2e-";
const HORA: i64 = 3600;
const RED_OK: EstadoRed = EstadoRed {
    vigilada: true,
    vpn: true,
    puerto: Some(51413),
};

/// Todo recién sembrado cumple (0 días, ratio 0) y no se para nunca; el aviso
/// vence en una hora (lo mínimo); sin categoría, se recoge si queda menos
/// del 90 % del disco libre.
const PRESET: &str = r#"formato = 1
nombre = "Punta a punta"
descripcion = "Cumple al momento; aviso de una hora; recoge sin espacio."

[normas]
umbral_pct = 0.0
siembra_dias = 0
ratio_min = 0.0
plazo_largo_dias = 0
ratio_pausa = 100.0
aviso_horas = 1

[categorias.sin_categoria]
disco_libre_pct = 90.0
"#;

/// El qBittorrent de verdad, según las variables de entorno.
struct Real {
    url: String,
    usuario: String,
    clave: String,
    fichero_clave: PathBuf,
    datos: PathBuf,
    ruta: String,
}

/// `None` (y se dice) si no hay `SEMILLERO_QBT_REAL`.
fn real(prueba: &str) -> Option<Real> {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    let Some(url) = var("SEMILLERO_QBT_REAL") else {
        eprintln!("{prueba}: saltada, sin SEMILLERO_QBT_REAL");
        return None;
    };
    let hace_falta = |k: &str| var(k).unwrap_or_else(|| panic!("con SEMILLERO_QBT_REAL falta {k}"));
    let fichero_clave = PathBuf::from(hace_falta("SEMILLERO_QBT_REAL_CLAVE"));
    let clave = std::fs::read_to_string(&fichero_clave)
        .unwrap()
        .trim()
        .to_owned();
    Some(Real {
        url: url.trim_end_matches('/').to_owned(),
        usuario: var("SEMILLERO_QBT_REAL_USUARIO").unwrap_or_else(|| "admin".into()),
        clave,
        fichero_clave,
        datos: hace_falta("SEMILLERO_QBT_REAL_DATOS").into(),
        ruta: var("SEMILLERO_QBT_REAL_RUTA").unwrap_or_else(|| "/downloads".into()),
    })
}

impl Real {
    /// semillero en modo con contraseña contra este qBittorrent.
    async fn semillero(&self) -> Prueba {
        comun::con_qbt(
            &self.url,
            ConfCuentaQbt {
                usuario: self.usuario.clone(),
                clave: self.fichero_clave.clone(),
            },
        )
        .await
    }
}

/// La API de qBittorrent sin pasar por semillero: para poner los torrents
/// de la prueba y ver qué queda de verdad.
struct Directo {
    url: String,
    http: reqwest::Client,
    cookie: String,
}

impl Directo {
    async fn entrar(r: &Real) -> Self {
        let http = reqwest::Client::new();
        let resp = http
            .post(format!("{}/api/v2/auth/login", r.url))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!(
                "username={}&password={}",
                r.usuario,
                form_urlencoded_valor(&r.clave)
            ))
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success(), "login: {}", resp.status());
        let cookie = resp.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        Self {
            url: r.url.clone(),
            http,
            cookie,
        }
    }

    async fn torrents(&self) -> Vec<Value> {
        let resp = self
            .http
            .get(format!("{}/api/v2/torrents/info", self.url))
            .header("cookie", &self.cookie)
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success(), "info: {}", resp.status());
        serde_json::from_str(&resp.text().await.unwrap()).unwrap()
    }

    async fn torrent(&self, hash: &str) -> Option<Value> {
        self.torrents()
            .await
            .into_iter()
            .find(|t| t["hash"] == hash)
    }

    /// Añade el `.torrent` con sus datos ya en `ruta` (sin gestión
    /// automática, para que la ruta valga).
    async fn anadir(&self, archivo: &[u8], ruta: &str) {
        const LINDE: &str = "semillero-e2e-linde";
        let campo = |nombre: &str, valor: &str| {
            format!(
                "--{LINDE}\r\nContent-Disposition: form-data; name=\"{nombre}\"\r\n\r\n{valor}\r\n"
            )
        };
        let mut cuerpo = format!(
            "--{LINDE}\r\nContent-Disposition: form-data; name=\"torrents\"; \
             filename=\"prueba.torrent\"\r\nContent-Type: application/x-bittorrent\r\n\r\n"
        )
        .into_bytes();
        cuerpo.extend_from_slice(archivo);
        cuerpo.extend_from_slice(b"\r\n");
        for (n, v) in [
            ("savepath", ruta),
            ("autoTMM", "false"),
            ("contentLayout", "Original"),
        ] {
            cuerpo.extend_from_slice(campo(n, v).as_bytes());
        }
        cuerpo.extend_from_slice(format!("--{LINDE}--\r\n").as_bytes());
        let resp = self
            .http
            .post(format!("{}/api/v2/torrents/add", self.url))
            .header("cookie", &self.cookie)
            .header(
                "content-type",
                format!("multipart/form-data; boundary={LINDE}"),
            )
            .body(cuerpo)
            .send()
            .await
            .unwrap();
        let estado = resp.status();
        let texto = resp.text().await.unwrap();
        assert!(
            estado.is_success() && texto.trim() != "Fails.",
            "add: {estado} {texto}"
        );
    }

    async fn borrar(&self, hashes: &[String]) {
        if hashes.is_empty() {
            return;
        }
        let resp = self
            .http
            .post(format!("{}/api/v2/torrents/delete", self.url))
            .header("cookie", &self.cookie)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!("hashes={}&deleteFiles=true", hashes.join("%7C")))
            .send()
            .await
            .unwrap();
        assert!(resp.status().is_success(), "delete: {}", resp.status());
    }

    /// Quita lo que dejó una ejecución anterior; con cualquier otro
    /// torrent, se niega a seguir.
    async fn limpiar(&self) {
        let mut nuestros = Vec::new();
        for t in self.torrents().await {
            let nombre = t["name"].as_str().unwrap_or_default();
            assert!(
                nombre.starts_with(PREFIJO),
                "qBittorrent tiene un torrent que no es de la prueba ({nombre}); \
                 la prueba borra de verdad: usa uno de usar y tirar"
            );
            nuestros.push(t["hash"].as_str().unwrap().to_owned());
        }
        self.borrar(&nuestros).await;
    }
}

/// `valor` escapado para un formulario (la contraseña al azar es hex, pero
/// por si acaso).
fn form_urlencoded_valor(valor: &str) -> String {
    valor
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Repite `cond` hasta que se cumpla, como mucho `s` segundos.
async fn esperar<F: Future<Output = bool>>(s: u64, que: &str, mut cond: impl FnMut() -> F) {
    for _ in 0..s * 4 {
        if cond().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("tras {s} s: {que}");
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Un torrent de un fichero (privado: sin DHT ni PeX) con sus datos en
/// `dir/nombre`; devuelve el `.torrent` y su hash.
fn crear_torrent(dir: &Path, nombre: &str) -> (Vec<u8>, String) {
    crear_torrent_con(dir, nombre, None)
}

/// [`crear_torrent`] con `anuncio` como tracker.
fn crear_torrent_con(dir: &Path, nombre: &str, anuncio: Option<&str>) -> (Vec<u8>, String) {
    const PIEZA: usize = 16 * 1024;
    // 40 000 bytes que no se repiten de una prueba a otra (van con el nombre).
    let mut x: u32 = nombre
        .bytes()
        .fold(2166136261, |h, b| (h ^ u32::from(b)).wrapping_mul(16777619));
    let datos: Vec<u8> = (0..40_000)
        .map(|_| {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            (x >> 16) as u8
        })
        .collect();
    std::fs::write(dir.join(nombre), &datos).unwrap();
    let piezas: Vec<u8> = datos.chunks(PIEZA).flat_map(Sha1::digest).collect();
    // Bencode, con las claves en orden.
    let mut info = format!(
        "d6:lengthi{}e4:name{}:{nombre}12:piece lengthi{PIEZA}e6:pieces{}:",
        datos.len(),
        nombre.len(),
        piezas.len()
    )
    .into_bytes();
    info.extend_from_slice(&piezas);
    info.extend_from_slice(b"7:privatei1ee");
    let hash = hex(&Sha1::digest(&info));
    // Bencode: «announce» va antes que «info».
    let mut archivo = match anuncio {
        Some(a) => format!("d8:announce{}:{a}4:info", a.len()).into_bytes(),
        None => b"d4:info".to_vec(),
    };
    archivo.extend_from_slice(&info);
    archivo.push(b'e');
    (archivo, hash)
}

/// Disco casi lleno para todas las categorías: la regla de espacio elige.
fn disco_lleno(_: &Path) -> Option<Disco> {
    Some(Disco {
        libre: 0,
        total: 1 << 40,
    })
}

struct SinMediateca;
impl semillero::mediateca::Mediateca for SinMediateca {
    async fn olvidar(&self, _t: &qbt::TorrentQbt) -> Result<(), String> {
        Ok(())
    }
}

async fn pasar(u: &Ui, ahora: i64) -> tarea::Informe {
    tarea::pasada(&u.p.state, &SinMediateca, &disco_lleno, ahora, &RED_OK, 0).await
}

async fn portada(u: &Ui) -> DatosPortada {
    let (s, j) = u.llamar("/api/portada", json!({})).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    serde_json::from_str(&j).unwrap()
}

async fn eventos(u: &Ui) -> Vec<(String, Evento, Option<TipoRegla>)> {
    db::historial(&u.p.state.db, 100)
        .await
        .unwrap()
        .into_iter()
        .map(|a| (a.hash, a.evento, a.regla))
        .collect()
}

fn ahora() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[tokio::test]
async fn real_entra_con_la_cuenta_de_qbittorrent() {
    let Some(r) = real("real_entra_con_la_cuenta_de_qbittorrent") else {
        return;
    };
    let p = r.semillero().await;
    // qBittorrent rechaza una contraseña al azar: el acceso queda abierto.
    assert_eq!(comprobar_canario(&p.state).await, Canario::Bueno);

    let resp = comun::login(&p, &r.usuario, &r.clave).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let cookie = resp.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    assert!(cookie.starts_with("__Host-sm_session="), "{cookie}");
    let filas: Vec<String> = sqlx::query_scalar("SELECT usuario FROM sesiones")
        .fetch_all(&p.state.db)
        .await
        .unwrap();
    assert_eq!(filas, vec![r.usuario.clone()]);
    let resp = comun::enviar(&p.app, comun::get("/", Some(&cookie))).await;
    assert_eq!(resp.status(), StatusCode::OK);
    // Con una contraseña mala, no.
    let resp = comun::login(&p, &r.usuario, "no-es-la-clave").await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert!(resp.headers().get("set-cookie").is_none());
    let cuerpo = comun::texto(resp).await;
    assert!(
        cuerpo.contains("Usuario o contraseña incorrectos"),
        "{cuerpo}"
    );
}

#[tokio::test]
async fn real_clasifica_avisa_cancela_y_borra() {
    let Some(r) = real("real_clasifica_avisa_cancela_y_borra") else {
        return;
    };
    let _turno = UNA_A_UNA.lock().await;
    let qbt = Directo::entrar(&r).await;
    qbt.limpiar().await;
    let marca = format!("{PREFIJO}{}-{}", std::process::id(), ahora());
    let (nombre_a, nombre_b) = (format!("{marca}-a.bin"), format!("{marca}-b.bin"));
    let (torrent_a, a) = crear_torrent(&r.datos, &nombre_a);
    let (torrent_b, b) = crear_torrent(&r.datos, &nombre_b);
    for t in [&torrent_a, &torrent_b] {
        qbt.anadir(t, &r.ruta).await;
    }
    // Comprueba los datos y se pone a sembrar sin bajar nada.
    for h in [&a, &b] {
        esperar(60, &format!("{h} sembrando"), || async {
            qbt.torrent(h).await.is_some_and(|t| {
                t["progress"] == 1.0
                    && ["uploading", "stalledUP", "queuedUP", "forcedUP"]
                        .contains(&t["state"].as_str().unwrap_or_default())
            })
        })
        .await;
    }

    // Entrar.
    let p = r.semillero().await;
    let resp = comun::login(&p, &r.usuario, &r.clave).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let cookie = resp.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let token = comun::token(&p, &cookie).await;
    let u = Ui { p, cookie, token };

    // El preset, subido y aplicado desde la página de reglas.
    let (s, cuerpo) = u
        .llamar("/api/subir_preset", json!({ "archivo": PRESET }))
        .await;
    assert_eq!(s, StatusCode::OK, "{cuerpo}");
    let (s, cuerpo) = u.llamar("/api/reglas", json!({})).await;
    assert_eq!(s, StatusCode::OK);
    let v: Value = serde_json::from_str(&cuerpo).unwrap();
    let id = v["presets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["nombre"] == "Punta a punta")
        .expect("el preset subido")["id"]
        .as_i64()
        .unwrap();
    let (s, _) = u.llamar("/api/aplicar_preset", json!({ "id": id })).await;
    assert_eq!(s, StatusCode::OK);
    let preset = Preset::leer(PRESET).unwrap();
    assert_eq!(db::normas(&u.p.state.db).await.unwrap(), preset.normas);
    assert_eq!(
        db::reglas(&u.p.state.db).await.unwrap()[""],
        preset.categorias[""]
    );

    // Modo real, con la contraseña de qBittorrent.
    let (s, _) = u
        .llamar(
            "/api/cambiar_modo",
            json!({ "real": true, "clave": r.clave }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(db::modo(&u.p.state.db).await.unwrap(), Modo::BorradoReal);

    // Clasificar: lo que dice qBittorrent, con las normas del preset.
    let d = portada(&u).await;
    assert!(d.modo_real);
    for (h, nombre) in [(&a, &nombre_a), (&b, &nombre_b)] {
        let f = d
            .torrents
            .iter()
            .find(|t| &t.hash == h)
            .unwrap_or_else(|| panic!("{h} en la portada"));
        assert_eq!(&f.nombre, nombre);
        assert_eq!(f.categoria, "Sin categoría");
        assert_eq!(f.estado, Estado::Cumplido, "{f:?}");
        assert_eq!(f.tamano, 40_000);
        assert_eq!(f.aviso, None);
    }

    // Avisar: la tarea horaria avisa de los dos por falta de espacio.
    let t0 = ahora();
    let inf = pasar(&u, t0).await;
    assert!(inf.qbt_ok && inf.errores.is_empty(), "{inf:?}");
    let mut avisados = inf.avisados.clone();
    avisados.sort();
    let mut esperados = vec![a.clone(), b.clone()];
    esperados.sort();
    assert_eq!(avisados, esperados);
    assert!(
        inf.borrados.is_empty() && inf.pausados.is_empty(),
        "{inf:?}"
    );
    let avisos = db::avisos(&u.p.state.db).await.unwrap();
    assert_eq!(avisos.len(), 2, "{avisos:?}");
    assert!(
        avisos
            .iter()
            .all(|x| x.regla == TipoRegla::Disco && x.creado_en == t0 && x.vence_en == t0 + HORA),
        "{avisos:?}"
    );
    let d = portada(&u).await;
    assert_eq!(d.avisos.len(), 2, "{:?}", d.avisos);
    assert!(
        d.torrents
            .iter()
            .filter(|t| t.hash == a || t.hash == b)
            .all(|t| t.aviso == Some(TipoRegla::Disco))
    );
    let ev = eventos(&u).await;
    for h in [&a, &b] {
        assert!(
            ev.contains(&(h.clone(), Evento::Aviso, Some(TipoRegla::Disco))),
            "{ev:?}"
        );
    }

    // Cancelar el aviso de B con «No recoger».
    let (s, _) = u
        .llamar("/api/no_borrar", json!({ "hash": b, "regla": "Disco" }))
        .await;
    assert_eq!(s, StatusCode::OK);
    let avisos = db::avisos(&u.p.state.db).await.unwrap();
    assert_eq!(avisos.iter().map(|x| &x.hash).collect::<Vec<_>>(), vec![&a]);
    assert!(
        db::exenciones(&u.p.state.db)
            .await
            .unwrap()
            .contains(&(b.clone(), TipoRegla::Disco))
    );
    assert_eq!(
        eventos(&u).await[0],
        (b.clone(), Evento::Cancelado, Some(TipoRegla::Disco))
    );
    let d = portada(&u).await;
    let ficha_b = d.torrents.iter().find(|t| t.hash == b).unwrap();
    assert_eq!(ficha_b.aviso, None);
    assert_eq!(ficha_b.exenciones, vec![TipoRegla::Disco]);

    // Antes de la hora no se borra nada.
    let inf = pasar(&u, t0 + HORA - 1).await;
    assert!(inf.borrados.is_empty(), "{inf:?}");
    assert!(qbt.torrent(&a).await.is_some());

    // Borrar: vence el aviso de A y se borra con sus ficheros, en
    // qBittorrent de verdad; B, con el aviso cancelado, se queda.
    let inf = pasar(&u, t0 + HORA).await;
    assert!(inf.errores.is_empty(), "{inf:?}");
    assert_eq!(inf.borrados, vec![a.clone()]);
    assert!(inf.avisados.is_empty(), "{inf:?}");
    assert!(db::avisos(&u.p.state.db).await.unwrap().is_empty());
    assert_eq!(
        eventos(&u).await[0],
        (a.clone(), Evento::Borrado, Some(TipoRegla::Disco))
    );
    esperar(30, "A fuera de qBittorrent", || async {
        qbt.torrent(&a).await.is_none()
    })
    .await;
    esperar(30, "los datos de A borrados", || async {
        !r.datos.join(&nombre_a).exists()
    })
    .await;
    assert!(qbt.torrent(&b).await.is_some());
    assert!(r.datos.join(&nombre_b).exists());
    let d = portada(&u).await;
    assert!(d.torrents.iter().all(|t| t.hash != a));
    assert!(d.torrents.iter().any(|t| t.hash == b));
    assert!(d.avisos.is_empty(), "{:?}", d.avisos);
    let historial = u.pagina("/historial").await;
    assert!(historial.contains(&nombre_a), "{historial}");

    qbt.borrar(std::slice::from_ref(&b)).await;
}

/// Un tracker de mentira en este equipo: contesta a cada anuncio sin pares y
/// cuenta los anuncios.
async fn tracker_de_prueba() -> (u16, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let anuncios = Arc::new(AtomicUsize::new(0));
    let cuenta = anuncios.clone();
    let app = axum::Router::new().route(
        "/announce",
        axum::routing::get(move || {
            let cuenta = cuenta.clone();
            async move {
                cuenta.fetch_add(1, Ordering::SeqCst);
                "d8:intervali60e5:peers0:e"
            }
        }),
    );
    let escucha = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let puerto = escucha.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(escucha, app).await.unwrap() });
    (puerto, anuncios)
}

#[tokio::test]
async fn real_anade_desde_semillero_con_las_normas_de_su_tracker() {
    use base64::Engine;
    use std::sync::atomic::Ordering;
    let Some(r) = real("real_anade_desde_semillero_con_las_normas_de_su_tracker") else {
        return;
    };
    let _turno = UNA_A_UNA.lock().await;
    let qbt = Directo::entrar(&r).await;
    qbt.limpiar().await;
    let (puerto, anuncios) = tracker_de_prueba().await;
    // El passkey va en el anuncio: semillero no debe enseñarlo ni guardarlo.
    const PASSKEY: &str = "e2epasskey0no0se0ve";
    let anuncio = format!("http://127.0.0.1:{puerto}/announce?passkey={PASSKEY}");
    let nombre = format!("{PREFIJO}{}-{}-t.bin", std::process::id(), ahora());
    let (archivo, hash) = crear_torrent_con(&r.datos, &nombre, Some(&anuncio));

    let p = r.semillero().await;
    let resp = comun::login(&p, &r.usuario, &r.clave).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let cookie = resp.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let token = comun::token(&p, &cookie).await;
    let u = Ui { p, cookie, token };

    // Normas propias para el tracker, distintas de las de por defecto.
    let propias = reglas::NormasTracker {
        aviso_horas: 5,
        siembra_dias: 3,
        ..reglas::NormasTracker::POR_DEFECTO
    };
    let (s, cuerpo) = u
        .llamar(
            "/api/guardar_normas_tracker",
            json!({ "tracker": "127.0.0.1", "normas": propias, "clave": null }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{cuerpo}");

    // Añadir el .torrent desde semillero.
    let datos = base64::engine::general_purpose::STANDARD.encode(&archivo);
    let (s, cuerpo) = u
        .llamar(
            "/api/anadir_torrents",
            json!({
                "archivos": [{ "nombre": "prueba.torrent", "datos": datos }],
                "magnets": "",
                "categoria": "",
            }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{cuerpo}");
    assert!(!cuerpo.contains(PASSKEY), "{cuerpo}");
    // qBittorrent da el tracker en cuanto lo tiene, aunque aún no lo haya
    // contactado; la prueba espera además al primer anuncio.
    esperar(
        60,
        "el torrent en qBittorrent, anunciado a su tracker",
        || async {
            anuncios.load(Ordering::SeqCst) > 0
                && qbt
                    .torrent(&hash)
                    .await
                    .is_some_and(|t| !t["tracker"].as_str().unwrap_or_default().is_empty())
        },
    )
    .await;

    // La portada: su tracker (solo el host) y sus normas.
    let (s, cuerpo) = u.llamar("/api/portada", json!({})).await;
    assert_eq!(s, StatusCode::OK);
    assert!(!cuerpo.contains(PASSKEY), "{cuerpo}");
    let d: DatosPortada = serde_json::from_str(&cuerpo).unwrap();
    let f = d
        .torrents
        .iter()
        .find(|t| t.hash == hash)
        .expect("el torrent añadido en la portada");
    assert_eq!(f.tracker, "127.0.0.1");
    assert_eq!(f.normas, propias);
    assert!(
        eventos(&u)
            .await
            .contains(&(hash.clone(), Evento::Anadido, None)),
        "{:?}",
        eventos(&u).await
    );

    // La tarea horaria también: lo recuerda, y sin el passkey.
    let inf = pasar(&u, ahora()).await;
    assert!(inf.qbt_ok && inf.errores.is_empty(), "{inf:?}");
    let pagina = u.pagina("/").await;
    assert!(!pagina.contains(PASSKEY));
    let reglas_pag = u.pagina("/reglas").await;
    assert!(reglas_pag.contains("127.0.0.1"), "{reglas_pag}");
    assert!(!reglas_pag.contains(PASSKEY));

    qbt.borrar(std::slice::from_ref(&hash)).await;
}
