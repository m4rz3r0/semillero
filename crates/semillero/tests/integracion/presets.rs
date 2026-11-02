//! Presets por la interfaz: guardar la configuración, subir un archivo,
//! descargarlo, aplicarlo, cambiarlo y eliminarlo; el incluido no se
//! elimina; todo exige sesión y token y nada se guarda si el archivo falla.

use axum::http::StatusCode;
use reglas::preset::{self, Preset};
use reglas::{Estado, NormasTracker, ReglasCategoria};
use semillero::db::{self, Evento};
use serde_json::{Value, json};

use crate::comun::{enviar, get, texto};
use crate::interfaz::{OBLIGADO, Ui, api, ui};

/// Un preset para un tracker más suelto: 1 día de siembra y ratio 0,2.
const SUELTO: &str = r#"formato = 1
nombre = "Tracker suelto"
descripcion = "Siembra 1 día y ratio 0,2."

[normas]
umbral_pct = 20.0
siembra_dias = 1
ratio_min = 0.2
plazo_largo_dias = 10
ratio_pausa = 4.0
aviso_horas = 12

[categorias.series]
ratio = 3.0

[categorias.sin_categoria]
inactividad_dias = 20
"#;

fn suelto() -> Preset {
    Preset::leer(SUELTO).unwrap()
}

impl Ui {
    async fn presets(&self) -> Vec<Value> {
        let (s, cuerpo) = self.llamar("/api/reglas", json!({})).await;
        assert_eq!(s, StatusCode::OK);
        let v: Value = serde_json::from_str(&cuerpo).unwrap();
        v["presets"].as_array().unwrap().clone()
    }

    async fn id_de(&self, nombre: &str) -> i64 {
        self.presets()
            .await
            .iter()
            .find(|p| p["nombre"] == nombre)
            .unwrap_or_else(|| panic!("falta el preset {nombre}"))["id"]
            .as_i64()
            .unwrap()
    }

    async fn subir(&self, archivo: &str) -> (StatusCode, String) {
        self.llamar("/api/subir_preset", json!({ "archivo": archivo }))
            .await
    }

    /// `GET` de la descarga: estado, cabeceras y cuerpo.
    async fn descargar(&self, id: i64) -> (StatusCode, axum::http::HeaderMap, String) {
        let r = enviar(
            &self.p.app,
            get(
                &format!("/api/descargar_preset?id={id}"),
                Some(&self.cookie),
            ),
        )
        .await;
        let (s, h) = (r.status(), r.headers().clone());
        (s, h, texto(r).await)
    }

    async fn estado_de(&self, hash: &str) -> Estado {
        let (_, j) = self.llamar("/api/portada", json!({})).await;
        let d: semillero::api::DatosPortada = serde_json::from_str(&j).unwrap();
        d.torrents.iter().find(|t| t.hash == hash).unwrap().estado
    }
}

#[tokio::test]
async fn cada_accion_de_presets_exige_sesion_y_token() {
    let u = ui().await;
    let llamadas: [(&str, Value); 5] = [
        (
            "/api/guardar_preset",
            json!({ "nombre": "Uno", "descripcion": "" }),
        ),
        ("/api/subir_preset", json!({ "archivo": SUELTO })),
        (
            "/api/cambiar_preset",
            json!({ "id": 0, "nombre": "Otro", "descripcion": "", "sobrescribir": true }),
        ),
        ("/api/aplicar_preset", json!({ "id": 0 })),
        ("/api/eliminar_preset", json!({ "id": 0 })),
    ];
    for (uri, cuerpo) in &llamadas {
        let sin_sesion = enviar(&u.p.app, api(uri, cuerpo.clone(), None, None)).await;
        assert_eq!(sin_sesion.status(), StatusCode::UNAUTHORIZED, "{uri}");
        let sin_token = enviar(&u.p.app, api(uri, cuerpo.clone(), Some(&u.cookie), None)).await;
        assert_eq!(sin_token.status(), StatusCode::FORBIDDEN, "{uri}");
    }
    let sin_sesion = enviar(&u.p.app, get("/api/descargar_preset?id=0", None)).await;
    assert_eq!(sin_sesion.status(), StatusCode::UNAUTHORIZED);
    assert!(texto(sin_sesion).await.find("formato").is_none());
    // Nada cambió: solo el incluido, y las normas de siempre.
    assert_eq!(u.presets().await.len(), 1);
    assert_eq!(
        db::normas(&u.p.state.db).await.unwrap(),
        NormasTracker::default()
    );
}

#[tokio::test]
async fn el_incluido_esta_siempre_se_aplica_y_se_descarga_pero_no_se_cambia() {
    let u = ui().await;
    let lista = u.presets().await;
    assert_eq!(lista.len(), 1);
    assert_eq!(lista[0]["id"], 0);
    assert_eq!(lista[0]["incluido"], true);
    assert_eq!(lista[0]["nombre"], Preset::neutro().nombre);

    let (s, _) = u.llamar("/api/eliminar_preset", json!({ "id": 0 })).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = u
        .llamar(
            "/api/cambiar_preset",
            json!({ "id": 0, "nombre": "Mío", "descripcion": "", "sobrescribir": true }),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert_eq!(u.presets().await.len(), 1);

    let (s, _, cuerpo) = u.descargar(0).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(cuerpo, Preset::neutro().escribir());

    // Aplicarlo devuelve todo a lo de por defecto.
    db::guardar_normas(
        &u.p.state.db,
        &NormasTracker {
            siembra_dias: 1,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    db::guardar_reglas(
        &u.p.state.db,
        "series",
        &ReglasCategoria {
            ratio: Some(2.0),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let (s, _) = u.llamar("/api/aplicar_preset", json!({ "id": 0 })).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        db::normas(&u.p.state.db).await.unwrap(),
        NormasTracker::default()
    );
    assert!(
        db::reglas(&u.p.state.db)
            .await
            .unwrap()
            .values()
            .all(ReglasCategoria::apagadas)
    );
}

#[tokio::test]
async fn subir_y_aplicar_un_preset_cambia_la_clasificacion() {
    let u = ui().await;
    // Con las normas de siempre, 2 días sembrando es recién sembrado.
    assert_eq!(u.estado_de(OBLIGADO).await, Estado::Obligado);

    let (s, cuerpo) = u.subir(SUELTO).await;
    assert_eq!(s, StatusCode::OK, "{cuerpo}");
    let id = u.id_de("Tracker suelto").await;
    assert!(id > 0);
    // Subirlo no cambia nada todavía.
    assert_eq!(u.estado_de(OBLIGADO).await, Estado::Obligado);

    let (s, _) = u.llamar("/api/aplicar_preset", json!({ "id": id })).await;
    assert_eq!(s, StatusCode::OK);
    let p = suelto();
    assert_eq!(db::normas(&u.p.state.db).await.unwrap(), p.normas);
    let guardadas = db::reglas(&u.p.state.db).await.unwrap();
    assert_eq!(guardadas["series"], p.categorias["series"]);
    assert_eq!(guardadas[""], p.categorias[""]);
    // Lo que había en películas y no está en el preset, apagado.
    assert!(
        guardadas
            .get("peliculas")
            .is_none_or(ReglasCategoria::apagadas)
    );
    assert_eq!(u.estado_de(OBLIGADO).await, Estado::Cumplido);
    // Queda apuntado en el historial.
    let h = db::historial(&u.p.state.db, 10).await.unwrap();
    assert!(
        h.iter().any(|a| a.evento == Evento::PresetAplicado
            && a.hash.is_empty()
            && a.detalle.contains("Tracker suelto")),
        "{h:?}"
    );
    let html = u.pagina("/historial").await;
    assert!(html.contains("Preset aplicado"), "{html}");
}

#[tokio::test]
async fn guardar_como_preset_y_descargarlo_da_la_configuracion_de_ahora() {
    let u = ui().await;
    let n = NormasTracker {
        siembra_dias: 9,
        ratio_min: 1.2,
        ..Default::default()
    };
    db::guardar_normas(&u.p.state.db, &n).await.unwrap();
    let r = ReglasCategoria {
        ratio: Some(2.5),
        inactividad_dias: Some(21),
        ..Default::default()
    };
    db::guardar_reglas(&u.p.state.db, "peliculas", &r)
        .await
        .unwrap();
    // Una categoría con todo apagado no hace falta en el archivo.
    db::guardar_reglas(&u.p.state.db, "series", &ReglasCategoria::default())
        .await
        .unwrap();

    let (s, cuerpo) = u
        .llamar(
            "/api/guardar_preset",
            json!({ "nombre": "  Mi tracker  ", "descripcion": "Para casa." }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{cuerpo}");
    let id = u.id_de("Mi tracker").await;

    let (s, h, cuerpo) = u.descargar(id).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h["content-type"], "application/toml; charset=utf-8");
    assert_eq!(
        h["content-disposition"],
        "attachment; filename=\"semillero-preset-mi-tracker.toml\""
    );
    let p = Preset::leer(&cuerpo).unwrap();
    assert_eq!(p.nombre, "Mi tracker");
    assert_eq!(p.descripcion, "Para casa.");
    assert_eq!(p.normas, n);
    assert_eq!(p.categorias.len(), 1);
    assert_eq!(p.categorias["peliculas"], r);

    // Ida y vuelta: lo descargado se sube con otro nombre y es lo mismo.
    let otro = cuerpo.replace("Mi tracker", "Copia");
    let (s, _) = u.subir(&otro).await;
    assert_eq!(s, StatusCode::OK);
    let (_, _, copia) = u.descargar(u.id_de("Copia").await).await;
    assert_eq!(copia, otro);

    // El mismo nombre (sin mirar mayúsculas), no.
    let (s, cuerpo) = u
        .llamar(
            "/api/guardar_preset",
            json!({ "nombre": "MI TRACKER", "descripcion": "" }),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{cuerpo}");
    assert!(cuerpo.contains("Ya hay un preset"), "{cuerpo}");
    // Ni el del incluido.
    let (s, _) = u
        .llamar(
            "/api/guardar_preset",
            json!({ "nombre": Preset::neutro().nombre, "descripcion": "" }),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn el_nombre_del_archivo_descargado_es_seguro() {
    let u = ui().await;
    let raro = SUELTO.replace("Tracker suelto", "../Ñu \\\"x\\\"; rm");
    let (s, cuerpo) = u.subir(&raro).await;
    assert_eq!(s, StatusCode::OK, "{cuerpo}");
    let id = u.presets().await[1]["id"].as_i64().unwrap();
    let (s, h, _) = u.descargar(id).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        h["content-disposition"],
        "attachment; filename=\"semillero-preset-u-x-rm.toml\""
    );
    // Uno que no existe, 404.
    let (s, _, _) = u.descargar(999).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn un_archivo_malo_se_rechaza_con_su_motivo_y_no_se_guarda_nada() {
    let u = ui().await;
    let casos: [(String, &str); 6] = [
        (SUELTO.replace("formato = 1", "formato = 2"), "formato 2"),
        (
            SUELTO.replace("aviso_horas = 12", "aviso_horas = 12\nvelocidad = 3"),
            "velocidad",
        ),
        (
            SUELTO.replace("siembra_dias = 1", "siembra_dias = 9000"),
            "días de siembra",
        ),
        (
            SUELTO.replace("[categorias.series]", "[categorias.musica]"),
            "musica",
        ),
        (
            SUELTO.replace("Tracker suelto", &"x".repeat(preset::MAX_NOMBRE + 1)),
            "nombre",
        ),
        ("esto no es un preset".to_owned(), "TOML"),
    ];
    for (archivo, palabra) in &casos {
        let (s, cuerpo) = u.subir(archivo).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{archivo}");
        assert!(cuerpo.contains(palabra), "{cuerpo:?} no habla de {palabra}");
    }
    // Más de 64 KiB: se rechaza sin leerlo.
    let grande = format!("{SUELTO}# {}\n", "x".repeat(preset::MAX_BYTES));
    let (s, cuerpo) = u.subir(&grande).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE, "{cuerpo}");
    assert!(cuerpo.contains("64 KiB"), "{cuerpo}");
    // Y una petición mucho más grande ni llega.
    let enorme = "x".repeat(4 * preset::MAX_BYTES);
    let (s, _) = u.subir(&enorme).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    // El mismo nombre que uno que ya está, tampoco.
    let (s, _) = u.subir(SUELTO).await;
    assert_eq!(s, StatusCode::OK);
    let (s, cuerpo) = u.subir(SUELTO).await;
    assert_eq!(s, StatusCode::CONFLICT, "{cuerpo}");

    assert_eq!(u.presets().await.len(), 2);
    assert_eq!(
        db::normas(&u.p.state.db).await.unwrap(),
        NormasTracker::default()
    );
}

#[tokio::test]
async fn cambiar_renombra_y_sobrescribe_con_la_configuracion_de_ahora() {
    let u = ui().await;
    u.subir(SUELTO).await;
    u.subir(&SUELTO.replace("Tracker suelto", "Otro")).await;
    let id = u.id_de("Tracker suelto").await;

    // Solo el nombre y la descripción: las cifras siguen.
    let (s, cuerpo) = u
        .llamar(
            "/api/cambiar_preset",
            json!({ "id": id, "nombre": "Suelto", "descripcion": "Nueva.", "sobrescribir": false }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{cuerpo}");
    let (_, _, archivo) = u.descargar(id).await;
    let p = Preset::leer(&archivo).unwrap();
    assert_eq!(
        (p.nombre.as_str(), p.descripcion.as_str()),
        ("Suelto", "Nueva.")
    );
    assert_eq!(p.normas, suelto().normas);
    assert_eq!(p.categorias, suelto().categorias);

    // Sobrescribir: las cifras de ahora (las de por defecto, sin reglas).
    let (s, _) = u
        .llamar(
            "/api/cambiar_preset",
            json!({ "id": id, "nombre": "Suelto", "descripcion": "Nueva.", "sobrescribir": true }),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (_, _, archivo) = u.descargar(id).await;
    let p = Preset::leer(&archivo).unwrap();
    assert_eq!(p.normas, NormasTracker::default());
    assert!(p.categorias.is_empty());

    // Al nombre de otro, no; con un nombre vacío, tampoco.
    for nombre in ["otro", "   "] {
        let (s, _) = u
            .llamar(
                "/api/cambiar_preset",
                json!({ "id": id, "nombre": nombre, "descripcion": "", "sobrescribir": false }),
            )
            .await;
        assert_ne!(s, StatusCode::OK, "{nombre:?}");
    }
    assert!(u.presets().await.iter().any(|p| p["nombre"] == "Suelto"));
    // Uno que no existe.
    let (s, _) = u
        .llamar(
            "/api/cambiar_preset",
            json!({ "id": 999, "nombre": "X", "descripcion": "", "sobrescribir": false }),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn eliminar_quita_el_preset_y_nada_mas() {
    let u = ui().await;
    u.subir(SUELTO).await;
    let id = u.id_de("Tracker suelto").await;
    let (s, _) = u.llamar("/api/eliminar_preset", json!({ "id": id })).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(u.presets().await.len(), 1);
    let (s, _) = u.llamar("/api/eliminar_preset", json!({ "id": id })).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    // Las normas no se tocan al eliminar.
    assert_eq!(
        db::normas(&u.p.state.db).await.unwrap(),
        NormasTracker::default()
    );
}

#[tokio::test]
async fn la_pagina_de_reglas_ensena_los_presets_en_palabras() {
    let u = ui().await;
    u.subir(SUELTO).await;
    let html = u.pagina("/reglas").await;
    for texto in [
        "Presets",
        "Por defecto",
        "Incluido",
        "Tracker suelto",
        "Siembra 1 día y ratio 0,2.",
        "Guardar como preset",
        "Subir preset",
        "Aplicar",
        "Descargar",
        "Cambiar",
        "Eliminar",
        "/api/descargar_preset?id=0",
    ] {
        assert!(html.contains(texto), "falta {texto:?} en las reglas");
    }
    // Las tarjetas de siempre siguen ahí.
    for base in ["reglas-peliculas", "reglas-series", "reglas-sin"] {
        assert!(html.contains(&format!("id=\"{base}-ratio\"")), "{base}");
    }
    // Ningún nombre interno a la vista (los datos para el navegador van aparte).
    for interno in ["subir_preset", "aplicar_preset", "formato = 1"] {
        assert!(!html.contains(interno), "la página enseña {interno:?}");
    }
}

#[tokio::test]
async fn el_limite_mayor_es_solo_para_subir_presets() {
    let u = ui().await;
    // 40 KiB caben en la subida de un preset (y se rechazan por no serlo)...
    let (s, _) = u.subir(&"x".repeat(40 * 1024)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // ...pero en cualquier otra acción siguen siendo demasiado.
    let (s, _) = u
        .llamar(
            "/api/guardar_preset",
            json!({ "nombre": "x".repeat(40 * 1024), "descripcion": "" }),
        )
        .await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(u.presets().await.len(), 1);
}

#[tokio::test]
async fn con_borrado_real_aplicar_pide_la_contrasena() {
    use semillero::api::ERROR_REAUTH;
    let u = ui().await;
    semillero::tarea::cambiar_modo(
        &u.p.state.db,
        db::Modo::BorradoReal,
        semillero::tarea::ahora_unix(),
    )
    .await
    .unwrap();
    let siembra_1 = NormasTracker {
        siembra_dias: 1,
        ..Default::default()
    };
    db::guardar_normas(&u.p.state.db, &siembra_1).await.unwrap();
    // Sin contraseña reciente: se pide y no cambia nada.
    let (s, c) = u.llamar("/api/aplicar_preset", json!({ "id": 0 })).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{c}");
    assert!(c.contains(ERROR_REAUTH), "{c}");
    assert_eq!(db::normas(&u.p.state.db).await.unwrap(), siembra_1);
    // Contraseña mala: tampoco.
    let (s, _) = u
        .llamar("/api/aplicar_preset", json!({ "id": 0, "clave": "mala" }))
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(db::normas(&u.p.state.db).await.unwrap(), siembra_1);
    // Con la buena (pasado el freno que dejó la mala), se aplica.
    sqlx::query("DELETE FROM freno")
        .execute(&u.p.state.db)
        .await
        .unwrap();
    let (s, c) = u
        .llamar(
            "/api/aplicar_preset",
            json!({ "id": 0, "clave": crate::comun::CLAVE }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{c}");
    assert_eq!(
        db::normas(&u.p.state.db).await.unwrap(),
        NormasTracker::default()
    );
}

/// Con el borrado real, guardar las normas o las reglas de una categoría
/// pide la contraseña reciente, como aplicar un preset (S11): sin ella o
/// con una mala no cambia nada; con la buena se guarda.
#[tokio::test]
async fn con_borrado_real_guardar_normas_y_reglas_pide_la_contrasena() {
    use semillero::api::ERROR_REAUTH;
    let u = ui().await;
    semillero::tarea::cambiar_modo(
        &u.p.state.db,
        db::Modo::BorradoReal,
        semillero::tarea::ahora_unix(),
    )
    .await
    .unwrap();
    let normas = NormasTracker {
        siembra_dias: 3,
        ..Default::default()
    };
    let reglas = ReglasCategoria {
        ratio: Some(2.5),
        ..Default::default()
    };
    let llamadas = [
        ("/api/guardar_normas", json!({ "normas": normas })),
        (
            "/api/guardar_reglas",
            json!({ "categoria": "peliculas", "reglas": reglas }),
        ),
    ];
    let normas_antes = db::normas(&u.p.state.db).await.unwrap();
    let reglas_antes = db::reglas(&u.p.state.db).await.unwrap();
    let sin_cambios = |u: &Ui| {
        let db = u.p.state.db.clone();
        let (n, r) = (normas_antes, reglas_antes.clone());
        async move {
            assert_eq!(db::normas(&db).await.unwrap(), n);
            assert_eq!(db::reglas(&db).await.unwrap(), r);
        }
    };
    for (ruta, cuerpo) in &llamadas {
        // Sin contraseña reciente: se pide y no cambia nada.
        let (s, c) = u.llamar(ruta, cuerpo.clone()).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{ruta} {c}");
        assert!(c.contains(ERROR_REAUTH), "{ruta} {c}");
        sin_cambios(&u).await;
        // Contraseña mala: tampoco.
        let mut mala = cuerpo.clone();
        mala["clave"] = json!("mala");
        let (s, c) = u.llamar(ruta, mala).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{ruta} {c}");
        assert!(c.contains("La contraseña no es correcta."), "{ruta} {c}");
        sin_cambios(&u).await;
        sqlx::query("DELETE FROM freno")
            .execute(&u.p.state.db)
            .await
            .unwrap();
    }
    // Con la buena, se guardan las normas; y durante 5 minutos las reglas
    // ya no la piden.
    let mut buena = llamadas[0].1.clone();
    buena["clave"] = json!(crate::comun::CLAVE);
    let (s, c) = u.llamar(llamadas[0].0, buena).await;
    assert_eq!(s, StatusCode::OK, "{c}");
    assert_eq!(db::normas(&u.p.state.db).await.unwrap(), normas);
    let (s, c) = u.llamar(llamadas[1].0, llamadas[1].1.clone()).await;
    assert_eq!(s, StatusCode::OK, "{c}");
    assert_eq!(
        db::reglas(&u.p.state.db)
            .await
            .unwrap()
            .get("peliculas")
            .copied(),
        Some(reglas)
    );
}

/// En modo solo vigilar, guardar normas y reglas no pide la contraseña.
#[tokio::test]
async fn en_solo_vigilar_guardar_normas_y_reglas_no_pide_la_contrasena() {
    let u = ui().await;
    let normas = NormasTracker {
        siembra_dias: 3,
        ..Default::default()
    };
    let (s, c) = u
        .llamar("/api/guardar_normas", json!({ "normas": normas }))
        .await;
    assert_eq!(s, StatusCode::OK, "{c}");
    let reglas = ReglasCategoria {
        ratio: Some(2.5),
        ..Default::default()
    };
    let (s, c) = u
        .llamar(
            "/api/guardar_reglas",
            json!({ "categoria": "peliculas", "reglas": reglas }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{c}");
    assert_eq!(db::normas(&u.p.state.db).await.unwrap(), normas);
    assert_eq!(
        db::reglas(&u.p.state.db)
            .await
            .unwrap()
            .get("peliculas")
            .copied(),
        Some(reglas)
    );
}
