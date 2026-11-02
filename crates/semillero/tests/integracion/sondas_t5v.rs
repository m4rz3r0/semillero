//! Sondas del verificador independiente de T5v (jardín, «Recoger», bancales).
//! Todo lo rechazado deja la base y qBittorrent como estaban.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use reglas::{Aviso, TipoRegla};
use semillero::api::ERROR_REAUTH;
use semillero::db;
use serde_json::{Value, json};

use crate::comun::*;

const DIA: i64 = 86_400;
const CUMPLIDO: &str = "1111111111111111111111111111111111111111";
const OBLIGADO: &str = "2222222222222222222222222222222222222222";
const ESPERA: &str = "3333333333333333333333333333333333333333";
const LIBRE: &str = "4444444444444444444444444444444444444444";

fn torrent(hash: &str, nombre: &str, descargado: i64, dias: i64, ratio: f64) -> Value {
    json!({
        "hash": hash, "name": nombre, "category": "peliculas", "state": "stalledUP",
        "downloaded": descargado, "total_size": 1000, "uploaded": 0,
        "seeding_time": dias * DIA, "ratio": ratio, "added_on": 1_700_000_000,
        "save_path": "/media/peliculas", "upspeed": 0, "dlspeed": 0, "progress": 1.0
    })
}

fn todos() -> Vec<Value> {
    vec![
        torrent(CUMPLIDO, "Cumplida", 1000, 8, 1.5),
        torrent(OBLIGADO, "Obligada", 1000, 2, 0.3),
        torrent(ESPERA, "Espera", 1000, 10, 0.5),
        torrent(LIBRE, "Libre", 0, 0, 0.0),
    ]
}

struct U {
    p: Prueba,
    c: String,
    t: String,
}

async fn u_con(lista: Vec<Value>) -> U {
    let p = con_sim(qbt_sim::Conf {
        api_abierta: true,
        ..Default::default()
    })
    .await;
    p.sim().control.fijar_torrents(lista);
    let c = sesion(&p).await;
    let t = token(&p, &c).await;
    U { p, c, t }
}

fn api(uri: &str, cuerpo: &Value, cookie: &str, token: Option<&str>) -> Request<Body> {
    let mut req = post(uri, &cuerpo.to_string());
    let h = req.headers_mut();
    h.insert("content-type", "application/json".parse().unwrap());
    h.insert("cookie", cookie.parse().unwrap());
    if let Some(t) = token {
        h.insert("x-csrf-token", t.parse().unwrap());
    }
    req
}

impl U {
    async fn llamar(&self, uri: &str, cuerpo: Value) -> (StatusCode, String) {
        let r = enviar(&self.p.app, api(uri, &cuerpo, &self.c, Some(&self.t))).await;
        (r.status(), texto(r).await)
    }
    async fn pagina(&self, ruta: &str) -> (StatusCode, String) {
        let r = enviar(&self.p.app, get(ruta, Some(&self.c))).await;
        (r.status(), texto(r).await)
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
    async fn foto(&self) -> String {
        let db = &self.p.state.db;
        let avisos = db::avisos(db).await.unwrap();
        let mut ex: Vec<_> = db::exenciones(db).await.unwrap().into_iter().collect();
        ex.sort();
        let hist = db::historial(db, 1000).await.unwrap().len();
        let modo = db::modo(db).await.unwrap();
        format!(
            "{avisos:?}|{ex:?}|{hist}|{modo:?}|{}|{}",
            self.p.sim().control.torrents().len(),
            self.borrados().len()
        )
    }
}

fn quitar(hash: &str, ficheros: bool, clave: Option<&str>) -> Value {
    json!({ "hash": hash, "ficheros": ficheros, "clave": clave })
}

/// El bancal elegido en la página: el `aria-selected="true"`.
fn elegido(html: &str) -> &'static str {
    for b in ["listos", "creciendo", "recien", "sin-arraigar"] {
        let id = format!("id=\"pestana-{b}\"");
        let p = html.find(&id).unwrap_or_else(|| panic!("falta {id}"));
        let f = p + html[p..].find('>').unwrap();
        if html[p..f].contains("aria-selected=\"true\"") {
            return b;
        }
    }
    panic!("ningún bancal elegido");
}

fn panel(html: &str) -> &str {
    let p = html.find("role=\"tabpanel\"").expect("falta el bancal");
    let i = html[..p].rfind("<section").unwrap();
    let f = p + html[p..].find("</section>").unwrap();
    &html[i..f]
}

#[tokio::test]
async fn sonda_t5v_bancal_invalido_cae_al_primero_con_macetas_sin_reflejarse() {
    // Sin cumplidos: el primero con macetas es «Creciendo».
    let u = u_con(vec![
        torrent(OBLIGADO, "Obligada", 1000, 2, 0.3),
        torrent(ESPERA, "Espera", 1000, 10, 0.5),
    ])
    .await;
    let (s, html) = u.pagina("/").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(elegido(&html), "creciendo");
    for q in [
        "%3Cscript%3Ealert(1)%3C%2Fscript%3E",
        "%22%3E%3Cimg%20src%3Dx%20onerror%3Dalert(1)%3E",
        "LISTOS",
        "listos%00",
        "",
        "recien&bancal=listos",
    ] {
        let (s, html) = u.pagina(&format!("/?bancal={q}")).await;
        assert_eq!(s, StatusCode::OK, "{q}");
        assert!(!html.contains("<script>alert"), "{q} reflejado");
        assert!(!html.contains("onerror=alert"), "{q} reflejado");
        assert!(!html.contains("LISTOS"), "{q} reflejado");
        let b = elegido(&html);
        if q.starts_with("recien") {
            // Repetido: vale cualquiera de los dos (gana el último).
            assert!(b == "recien" || b == "listos", "{q}: {b}");
        } else {
            assert_eq!(b, "creciendo", "{q}");
        }
    }
    // Un bancal válido aunque esté vacío se respeta.
    let (_, html) = u.pagina("/?bancal=listos").await;
    assert_eq!(elegido(&html), "listos");
    assert!(panel(&html).contains("Ninguna maceta"), "{}", panel(&html));
}

#[tokio::test]
async fn sonda_t5v_recoger_solo_en_listos_y_sin_arraigar() {
    let u = u_con(todos()).await;
    for (b, con) in [
        ("listos", true),
        ("creciendo", false),
        ("recien", false),
        ("sin-arraigar", true),
    ] {
        let (_, html) = u.pagina(&format!("/?bancal={b}")).await;
        let p = panel(&html);
        assert_eq!(p.contains(">Recoger<"), con, "{b}: {p}");
        assert!(!p.contains("Quitar sin borrar"), "{b}: sin ficha en el SSR");
    }
}

#[tokio::test]
async fn sonda_t5v_servidor_rechaza_recien_y_creciendo_sin_cambiar_nada() {
    let u = u_con(todos()).await;
    let antes = u.foto().await;
    for h in [OBLIGADO, ESPERA] {
        for fich in [true, false] {
            let (s, cuerpo) = u.llamar("/api/quitar", quitar(h, fich, Some(CLAVE))).await;
            assert_eq!(s, StatusCode::CONFLICT, "{h} {fich}: {cuerpo}");
            assert!(
                cuerpo.contains("recién sembrado") || cuerpo.contains("creciendo"),
                "{cuerpo}"
            );
            for n in ["Obligado", "En espera", "Cumplido", "Libre"] {
                assert!(!cuerpo.contains(n), "{cuerpo}");
            }
        }
    }
    assert!(u.borrados().is_empty());
    assert_eq!(u.foto().await, antes);
}

#[tokio::test]
async fn sonda_t5v_sin_token_o_sin_contrasena_no_se_recoge_nada() {
    let u = u_con(todos()).await;
    let antes = u.foto().await;
    // Sin CSRF.
    let r = enviar(
        &u.p.app,
        api(
            "/api/quitar",
            &quitar(CUMPLIDO, true, Some(CLAVE)),
            &u.c,
            None,
        ),
    )
    .await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    // Token ajeno.
    let r = enviar(
        &u.p.app,
        api(
            "/api/quitar",
            &quitar(CUMPLIDO, true, Some(CLAVE)),
            &u.c,
            Some("x".repeat(43).as_str()),
        ),
    )
    .await;
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
    // Sin la contraseña (ventana cerrada): Recoger, Quitar sin ficheros y
    // el borrado real.
    for f in [true, false] {
        let (s, c) = u.llamar("/api/quitar", quitar(LIBRE, f, None)).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{c}");
        assert!(c.contains(ERROR_REAUTH));
    }
    let (s, c) = u
        .llamar("/api/cambiar_modo", json!({ "real": true, "clave": null }))
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{c}");
    // Contraseña mala.
    let (s, _) = u
        .llamar("/api/quitar", quitar(LIBRE, true, Some("mala")))
        .await;
    assert_ne!(s, StatusCode::OK);
    assert!(u.borrados().is_empty());
    assert_eq!(u.foto().await, antes);
}

#[tokio::test]
async fn sonda_t5v_no_recoger_y_volver_a_contar_por_regla_y_en_la_ficha() {
    let u = u_con(todos()).await;
    db::guardar_avisos(
        &u.p.state.db,
        &[Aviso {
            hash: CUMPLIDO.into(),
            regla: TipoRegla::Ratio,
            creado_en: 0,
            vence_en: i64::MAX / 2,
        }],
    )
    .await
    .unwrap();
    let (_, html) = u.pagina("/?bancal=listos").await;
    assert!(html.contains("No recoger"), "falta «No recoger»");
    assert!(html.contains("(regla: ratio)"), "falta la regla del aviso");

    let (s, _) = u
        .llamar(
            "/api/no_borrar",
            json!({ "hash": CUMPLIDO, "regla": "Ratio" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let ex = db::exenciones(&u.p.state.db).await.unwrap();
    assert!(
        ex.contains(&(CUMPLIDO.to_owned(), TipoRegla::Ratio)),
        "{ex:?}"
    );
    assert_eq!(ex.len(), 1, "{ex:?}");
    // La portada lo cuenta, solo en ese torrent.
    let (_, j) = u.llamar("/api/portada", json!({})).await;
    let v: Value = serde_json::from_str(&j).unwrap();
    for t in v["torrents"].as_array().unwrap() {
        let e = &t["exenciones"];
        if t["hash"] == CUMPLIDO {
            assert_eq!(e, &json!(["Ratio"]), "{t}");
        } else {
            assert_eq!(e, &json!([]), "{t}");
        }
    }
    let (_, hist) = u.pagina("/historial").await;
    assert!(hist.contains("No recoger"), "historial sin «No recoger»");

    let (s, _) = u
        .llamar(
            "/api/quitar_exencion",
            json!({ "hash": CUMPLIDO, "regla": "Ratio" }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert!(db::exenciones(&u.p.state.db).await.unwrap().is_empty());
    let (_, hist) = u.pagina("/historial").await;
    assert!(hist.contains("Vuelve a contar"), "historial sin deshacer");
}

#[tokio::test]
async fn sonda_t5v_historial_distingue_recoger_de_quitar() {
    let u = u_con(todos()).await;
    let (s, _) = u
        .llamar("/api/quitar", quitar(CUMPLIDO, true, Some(CLAVE)))
        .await;
    assert_eq!(s, StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let (s, _) = u
        .llamar("/api/quitar", quitar(LIBRE, false, Some(CLAVE)))
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        u.borrados(),
        vec![
            format!("hashes={CUMPLIDO}&deleteFiles=true"),
            format!("hashes={LIBRE}&deleteFiles=false"),
        ]
    );
    let (_, hist) = u.pagina("/historial").await;
    let i_rec = hist.find(">Recogido<").expect("falta «Recogido»");
    let i_qui = hist
        .find(">Quitado sin borrar los ficheros<")
        .expect("falta «Quitado sin borrar los ficheros»");
    // El más nuevo arriba: primero el quitado (Libre), luego el recogido.
    assert!(i_qui < i_rec);
    assert!(hist[i_rec..].contains("con sus ficheros"));
    for n in ["Obligados", "Cumplidos", "Libres", "En espera"] {
        assert!(!hist.contains(n), "{n}");
    }
}
