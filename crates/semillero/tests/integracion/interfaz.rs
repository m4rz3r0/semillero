//! La interfaz: server functions de las páginas (sesión, token, borrado a
//! mano con reautenticación, reglas, «No borrar», modo) y las páginas
//! generadas en el servidor.

use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use qbt::TorrentQbt;
use reglas::{Aviso, ReglasCategoria, TipoRegla};
use semillero::api::ERROR_REAUTH;
use semillero::db::{self, Evento, Modo};
use semillero::mediateca::{Compartida, Mediateca};
use serde_json::{Value, json};

use crate::comun::*;

const DIA: i64 = 86_400;
/// Cumplido: 8 días sembrando y ratio 1,5.
const CUMPLIDO: &str = "1111111111111111111111111111111111111111";
/// Obligado: 2 días sembrando.
pub(crate) const OBLIGADO: &str = "2222222222222222222222222222222222222222";
/// En espera: 10 días y ratio 0,5.
const ESPERA: &str = "3333333333333333333333333333333333333333";
/// Libre: apenas descargado.
const LIBRE: &str = "4444444444444444444444444444444444444444";
/// Otro cumplido.
const CUMPLIDO2: &str = "5555555555555555555555555555555555555555";

/// Los nombres de antes de los grupos: ya no salen en ninguna página.
const NOMBRES_VIEJOS: [&str; 4] = ["Obligados", "En espera", "Cumplidos", "Libres"];

/// Lo que nunca puede salir en una página: nombres y estados de la API de
/// qBittorrent.
const NOMBRES_DE_LA_API: [&str; 12] = [
    "seeding_time",
    "ratio_limit",
    "total_size",
    "save_path",
    "added_on",
    "deleteFiles",
    "stalledUP",
    "stoppedUP",
    "stalledDL",
    "uploading",
    "pausedUP",
    "missingFiles",
];

fn torrent(
    hash: &str,
    nombre: &str,
    estado: &str,
    descargado: i64,
    dias: i64,
    ratio: f64,
) -> Value {
    json!({
        "hash": hash, "name": nombre, "category": "peliculas", "state": estado,
        "downloaded": descargado, "total_size": 1000, "uploaded": 0,
        "seeding_time": dias * DIA, "ratio": ratio, "added_on": 1_700_000_000,
        "save_path": "/media/peliculas", "upspeed": 0, "dlspeed": 0, "progress": 1.0
    })
}

fn torrents() -> Vec<Value> {
    vec![
        torrent(CUMPLIDO, "Película cumplida", "stalledUP", 1000, 8, 1.5),
        torrent(OBLIGADO, "Película obligada", "uploading", 1000, 2, 0.3),
        torrent(ESPERA, "Película en espera", "stoppedUP", 1000, 10, 0.5),
        torrent(LIBRE, "Película libre", "stalledDL", 0, 0, 0.0),
        torrent(CUMPLIDO2, "Otra cumplida", "stalledUP", 1000, 40, 0.2),
    ]
}

pub(crate) struct Ui {
    pub(crate) p: Prueba,
    pub(crate) cookie: String,
    pub(crate) token: String,
}

pub(crate) async fn ui() -> Ui {
    let p = con_sim(qbt_sim::Conf {
        api_abierta: true,
        ..Default::default()
    })
    .await;
    p.sim().control.fijar_torrents(torrents());
    let cookie = sesion(&p).await;
    let token = token(&p, &cookie).await;
    Ui { p, cookie, token }
}

/// `POST` JSON a una server function, con la sesión y el token si se dan.
pub(crate) fn api(
    uri: &str,
    cuerpo: Value,
    cookie: Option<&str>,
    token: Option<&str>,
) -> Request<Body> {
    let mut req = post(uri, &cuerpo.to_string());
    let h = req.headers_mut();
    h.insert("content-type", "application/json".parse().unwrap());
    if let Some(c) = cookie {
        h.insert("cookie", c.parse().unwrap());
    }
    if let Some(t) = token {
        h.insert("x-csrf-token", t.parse().unwrap());
    }
    req
}

impl Ui {
    pub(crate) async fn llamar(&self, uri: &str, cuerpo: Value) -> (StatusCode, String) {
        let r = enviar(
            &self.p.app,
            api(uri, cuerpo, Some(&self.cookie), Some(&self.token)),
        )
        .await;
        (r.status(), texto(r).await)
    }

    pub(crate) async fn pagina(&self, ruta: &str) -> String {
        let r = enviar(&self.p.app, get(ruta, Some(&self.cookie))).await;
        assert_eq!(r.status(), StatusCode::OK, "{ruta}");
        texto(r).await
    }

    fn borrados(&self) -> Vec<String> {
        self.p
            .sim()
            .control
            .llamadas()
            .into_iter()
            .filter(|l| l.ruta == "/api/v2/torrents/delete")
            .map(|l| l.cuerpo)
            .collect()
    }

    fn en_qbt(&self, hash: &str) -> bool {
        self.p
            .sim()
            .control
            .torrents()
            .iter()
            .any(|t| t["hash"] == hash)
    }

    async fn eventos(&self) -> Vec<(String, Evento, Option<TipoRegla>)> {
        db::historial(&self.p.state.db, 100)
            .await
            .unwrap()
            .into_iter()
            .map(|a| (a.hash, a.evento, a.regla))
            .collect()
    }

    /// Cierra la ventana de reautenticación de todas las sesiones, como si
    /// hubieran pasado los 5 minutos.
    async fn pasan_cinco_minutos(&self) {
        sqlx::query("UPDATE sesiones SET reautenticado_hasta = reautenticado_hasta - 301")
            .execute(&self.p.state.db)
            .await
            .unwrap();
    }
}

fn quitar(hash: &str, ficheros: bool, clave: Option<&str>) -> Value {
    json!({ "hash": hash, "ficheros": ficheros, "clave": clave })
}

#[tokio::test]
async fn cada_server_function_exige_sesion_y_token() {
    let u = ui().await;
    let llamadas: [(&str, Value); 10] = [
        ("/api/portada", json!({})),
        ("/api/pausar", json!({ "hash": CUMPLIDO, "parar": true })),
        ("/api/quitar", quitar(CUMPLIDO, true, Some(CLAVE))),
        (
            "/api/no_borrar",
            json!({ "hash": CUMPLIDO, "regla": "Ratio" }),
        ),
        (
            "/api/quitar_exencion",
            json!({ "hash": CUMPLIDO, "regla": "Ratio" }),
        ),
        ("/api/reglas", json!({})),
        (
            "/api/guardar_reglas",
            json!({ "categoria": "peliculas", "reglas": { "ratio": 2.0 } }),
        ),
        ("/api/cambiar_modo", json!({ "real": true, "clave": CLAVE })),
        ("/api/historial", json!({})),
        (
            "/api/guardar_normas",
            json!({ "normas": reglas::NormasTracker { siembra_dias: 1, ..Default::default() } }),
        ),
    ];
    for (uri, cuerpo) in &llamadas {
        let sin_sesion = enviar(&u.p.app, api(uri, cuerpo.clone(), None, None)).await;
        assert_eq!(sin_sesion.status(), StatusCode::UNAUTHORIZED, "{uri}");
        let sin_token = enviar(&u.p.app, api(uri, cuerpo.clone(), Some(&u.cookie), None)).await;
        assert_eq!(sin_token.status(), StatusCode::FORBIDDEN, "{uri}");
        let token_falso = enviar(
            &u.p.app,
            api(uri, cuerpo.clone(), Some(&u.cookie), Some("falso")),
        )
        .await;
        assert_eq!(token_falso.status(), StatusCode::FORBIDDEN, "{uri}");
    }
    // Nada de lo anterior llegó a hacer nada.
    assert!(u.borrados().is_empty());
    assert_eq!(db::modo(&u.p.state.db).await.unwrap(), Modo::SoloAviso);
    assert!(db::reglas(&u.p.state.db).await.unwrap().is_empty());
    assert_eq!(
        db::normas(&u.p.state.db).await.unwrap(),
        reglas::NormasTracker::default()
    );
    // Con sesión y token, las de leer contestan.
    for uri in ["/api/portada", "/api/reglas", "/api/historial"] {
        let (s, _) = u.llamar(uri, json!({})).await;
        assert_eq!(s, StatusCode::OK, "{uri}");
    }
}

#[tokio::test]
async fn borrar_a_mano_rechaza_obligados_y_en_espera() {
    let u = ui().await;
    for hash in [OBLIGADO, ESPERA] {
        for ficheros in [true, false] {
            let (s, cuerpo) = u
                .llamar("/api/quitar", quitar(hash, ficheros, Some(CLAVE)))
                .await;
            assert_eq!(s, StatusCode::CONFLICT, "{hash} {cuerpo}");
            assert!(cuerpo.contains("No se puede quitar"), "{cuerpo}");
            let minusculas = cuerpo.to_lowercase();
            assert!(
                !minusculas.contains("obligad") && !minusculas.contains("en espera"),
                "{cuerpo}"
            );
            assert!(u.en_qbt(hash));
        }
    }
    assert!(u.borrados().is_empty(), "{:?}", u.borrados());
    assert!(u.eventos().await.is_empty());
}

#[tokio::test]
async fn borrar_a_mano_vuelve_a_leer_el_torrent_antes() {
    let u = ui().await;
    // En la lista aparece cumplido, pero al volver a leerlo es obligado.
    u.p.sim().control.cambiar_al_releer(
        CUMPLIDO,
        Some(torrent(
            CUMPLIDO,
            "Película cumplida",
            "stalledUP",
            1000,
            3,
            1.5,
        )),
    );
    let (s, _) = u
        .llamar("/api/quitar", quitar(CUMPLIDO, true, Some(CLAVE)))
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert!(u.borrados().is_empty());
}

/// Mediateca falsa: apunta lo que se le pide olvidar.
#[derive(Default)]
struct Falsa(Mutex<Vec<String>>);

impl Mediateca for Falsa {
    async fn olvidar(&self, t: &TorrentQbt) -> Result<(), String> {
        self.0.lock().unwrap().push(t.hash.clone());
        Ok(())
    }
}

#[tokio::test]
async fn borrar_un_cumplido_con_sus_ficheros() {
    let mut u = ui().await;
    let falsa = Arc::new(Falsa::default());
    u.p.state.mediateca = Compartida(falsa.clone());
    u.p.app = semillero::construir_router(u.p.state.clone());
    // Tenía un aviso pendiente: se va con él.
    db::guardar_avisos(
        &u.p.state.db,
        &[Aviso {
            hash: CUMPLIDO.into(),
            regla: TipoRegla::Ratio,
            creado_en: 0,
            vence_en: DIA,
        }],
    )
    .await
    .unwrap();

    let (s, cuerpo) = u
        .llamar("/api/quitar", quitar(CUMPLIDO, true, Some(CLAVE)))
        .await;
    assert_eq!(s, StatusCode::OK, "{cuerpo}");
    assert_eq!(
        u.borrados(),
        vec![format!("hashes={CUMPLIDO}&deleteFiles=true")]
    );
    assert!(!u.en_qbt(CUMPLIDO));
    assert_eq!(*falsa.0.lock().unwrap(), vec![CUMPLIDO.to_owned()]);
    assert_eq!(
        u.eventos().await,
        vec![(CUMPLIDO.to_owned(), Evento::Borrado, None)]
    );
    assert!(db::avisos(&u.p.state.db).await.unwrap().is_empty());

    // «Quitar» un libre: sin ficheros y sin mediateca.
    let (s, _) = u.llamar("/api/quitar", quitar(LIBRE, false, None)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        u.borrados().last().unwrap(),
        &format!("hashes={LIBRE}&deleteFiles=false")
    );
    assert_eq!(falsa.0.lock().unwrap().len(), 1);
    assert_eq!(
        u.eventos().await[0],
        (LIBRE.to_owned(), Evento::Quitado, None)
    );
}

#[tokio::test]
async fn borrar_a_mano_pide_la_contrasena_pasados_cinco_minutos() {
    let u = ui().await;
    // Sin haberla escrito: hay que escribirla.
    let (s, cuerpo) = u.llamar("/api/quitar", quitar(CUMPLIDO, true, None)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(cuerpo.contains(ERROR_REAUTH), "{cuerpo}");
    // Una mala no vale.
    let (s, cuerpo) = u
        .llamar("/api/quitar", quitar(CUMPLIDO, true, Some("mala")))
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(cuerpo.contains("no es correcta"), "{cuerpo}");
    assert!(u.borrados().is_empty());
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    // La buena borra y abre la ventana: el siguiente no la pide.
    let (s, _) = u
        .llamar("/api/quitar", quitar(CUMPLIDO, true, Some(CLAVE)))
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = u.llamar("/api/quitar", quitar(LIBRE, true, None)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(u.borrados().len(), 2);
    // Pasados 5 minutos, otra vez.
    u.pasan_cinco_minutos().await;
    let (s, cuerpo) = u.llamar("/api/quitar", quitar(CUMPLIDO2, true, None)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(cuerpo.contains(ERROR_REAUTH), "{cuerpo}");
    assert_eq!(u.borrados().len(), 2);
    assert!(u.en_qbt(CUMPLIDO2));
}

#[tokio::test]
async fn pasar_al_borrado_real_pide_la_contrasena() {
    let u = ui().await;
    let modo = || async { db::modo(&u.p.state.db).await.unwrap() };
    let (s, cuerpo) = u
        .llamar("/api/cambiar_modo", json!({ "real": true, "clave": null }))
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(cuerpo.contains(ERROR_REAUTH), "{cuerpo}");
    assert_eq!(modo().await, Modo::SoloAviso);
    let (s, _) = u
        .llamar("/api/cambiar_modo", json!({ "real": true, "clave": CLAVE }))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(modo().await, Modo::BorradoReal);
    // Volver a «Solo aviso» no la pide.
    u.pasan_cinco_minutos().await;
    let (s, _) = u
        .llamar("/api/cambiar_modo", json!({ "real": false, "clave": null }))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(modo().await, Modo::SoloAviso);
    // Y otra vez al real, pasados los 5 minutos, sí.
    let (s, _) = u
        .llamar("/api/cambiar_modo", json!({ "real": true, "clave": null }))
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(modo().await, Modo::SoloAviso);
}

#[tokio::test]
async fn las_reglas_se_guardan_y_se_leen() {
    let u = ui().await;
    let r = ReglasCategoria {
        ratio: Some(2.5),
        siembra_dias: None,
        inactividad_dias: Some(14),
        disco_libre_pct: None,
    };
    let (s, _) = u
        .llamar(
            "/api/guardar_reglas",
            json!({ "categoria": "series", "reglas": r }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(db::reglas(&u.p.state.db).await.unwrap()["series"], r);
    let (s, cuerpo) = u.llamar("/api/reglas", json!({})).await;
    assert_eq!(s, StatusCode::OK);
    let v: Value = serde_json::from_str(&cuerpo).unwrap();
    let series = v["tarjetas"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["categoria"] == "series")
        .unwrap();
    assert_eq!(series["reglas"], serde_json::to_value(r).unwrap());
    // Las tres tarjetas, aunque no tengan nada guardado.
    assert_eq!(v["tarjetas"].as_array().unwrap().len(), 3);
    // Valores fuera de rango o categorías que no existen, no.
    for (cat, reglas) in [
        ("series", json!({ "ratio": 0.0 })),
        ("series", json!({ "siembra_dias": 0 })),
        ("series", json!({ "disco_libre_pct": 95.0 })),
        ("musica", json!({ "ratio": 2.0 })),
    ] {
        let (s, _) = u
            .llamar(
                "/api/guardar_reglas",
                json!({ "categoria": cat, "reglas": reglas }),
            )
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{cat} {reglas}");
    }
    assert_eq!(db::reglas(&u.p.state.db).await.unwrap().len(), 1);
}

/// La etiqueta `<input>` de la página con el identificador `id`.
pub(crate) fn campo<'a>(html: &'a str, id: &str) -> &'a str {
    let p = html
        .find(&format!("id=\"{id}\""))
        .unwrap_or_else(|| panic!("falta el campo {id}"));
    let i = html[..p].rfind("<input").expect("no es un <input>");
    let f = p + html[p..].find('>').unwrap();
    &html[i..f]
}

#[tokio::test]
async fn las_reglas_guardadas_salen_escritas_en_la_pagina() {
    let u = ui().await;
    let r = ReglasCategoria {
        ratio: Some(2.5),
        siembra_dias: None,
        inactividad_dias: Some(21),
        disco_libre_pct: None,
    };
    let (s, _) = u
        .llamar(
            "/api/guardar_reglas",
            json!({ "categoria": "series", "reglas": r }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let html = u.pagina("/reglas").await;
    // Lo guardado, en el HTML del servidor (sin esperar al navegador).
    let ratio = campo(&html, "reglas-series-ratio-valor");
    assert!(ratio.contains("value=\"2.5\""), "{ratio}");
    let inactividad = campo(&html, "reglas-series-inactividad-valor");
    assert!(inactividad.contains("value=\"21\""), "{inactividad}");
    // Lo apagado, con el valor que se propone al encenderlo.
    let siembra = campo(&html, "reglas-series-siembra-valor");
    assert!(siembra.contains("value=\"30\""), "{siembra}");
    let peliculas = campo(&html, "reglas-peliculas-ratio-valor");
    assert!(peliculas.contains("value=\"2.0\""), "{peliculas}");
}

#[tokio::test]
async fn no_borrar_y_deshacerlo_desde_la_interfaz() {
    let u = ui().await;
    db::guardar_avisos(
        &u.p.state.db,
        &[Aviso {
            hash: CUMPLIDO.into(),
            regla: TipoRegla::Ratio,
            creado_en: 0,
            vence_en: DIA,
        }],
    )
    .await
    .unwrap();
    let (s, _) = u
        .llamar(
            "/api/no_borrar",
            json!({ "hash": CUMPLIDO, "regla": "Ratio" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert!(db::avisos(&u.p.state.db).await.unwrap().is_empty());
    assert!(
        db::exenciones(&u.p.state.db)
            .await
            .unwrap()
            .contains(&(CUMPLIDO.to_owned(), TipoRegla::Ratio))
    );
    let pagina = u.pagina("/reglas").await;
    assert!(pagina.contains("Con «No recoger»"), "{pagina}");
    let (s, _) = u
        .llamar(
            "/api/quitar_exencion",
            json!({ "hash": CUMPLIDO, "regla": "Ratio" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert!(db::exenciones(&u.p.state.db).await.unwrap().is_empty());
    assert_eq!(
        u.eventos().await,
        vec![
            (
                CUMPLIDO.to_owned(),
                Evento::ExencionQuitada,
                Some(TipoRegla::Ratio)
            ),
            (
                CUMPLIDO.to_owned(),
                Evento::Cancelado,
                Some(TipoRegla::Ratio)
            ),
        ]
    );
    // Un hash que no es un hash, no.
    let (s, _) = u
        .llamar(
            "/api/no_borrar",
            json!({ "hash": "nada", "regla": "Ratio" }),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn parar_y_reanudar() {
    let u = ui().await;
    let (s, _) = u
        .llamar("/api/pausar", json!({ "hash": OBLIGADO, "parar": true }))
        .await;
    assert_eq!(s, StatusCode::OK);
    let estado = |h: &str| {
        u.p.sim()
            .control
            .torrents()
            .into_iter()
            .find(|t| t["hash"] == h)
            .unwrap()["state"]
            .clone()
    };
    assert_eq!(estado(OBLIGADO), "stoppedUP");
    let (s, _) = u
        .llamar("/api/pausar", json!({ "hash": OBLIGADO, "parar": false }))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(estado(OBLIGADO), "uploading");
}

fn sin_nombres_de_la_api(ruta: &str, html: &str) {
    for n in NOMBRES_DE_LA_API {
        assert!(!html.contains(n), "{ruta} contiene {n:?}");
    }
}

#[tokio::test]
async fn las_paginas_hablan_en_palabras() {
    let u = ui().await;
    db::guardar_avisos(
        &u.p.state.db,
        &[Aviso {
            hash: CUMPLIDO.into(),
            regla: TipoRegla::Siembra,
            creado_en: 0,
            vence_en: i64::MAX / 2,
        }],
    )
    .await
    .unwrap();
    semillero::tarea::cambiar_modo(&u.p.state.db, Modo::BorradoReal, 1_800_000_000)
        .await
        .unwrap();

    let portada = u.jardin_entero().await;
    for texto in [
        "Recién sembrados",
        "Creciendo",
        "Listos para recoger",
        "Sin arraigar",
        "Jardín",
        "No recoger",
        "regla: tiempo sembrando",
        "Película cumplida",
        ">Recoger<",
        "En pausa",
        "Ratio 1,50",
    ] {
        assert!(portada.contains(texto), "falta {texto:?} en la portada");
    }
    sin_nombres_de_la_api("/", &portada);

    let reglas = u.pagina("/reglas").await;
    for texto in [
        "Películas",
        "Series",
        "Sin categoría",
        "Solo aviso",
        "Borrado real",
        "Espacio en disco",
    ] {
        assert!(reglas.contains(texto), "falta {texto:?} en las reglas");
    }
    sin_nombres_de_la_api("/reglas", &reglas);

    let historial = u.pagina("/historial").await;
    assert!(historial.contains("Ajuste general"), "{historial}");
    assert!(historial.contains("Cambio de modo"), "{historial}");
    sin_nombres_de_la_api("/historial", &historial);

    // La API de la portada tampoco los manda.
    let (_, json) = u.llamar("/api/portada", json!({})).await;
    sin_nombres_de_la_api("/api/portada", &json);
}

#[tokio::test]
async fn solo_obligados_sin_boton_de_borrar() {
    let u = ui().await;
    u.p.sim().control.fijar_torrents(vec![torrent(
        OBLIGADO,
        "Película obligada",
        "uploading",
        1000,
        2,
        0.3,
    )]);
    let portada = u.pagina("/").await;
    assert!(!portada.contains(">Recoger<"));
    assert!(portada.contains("Le faltan 5 días"), "{portada}");
}

/// El trozo de la página de la maceta de `hash` (su `<li>`).
fn maceta<'a>(html: &'a str, hash: &str) -> &'a str {
    let id = format!("id=\"t-{hash}\"");
    let pos = html
        .find(&id)
        .unwrap_or_else(|| panic!("falta la maceta de {hash}"));
    let i = html[..pos].rfind("<li").expect("la maceta no es un <li>");
    let f = pos + html[pos..].find("</li>").expect("maceta sin cerrar") + "</li>".len();
    &html[i..f]
}

/// El dibujo (`<svg>`) de una maceta.
fn dibujo<'a>(html: &'a str, hash: &str) -> &'a str {
    let m = maceta(html, hash);
    let i = m.find("<svg").expect("la maceta no tiene dibujo");
    let f = i + m[i..].find("</svg>").expect("dibujo sin cerrar") + "</svg>".len();
    &m[i..f]
}

/// Los bancales del jardín, en el orden de las pestañas: el valor de
/// `?bancal=` y su nombre.
const BANCALES: [(&str, &str); 4] = [
    ("listos", "Listos para recoger"),
    ("creciendo", "Creciendo"),
    ("recien", "Recién sembrados"),
    ("sin-arraigar", "Sin arraigar"),
];

impl Ui {
    /// Las cuatro pantallas del jardín (una por bancal), seguidas.
    async fn jardin_entero(&self) -> String {
        let mut todo = String::new();
        for (bancal, _) in BANCALES {
            todo.push_str(&self.pagina(&format!("/?bancal={bancal}")).await);
        }
        todo
    }
}

/// Las pestañas de los bancales (cada `<a role="tab">…</a>`), en orden.
fn pestanas(html: &str) -> Vec<&str> {
    html.match_indices("role=\"tab\"")
        .map(|(p, _)| {
            let i = html[..p].rfind("<a").expect("la pestaña no es un <a>");
            assert!(!html[i..p].contains('>'), "la pestaña no es un <a>");
            let f = p + html[p..].find("</a>").expect("pestaña sin cerrar") + "</a>".len();
            &html[i..f]
        })
        .collect()
}

/// El bancal que se ve (`role="tabpanel"`).
fn panel(html: &str) -> &str {
    let p = html.find("role=\"tabpanel\"").expect("falta el bancal");
    let i = html[..p]
        .rfind("<section")
        .expect("el bancal no es un <section>");
    let f = p + html[p..].find("</section>").expect("bancal sin cerrar") + "</section>".len();
    &html[i..f]
}

/// El valor de `?bancal=` de la pestaña elegida.
fn elegido(html: &str) -> &'static str {
    let elegidas: Vec<&'static str> = pestanas(html)
        .iter()
        .zip(BANCALES)
        .filter(|(p, _)| p.contains("aria-selected=\"true\""))
        .map(|(_, (bancal, _))| bancal)
        .collect();
    assert_eq!(elegidas.len(), 1, "{elegidas:?}");
    elegidas[0]
}

/// Las macetas de una página.
fn macetas_en(html: &str) -> usize {
    html.matches("<li id=\"t-").count()
}

#[tokio::test]
async fn el_jardin_ensena_un_bancal_y_cuatro_pestanas() {
    let u = ui().await;
    let html = u.pagina("/").await;
    assert_eq!(html.matches("role=\"tablist\"").count(), 1);
    assert_eq!(html.matches("role=\"tabpanel\"").count(), 1);
    let p = pestanas(&html);
    assert_eq!(p.len(), 4, "{p:?}");
    // En orden, cada una un enlace a su bancal, con su nombre y su cuenta.
    for ((pestana, (bancal, nombre)), cuenta) in p.iter().zip(BANCALES).zip([2, 1, 1, 1]) {
        assert!(
            pestana.contains(&format!("href=\"?bancal={bancal}\"")),
            "{pestana}"
        );
        assert!(pestana.contains(nombre), "{pestana}");
        assert!(pestana.contains(&format!(">{cuenta}<")), "{pestana}");
        assert!(
            pestana.contains("aria-controls=\"bancal-elegido\""),
            "{pestana}"
        );
        assert!(!pestana.contains("style="), "{pestana}");
    }
    // Sin elegir, el primero con macetas: «Listos para recoger».
    assert_eq!(elegido(&html), "listos");
    assert!(p[0].contains("tabindex=\"0\""), "{}", p[0]);
    for otra in &p[1..] {
        assert!(otra.contains("aria-selected=\"false\""), "{otra}");
        assert!(otra.contains("tabindex=\"-1\""), "{otra}");
    }
    let b = panel(&html);
    assert!(b.contains("id=\"bancal-elegido\""), "{b}");
    assert!(b.contains("aria-labelledby=\"pestana-listos\""), "{b}");
    // Solo sus macetas.
    assert_eq!(macetas_en(&html), 2);
    for hash in [CUMPLIDO, CUMPLIDO2] {
        assert!(b.contains(&format!("id=\"t-{hash}\"")), "{hash}");
    }
}

#[tokio::test]
async fn cada_pestana_lleva_a_su_bancal() {
    let u = ui().await;
    let html = u.pagina("/?bancal=recien").await;
    assert_eq!(elegido(&html), "recien");
    let b = panel(&html);
    assert!(b.contains("aria-labelledby=\"pestana-recien\""), "{b}");
    assert!(b.contains(&format!("id=\"t-{OBLIGADO}\"")), "{b}");
    assert!(!html.contains(&format!("id=\"t-{CUMPLIDO}\"")));
    assert_eq!(macetas_en(&html), 1);
    // Un bancal que no existe: el de siempre.
    let html = u.pagina("/?bancal=huerto").await;
    assert_eq!(elegido(&html), "listos");
    assert!(html.contains(&format!("id=\"t-{CUMPLIDO}\"")));
    // Cada bancal con lo suyo.
    for (bancal, hashes) in [("creciendo", &[ESPERA][..]), ("sin-arraigar", &[LIBRE][..])] {
        let html = u.pagina(&format!("/?bancal={bancal}")).await;
        assert_eq!(elegido(&html), bancal);
        assert_eq!(macetas_en(&html), hashes.len(), "{bancal}");
        for h in hashes {
            assert!(panel(&html).contains(&format!("id=\"t-{h}\"")), "{bancal}");
        }
    }
}

#[tokio::test]
async fn un_bancal_vacio_lo_dice_sin_macetas() {
    let u = ui().await;
    u.p.sim().control.fijar_torrents(vec![torrent(
        OBLIGADO,
        "Película obligada",
        "uploading",
        1000,
        2,
        0.3,
    )]);
    // Sin elegir, el primero con macetas.
    let html = u.pagina("/").await;
    assert_eq!(elegido(&html), "recien");
    assert_eq!(macetas_en(&html), 1);
    assert!(!html.contains("Ninguna maceta en este bancal."));
    // Uno vacío lo dice en una frase.
    let html = u.pagina("/?bancal=listos").await;
    assert_eq!(elegido(&html), "listos");
    assert_eq!(macetas_en(&html), 0);
    assert!(panel(&html).contains("Ninguna maceta en este bancal."));
    assert!(!html.contains(">Recoger<"));
}

#[tokio::test]
async fn el_tiempo_del_jardin_va_en_la_cabecera() {
    let u = ui().await;
    let html = u.pagina("/").await;
    let i = html.find("<header").expect("falta la cabecera");
    let f = i + html[i..].find("</header>").unwrap();
    let cabecera = &html[i..f];
    assert!(cabecera.contains("class=\"tiempo-letrero\""), "{cabecera}");
    // Sin el fichero de estado no se sabe nada: tormenta, con su causa.
    assert!(cabecera.contains("La VPN no responde."), "{cabecera}");
    assert!(!panel(&html).contains("tiempo-letrero"));
    // Un solo tiempo en la página.
    assert_eq!(html.matches("class=\"tiempo-letrero\"").count(), 1);
}

#[tokio::test]
async fn el_jardin_tiene_un_bancal_por_estado_y_una_maceta_por_torrent() {
    let u = ui().await;
    // Una maceta (dibujo y etiqueta de herbolario) por torrent, en su bancal
    // y con su estado en la clase del dibujo.
    for (hash, bancal, clase) in [
        (CUMPLIDO, "listos", "planta-recoger"),
        (CUMPLIDO2, "listos", "planta-recoger"),
        (ESPERA, "creciendo", "planta-creciendo"),
        (OBLIGADO, "recien", "planta-sembrado"),
        (LIBRE, "sin-arraigar", "planta-arraigar"),
    ] {
        let html = u.pagina(&format!("/?bancal={bancal}")).await;
        let m = maceta(panel(&html), hash);
        assert_eq!(m.matches("<svg").count(), 1, "{m}");
        assert_eq!(m.matches("class=\"etiqueta").count(), 1, "{m}");
        assert!(dibujo(&html, hash).contains(clase), "{hash}: {m}");
        // Se llega con el teclado y abre su ficha.
        assert!(m.contains("aria-haspopup=\"dialog\""), "{m}");
        // Sin estilos en línea: la CSP no los deja.
        assert!(!m.contains("style="), "{m}");
    }
    let html = u.jardin_entero().await;
    assert_eq!(macetas_en(&html), 5);

    // «Recoger» en las etiquetas de los listos y de los sin arraigar; en
    // las demás, no.
    for hash in [CUMPLIDO, CUMPLIDO2, LIBRE] {
        assert!(maceta(&html, hash).contains(">Recoger<"), "{hash}");
    }
    for hash in [OBLIGADO, ESPERA] {
        assert!(!maceta(&html, hash).contains(">Recoger<"), "{hash}");
    }
    // Lo que dice cada etiqueta.
    assert!(maceta(&html, CUMPLIDO).contains("Lista para recoger"));
    assert!(maceta(&html, CUMPLIDO).contains("Ratio 1,50"));
    assert!(maceta(&html, OBLIGADO).contains("Le faltan 5 días"));
    assert!(
        maceta(&html, ESPERA).contains("Le falta ratio 1,0 (o faltan 20 días)"),
        "{}",
        maceta(&html, ESPERA)
    );
    assert!(maceta(&html, LIBRE).contains("No obliga a nada"));
    // El que está parado lo dice en su etiqueta; los demás, no.
    assert!(maceta(&html, ESPERA).contains("En pausa"));
    for hash in [CUMPLIDO, CUMPLIDO2, OBLIGADO, LIBRE] {
        assert!(!maceta(&html, hash).contains("En pausa"), "{hash}");
    }
    // Sin avisos, nada se recoge solo.
    assert!(!html.contains("Se recoge sola en"));
    assert!(!html.contains("No recoger"));
}

#[tokio::test]
async fn una_maceta_con_aviso_dice_cuando_se_recoge_sola() {
    let u = ui().await;
    let ahora = semillero::tarea::ahora_unix();
    db::guardar_avisos(
        &u.p.state.db,
        &[Aviso {
            hash: CUMPLIDO.into(),
            regla: TipoRegla::Ratio,
            creado_en: ahora,
            vence_en: ahora + 18 * 3600 + 600,
        }],
    )
    .await
    .unwrap();
    semillero::tarea::cambiar_modo(&u.p.state.db, Modo::BorradoReal, ahora)
        .await
        .unwrap();
    // VPN y puerto bien: la cuenta atrás avanza.
    std::fs::write(
        u.p.state.cfg.estado_red.as_ref().unwrap(),
        format!("escrito_en={ahora}\nhandshake={ahora}\npuerto=51413\nfallos=0\n"),
    )
    .unwrap();
    let html = u.pagina("/").await;
    let m = maceta(&html, CUMPLIDO);
    assert!(m.contains("class=\"etiqueta etiqueta-aviso"), "{m}");
    assert!(m.contains("Se recoge sola en 18 h (regla: ratio)"), "{m}");
    assert!(m.contains(">No recoger<"), "{m}");
    // La frase va en la cabecera y lleva a su bancal y a su maceta.
    let i = html.find("<header").unwrap();
    let cabecera = &html[i..i + html[i..].find("</header>").unwrap()];
    assert!(cabecera.contains("1 maceta se recogerá sola en menos de 24 h."));
    assert!(cabecera.contains(&format!("href=\"?bancal=listos#t-{CUMPLIDO}\"")));
    // Las demás, no.
    let html = u.jardin_entero().await;
    for hash in [CUMPLIDO2, OBLIGADO, ESPERA, LIBRE] {
        let m = maceta(&html, hash);
        assert!(!m.contains("Se recoge sola"), "{hash}");
        assert!(!m.contains("No recoger"), "{hash}");
    }
}

#[tokio::test]
async fn recoger_borra_con_los_ficheros_y_pide_la_contrasena() {
    use semillero::ui::Delicada;
    use semillero::ui::portada::{peticion_quitar_sin_ficheros, peticion_recoger};

    let u = ui().await;
    // Lo que manda el botón «Recoger»: borrar con los ficheros.
    let p = peticion_recoger(CUMPLIDO, "Película cumplida");
    assert_eq!(p.boton, "Recoger");
    assert!(
        p.texto.contains("ficheros") && p.texto.contains("Jellyfin"),
        "{}",
        p.texto
    );
    let Delicada::Quitar { hash, ficheros } = p.que else {
        panic!("«Recoger» no quita el torrent");
    };
    assert_eq!((hash.as_str(), ficheros), (CUMPLIDO, true));
    // Con la ventana de 5 minutos cerrada, pide la contraseña.
    let (s, cuerpo) = u.llamar("/api/quitar", quitar(&hash, ficheros, None)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(cuerpo.contains(ERROR_REAUTH), "{cuerpo}");
    assert!(u.borrados().is_empty());
    let (s, _) = u
        .llamar("/api/quitar", quitar(&hash, ficheros, Some(CLAVE)))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        u.borrados(),
        vec![format!("hashes={CUMPLIDO}&deleteFiles=true")]
    );

    // «Quitar sin borrar los ficheros»: los deja en el disco.
    let p = peticion_quitar_sin_ficheros(LIBRE, "Película libre");
    let Delicada::Quitar { hash, ficheros } = p.que else {
        panic!("«Quitar sin borrar los ficheros» no quita el torrent");
    };
    assert_eq!((hash.as_str(), ficheros), (LIBRE, false));
    u.pasan_cinco_minutos().await;
    let (s, cuerpo) = u.llamar("/api/quitar", quitar(&hash, ficheros, None)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert!(cuerpo.contains(ERROR_REAUTH), "{cuerpo}");
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let (s, _) = u
        .llamar("/api/quitar", quitar(&hash, ficheros, Some(CLAVE)))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        u.borrados().last().unwrap(),
        &format!("hashes={LIBRE}&deleteFiles=false")
    );
}

#[tokio::test]
async fn las_macetas_se_dibujan_igual_con_los_mismos_datos() {
    let u = ui().await;
    let hashes = [CUMPLIDO, OBLIGADO, ESPERA, LIBRE, CUMPLIDO2];
    let primero = u.jardin_entero().await;
    let segundo = u.jardin_entero().await;
    for h in hashes {
        assert_eq!(dibujo(&primero, h), dibujo(&segundo, h), "{h}");
    }
    // Ni siquiera depende del orden en que qBittorrent los da.
    let mut al_reves = torrents();
    al_reves.reverse();
    u.p.sim().control.fijar_torrents(al_reves);
    let tercero = u.jardin_entero().await;
    for h in hashes {
        assert_eq!(dibujo(&primero, h), dibujo(&tercero, h), "{h}");
    }
    // Dos listos para recoger no son la misma planta clonada.
    assert_ne!(
        dibujo(&primero, CUMPLIDO).replace(CUMPLIDO, ""),
        dibujo(&primero, CUMPLIDO2).replace(CUMPLIDO2, "")
    );
}

#[test]
fn el_dibujo_sale_de_los_datos() {
    use semillero::api::{Actividad, FichaTorrent};
    use semillero::ui::maceta::trazos;

    let base = FichaTorrent {
        hash: CUMPLIDO.into(),
        nombre: "Una".into(),
        categoria: "Películas".into(),
        estado: reglas::Estado::EnEspera,
        actividad: Actividad::Sembrando,
        ratio: 0.2,
        siembra_s: 10 * DIA,
        falta_s: 20 * DIA,
        tamano: 1000,
        aviso: None,
        exenciones: Vec::new(),
    };
    assert_eq!(
        trazos(&base, &reglas::NormasTracker::default()),
        trazos(&base.clone(), &reglas::NormasTracker::default())
    );
    // Otro torrent en el mismo punto: hojas algo distintas.
    let otro = FichaTorrent {
        hash: CUMPLIDO2.into(),
        ..base.clone()
    };
    assert_ne!(
        trazos(&base, &reglas::NormasTracker::default()),
        trazos(&otro, &reglas::NormasTracker::default())
    );
    // Más ratio, más planta.
    let crecido = FichaTorrent {
        ratio: 0.9,
        ..base.clone()
    };
    assert!(
        trazos(&crecido, &reglas::NormasTracker::default()).len()
            > trazos(&base, &reglas::NormasTracker::default()).len()
    );
    // Sin números raros en el dibujo.
    let raro = FichaTorrent {
        ratio: f64::NAN,
        siembra_s: -5,
        ..base.clone()
    };
    for t in trazos(&raro, &reglas::NormasTracker::default()) {
        assert!(!t.d.contains("NaN") && !t.d.contains("inf"), "{}", t.d);
    }
}

#[tokio::test]
async fn ninguna_pagina_usa_los_nombres_viejos() {
    let u = ui().await;
    for ruta in ["/", "/reglas", "/historial"] {
        let html = u.pagina(ruta).await;
        for n in NOMBRES_VIEJOS {
            assert!(!html.contains(n), "{ruta} contiene {n:?}");
        }
        // Ni el mapa del semillero, que ya no está.
        assert!(!html.contains("class=\"mapa"), "{ruta} tiene el mapa");
    }
    let login = texto(enviar(&u.p.app, get("/login", None)).await).await;
    for n in NOMBRES_VIEJOS {
        assert!(!login.contains(n), "/login contiene {n:?}");
    }
}

/// La cabecera de una página, de `<header` a `</header>`.
fn cabecera(html: &str) -> &str {
    let i = html.find("<header").expect("falta la cabecera");
    &html[i..i + html[i..].find("</header>").expect("cabecera sin cerrar")]
}

/// La primera fila de la cabecera (marca, secciones y botones), sin lo que
/// cambia de una página a otra (la sección en la que se está).
fn fila_de_cabecera(html: &str) -> String {
    let c = cabecera(html);
    let i = c
        .find("<div class=\"cabecera-fila\">")
        .expect("falta la fila");
    let f = c[i..].find("</nav>").map_or(c.len(), |f| i + f);
    c[i..f]
        .replace(" aria-current=\"page\"", "")
        .replace(" class=\"active\"", "")
}

#[tokio::test]
async fn la_misma_cabecera_en_todas_las_paginas() {
    let u = ui().await;
    let jardin = u.pagina("/").await;
    let reglas = u.pagina("/reglas").await;
    let historial = u.pagina("/historial").await;
    let login = texto(enviar(&u.p.app, get("/login", None)).await).await;
    // El mismo contenedor y la misma fila en todas, también en el acceso.
    for (ruta, html) in [
        ("/", &jardin),
        ("/reglas", &reglas),
        ("/historial", &historial),
        ("/login", &login),
    ] {
        let c = cabecera(html);
        assert!(
            c.starts_with("<header class=\"cabecera\"><div class=\"cabecera-fila\">"),
            "{ruta}: {c}"
        );
        assert!(c.contains("class=\"marca\""), "{ruta}: {c}");
        // Y la misma escena debajo: el seto y el suelo del jardín.
        assert!(html.contains("class=\"seto\""), "{ruta} sin seto");
        assert!(!c.contains("style="), "{ruta}: {c}");
    }
    let fila = fila_de_cabecera(&jardin);
    assert_eq!(fila, fila_de_cabecera(&reglas));
    assert_eq!(fila, fila_de_cabecera(&historial));
    // Con sesión: las tres secciones y, en una sola fila, el tema y
    // «Salir» como botones con su nombre.
    let c = cabecera(&jardin);
    for texto in [">Jardín<", ">Reglas<", ">Historial<"] {
        assert!(c.contains(texto), "falta {texto}: {c}");
    }
    assert!(c.contains("aria-label=\"Salir\""), "{c}");
    assert!(c.contains("aria-label=\"Cambiar el tema"), "{c}");
    // En el acceso no hay secciones ni «Salir».
    let c = cabecera(&login);
    assert!(!c.contains("<nav"), "{c}");
    assert!(!c.contains("Salir"), "{c}");
}

#[tokio::test]
async fn las_macetas_van_en_un_bancal_de_madera_con_su_letrero() {
    let u = ui().await;
    let html = u.pagina("/?bancal=listos").await;
    let b = panel(&html);
    // La explicación del bancal, en su letrero de madera.
    let i = b
        .find("class=\"letrero-bancal\"")
        .expect("falta el letrero del bancal");
    let letrero = &b[i..i + b[i..].find("</p>").unwrap()];
    assert!(letrero.contains("Ya se pueden recoger"), "{letrero}");
    // Las macetas, dentro del bancal de madera.
    let i = b.find("class=\"cama").expect("falta el bancal de madera");
    assert!(b[i..].contains(&format!("id=\"t-{CUMPLIDO}\"")), "{b}");
    // Cada maceta, con su etiqueta clavada en una estaca: nombre y estado
    // a la vista; los datos, para los lectores de pantalla (y en la ficha).
    let m = maceta(b, CUMPLIDO);
    assert!(m.contains("class=\"estaca\""), "{m}");
    assert!(m.contains("Película cumplida"), "{m}");
    assert!(
        m.contains("class=\"estado-planta\">Lista para recoger<"),
        "{m}"
    );
    assert!(m.contains("class=\"solo-lectores\""), "{m}");
    // Creciendo: una línea corta en la etiqueta.
    let html = u.pagina("/?bancal=creciendo").await;
    let m = maceta(&html, ESPERA);
    assert!(m.contains("Le falta ratio 1,0 (o faltan 20 días)"), "{m}");
}

#[tokio::test]
async fn las_reglas_en_el_jardin() {
    let u = ui().await;
    let html = u.pagina("/reglas").await;
    // «Solo aviso» y «Borrado real», dos letreros; el elegido, pulsado.
    let i = html
        .find("class=\"letreros-modo\"")
        .expect("faltan los letreros");
    let modo = &html[i..i + html[i..].find("</section>").unwrap()];
    assert_eq!(modo.matches("<button").count(), 2, "{modo}");
    let solo = &modo[modo.find("Solo aviso").unwrap().saturating_sub(300)..];
    assert!(modo.contains("aria-pressed=\"true\""), "{modo}");
    assert!(solo.contains("aria-pressed=\"true\""), "{solo}");
    // Un apartado por categoría, cada uno en su tablilla, con su tiesto y
    // sus cuatro reglas: interruptor y cifra, cada uno con su etiqueta.
    for base in ["reglas-peliculas", "reglas-series", "reglas-sin"] {
        let i = html
            .find(&format!("aria-labelledby=\"{base}-titulo\""))
            .unwrap_or_else(|| panic!("falta {base}"));
        let ini = html[..i].rfind("<section").unwrap();
        let fin = i + html[i..].find('>').unwrap();
        assert!(html[ini..fin].contains("tablilla"), "{base}");
        for r in ["ratio", "siembra", "inactividad", "disco"] {
            for id in [format!("{base}-{r}"), format!("{base}-{r}-valor")] {
                campo(&html, &id);
                assert!(
                    html.contains(&format!("for=\"{id}\"")),
                    "sin etiqueta: {id}"
                );
            }
        }
    }
    assert!(!html.contains("style="));
}

#[tokio::test]
async fn el_historial_es_un_cuaderno() {
    let u = ui().await;
    // Sin nada todavía: una frase y un dibujo, no una página en blanco.
    let html = u.pagina("/historial").await;
    let i = html.find("class=\"cuaderno").expect("falta el cuaderno");
    let cuaderno = &html[i..];
    assert!(cuaderno.contains("Aún no ha pasado nada."), "{cuaderno}");
    assert!(cuaderno.contains("<svg"), "{cuaderno}");
    // Con algo apuntado: la fecha y lo que pasó.
    semillero::tarea::cambiar_modo(&u.p.state.db, Modo::BorradoReal, 1_800_000_000)
        .await
        .unwrap();
    let html = u.pagina("/historial").await;
    let i = html.find("class=\"cuaderno").expect("falta el cuaderno");
    let cuaderno = &html[i..];
    let i = cuaderno.find("<time").expect("falta la fecha");
    let fecha = &cuaderno[i..i + cuaderno[i..].find('>').unwrap()];
    assert!(fecha.contains("class=\"linea-cuando\""), "{fecha}");
    assert!(
        fecha.contains("datetime=\"2027-01-15T08:00:00Z\""),
        "{fecha}"
    );
    assert!(cuaderno.contains("Cambio de modo"), "{cuaderno}");
    assert!(!cuaderno.contains("Aún no ha pasado nada."));
}

/// Escribe el fichero de estado de la red: la VPN (saludo reciente o de hace una
/// hora) y el puerto (abierto o con tres fallos seguidos).
fn fijar_red(u: &Ui, vpn: bool, puerto: bool) {
    let ahora = semillero::tarea::ahora_unix();
    let saludo = if vpn { ahora } else { ahora - 3600 };
    let fallos = if puerto { 0 } else { 3 };
    std::fs::write(
        u.p.state.cfg.estado_red.as_ref().unwrap(),
        format!("escrito_en={ahora}\nhandshake={saludo}\npuerto=51413\nfallos={fallos}\n"),
    )
    .unwrap();
}

/// El letrero del tiempo (lo que se lee): de `class="tiempo-letrero"` a su
/// `</div>`.
fn letrero_del_tiempo(html: &str) -> &str {
    let c = cabecera(html);
    let i = c
        .find("class=\"tiempo-letrero\"")
        .unwrap_or_else(|| panic!("falta el letrero del tiempo: {c}"));
    &c[i..i + c[i..].find("</div>").expect("letrero sin cerrar")]
}

#[tokio::test]
async fn el_tiempo_del_jardin_con_vpn_y_puerto_bien_es_buen_tiempo() {
    let u = ui().await;
    fijar_red(&u, true, true);
    let html = u.pagina("/").await;
    let c = cabecera(&html);
    // El dibujo del cielo es decoración; el letrero es lo que se anuncia.
    assert!(c.contains("class=\"tiempo tiempo-bueno\""), "{c}");
    assert!(c.contains("role=\"status\""), "{c}");
    let l = letrero_del_tiempo(&html);
    assert!(
        l.contains("Buen tiempo: el jardín riega y comparte."),
        "{l}"
    );
    // Sin palabras técnicas cuando todo va bien.
    for viejo in [
        "VPN conectada",
        "puerto abierto",
        "VPN",
        "puerto",
        "congelada",
    ] {
        assert!(!c.contains(viejo), "{viejo:?} en {c}");
    }
    assert!(!c.contains("Nublado") && !c.contains("Tormenta"), "{c}");
}

#[tokio::test]
async fn el_tiempo_del_jardin_con_el_puerto_cerrado_esta_nublado() {
    let u = ui().await;
    fijar_red(&u, true, false);
    let html = u.pagina("/").await;
    assert!(cabecera(&html).contains("class=\"tiempo tiempo-nublado\""));
    let l = letrero_del_tiempo(&html);
    assert!(
        l.contains("Nublado: llega poca gente a por las semillas."),
        "{l}"
    );
    // La causa, en palabras llanas, y que las cuentas atrás están paradas.
    assert!(l.contains("El puerto de la VPN está cerrado."), "{l}");
    assert!(
        l.contains("Las cuentas atrás están paradas y no se recoge nada automáticamente."),
        "{l}"
    );
    assert!(!l.contains("Buen tiempo") && !l.contains("Tormenta"), "{l}");
}

#[tokio::test]
async fn el_tiempo_del_jardin_con_la_vpn_caida_es_tormenta() {
    let u = ui().await;
    fijar_red(&u, false, true);
    let html = u.pagina("/").await;
    assert!(cabecera(&html).contains("class=\"tiempo tiempo-tormenta\""));
    let l = letrero_del_tiempo(&html);
    assert!(l.contains("Tormenta: el jardín está cerrado."), "{l}");
    assert!(
        l.contains(
            "La VPN no responde. Las cuentas atrás están paradas y no se recoge nada \
             automáticamente."
        ),
        "{l}"
    );
    assert!(!l.contains("Buen tiempo") && !l.contains("Nublado"), "{l}");
}

#[tokio::test]
async fn el_tiempo_del_jardin_avisa_de_la_tierra_seca() {
    let u = ui().await;
    fijar_red(&u, true, true);
    // Un disco que se mide de verdad («/») y un mínimo que no alcanza.
    let mut t = torrent(CUMPLIDO, "Película cumplida", "stalledUP", 1000, 8, 1.5);
    t["save_path"] = json!("/");
    u.p.sim().control.fijar_torrents(vec![t]);
    db::guardar_reglas(
        &u.p.state.db,
        "peliculas",
        &ReglasCategoria {
            disco_libre_pct: Some(100.0),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let html = u.pagina("/").await;
    let l = letrero_del_tiempo(&html);
    assert!(l.contains("Buen tiempo"), "{l}");
    assert!(
        l.contains("Tierra seca: queda poco sitio en Películas ("),
        "{l}"
    );
    assert!(l.contains(" % libre)."), "{l}");
}

#[tokio::test]
async fn la_ficha_es_un_pliego_de_herbario_con_cada_dato_una_vez() {
    let u = ui().await;
    let (s, j) = u.llamar("/api/portada", json!({})).await;
    assert_eq!(s, StatusCode::OK, "{j}");
    let datos: semillero::api::DatosPortada = serde_json::from_str(&j).unwrap();
    let f = semillero::ui::portada::ficha_en_html(&datos, CUMPLIDO).expect("falta la ficha");
    // El pliego: la planta montada y su etiqueta de herbario.
    for parte in [
        "class=\"pliego\"",
        "class=\"pliego-planta\"",
        "class=\"pliego-etiqueta\"",
        "id=\"ficha-titulo\"",
        "Película cumplida",
    ] {
        assert!(f.contains(parte), "falta {parte:?}: {f}");
    }
    // Cada dato una vez: «Lista para recoger» no se repite (ni con el
    // nombre del bancal).
    assert_eq!(f.matches("Lista para recoger").count(), 1, "{f}");
    assert!(!f.contains("Listo para recoger"), "{f}");
    assert_eq!(f.matches("Ratio").count(), 1, "{f}");
    // Las acciones, y el botón para cerrarla.
    for boton in [
        ">Parar<",
        ">Recoger<",
        ">Quitar sin borrar los ficheros<",
        ">Cerrar<",
    ] {
        assert_eq!(f.matches(boton).count(), 1, "{boton}: {f}");
    }
    assert!(!f.contains("style="), "{f}");
    // Una maceta que no está no tiene ficha.
    assert!(semillero::ui::portada::ficha_en_html(&datos, "ab").is_none());
}

#[tokio::test]
async fn las_normas_del_tracker_salen_en_las_reglas_y_se_guardan() {
    let u = ui().await;
    // Por defecto, las de siempre, escritas en la página.
    let html = u.pagina("/reglas").await;
    assert!(html.contains("Normas del tracker"), "{html}");
    for (id, valor) in [
        ("normas-umbral", "10.0"),
        ("normas-siembra", "7"),
        ("normas-ratio", "1.0"),
        ("normas-plazo", "30"),
        ("normas-pausa", "2.0"),
        ("normas-aviso", "24"),
    ] {
        let c = campo(&html, id);
        assert!(c.contains(&format!("value=\"{valor}\"")), "{c}");
    }
    assert!(html.contains("avisa 24 horas antes"), "{html}");
    let (s, cuerpo) = u.llamar("/api/reglas", json!({})).await;
    assert_eq!(s, StatusCode::OK);
    let v: Value = serde_json::from_str(&cuerpo).unwrap();
    assert_eq!(v["normas"]["siembra_dias"], 7);

    // Fuera de rango, no; y nada cambia.
    for malas in [
        json!({ "umbral_pct": 101.0, "siembra_dias": 7, "ratio_min": 1.0,
                "plazo_largo_dias": 30, "ratio_pausa": 2.0, "aviso_horas": 24 }),
        json!({ "umbral_pct": 10.0, "siembra_dias": 400, "ratio_min": 1.0,
                "plazo_largo_dias": 400, "ratio_pausa": 2.0, "aviso_horas": 24 }),
        json!({ "umbral_pct": 10.0, "siembra_dias": 7, "ratio_min": 1.0,
                "plazo_largo_dias": 30, "ratio_pausa": 2.0, "aviso_horas": 0 }),
    ] {
        let (s, cuerpo) = u
            .llamar("/api/guardar_normas", json!({ "normas": malas }))
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{malas} {cuerpo}");
    }
    assert_eq!(
        db::normas(&u.p.state.db).await.unwrap(),
        reglas::NormasTracker::default()
    );

    // Unas buenas se guardan y cambian lo que dice el jardín.
    let n = reglas::NormasTracker {
        ratio_min: 1.5,
        ratio_pausa: 3.0,
        plazo_largo_dias: 40,
        aviso_horas: 12,
        ..Default::default()
    };
    let (s, _) = u
        .llamar("/api/guardar_normas", json!({ "normas": n }))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(db::normas(&u.p.state.db).await.unwrap(), n);
    let html = u.pagina("/?bancal=creciendo").await;
    let m = maceta(&html, ESPERA);
    assert!(m.contains("Le falta ratio 1,5 (o faltan 30 días)"), "{m}");
    assert!(
        html.contains("hasta llegar a ratio 1,5 o a los 40 días"),
        "{html}"
    );
    let html = u.pagina("/reglas").await;
    assert!(html.contains("avisa 12 horas antes"), "{html}");
}

#[tokio::test]
async fn con_las_normas_por_defecto_el_jardin_dice_lo_de_siempre() {
    let u = ui().await;
    let html = u.pagina("/?bancal=creciendo").await;
    assert!(
        html.contains(
            "Les falta ratio. Siguen sembrando hasta llegar a ratio 1,0 o a los 30 días, y \
             hasta entonces no se pueden recoger."
        ),
        "{html}"
    );
    let html = u.pagina("/?bancal=recien").await;
    assert!(
        html.contains("Aún no se pueden recoger: llevan menos de 7 días sembrando."),
        "{html}"
    );
    // Y el borrado a mano de uno que crece lo explica con las mismas cifras.
    let (s, cuerpo) = u
        .llamar("/api/quitar", quitar(ESPERA, false, Some(CLAVE)))
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert!(
        cuerpo.contains("hasta llegar a ratio 1,0 o a los 30 días sembrando"),
        "{cuerpo}"
    );
}

#[tokio::test]
async fn sin_fichero_de_estado_la_red_no_se_vigila() {
    let p = con_sim_y(
        qbt_sim::Conf {
            api_abierta: true,
            ..Default::default()
        },
        |c| c.estado_red = None,
    )
    .await;
    p.sim().control.fijar_torrents(torrents());
    let cookie = sesion(&p).await;
    let resp = enviar(&p.app, get("/", Some(&cookie))).await;
    let html = texto(resp).await;
    let letrero = letrero_del_tiempo(&html);
    // Buen tiempo, sin tormenta, y dicho que no se vigila.
    assert!(letrero.contains("Buen tiempo"), "{letrero}");
    assert!(!letrero.contains("La VPN no responde."), "{letrero}");
    assert!(letrero.contains("no se vigilan"), "{letrero}");
}
