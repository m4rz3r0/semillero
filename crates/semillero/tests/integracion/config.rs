//! Configuración por variables de entorno.

use semillero::config::Config;

fn con(vars: &[(&str, &str)]) -> Result<Config, String> {
    let vars: Vec<(String, String)> = vars
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    Config::desde(move |k| vars.iter().find(|(c, _)| c == k).map(|(_, v)| v.clone()))
}

#[test]
fn valores_por_defecto_sin_nada_del_sitio() {
    let c = con(&[]).unwrap();
    assert_eq!(c.escucha, "127.0.0.1:8090".parse().unwrap());
    assert_eq!(c.qbt, "http://127.0.0.1:8080");
    // Modo lista blanca, como siempre: las contraseñas se comprueban desde
    // 127.0.0.1 y semillero no entra en qBittorrent con ninguna cuenta.
    assert_eq!(c.login_desde, None);
    assert_eq!(
        c.login_desde_efectivo(),
        Some(std::net::IpAddr::from([127, 0, 0, 1]))
    );
    assert_eq!(c.cuenta_qbt, None);
    assert_eq!(c.aviso_login(), None);
    assert_eq!(c.db, std::path::Path::new("semillero.db"));
    // Sin fichero de estado de la VPN y del puerto: no se vigila.
    assert_eq!(c.estado_red, None);
    assert_eq!(
        c.origenes,
        vec!["http://127.0.0.1:8090", "http://localhost:8090"]
    );
    // Sin proxy: no se cree a nadie y vale la IP de la conexión.
    assert_eq!(c.proxy, None);
}

#[test]
fn el_origen_por_defecto_sigue_al_puerto() {
    let c = con(&[("SEMILLERO_PORT", "9000")]).unwrap();
    assert_eq!(
        c.origenes,
        vec!["http://127.0.0.1:9000", "http://localhost:9000"]
    );
    let c = con(&[("SEMILLERO_LISTEN", "192.0.2.4:7000")]).unwrap();
    assert_eq!(
        c.origenes,
        vec!["http://127.0.0.1:7000", "http://localhost:7000"]
    );
}

#[test]
fn estado_de_la_red_opcional() {
    let c = con(&[("SEMILLERO_ESTADO", "/run/vpn/estado")]).unwrap();
    assert_eq!(
        c.estado_red.as_deref(),
        Some(std::path::Path::new("/run/vpn/estado"))
    );
}

#[test]
fn proxy_de_confianza() {
    let c = con(&[("SEMILLERO_PROXY", "192.0.2.7")]).unwrap();
    assert_eq!(c.proxy, Some(std::net::IpAddr::from([192, 0, 2, 7])));
    assert!(con(&[("SEMILLERO_PROXY", "proxy")]).is_err());
}

#[test]
fn raiz_de_la_web_junto_al_ejecutable() {
    use semillero::config::raiz_web;
    let exe = std::path::Path::new("/opt/semillero/semillero");
    assert_eq!(
        raiz_web(|_| None, Some(exe)),
        std::path::Path::new("/opt/semillero/site")
    );
    // `LEPTOS_SITE_ROOT` manda.
    let leer = |k: &str| (k == "LEPTOS_SITE_ROOT").then(|| "/srv/web".to_owned());
    assert_eq!(raiz_web(leer, Some(exe)), std::path::Path::new("/srv/web"));
    // Sin saber dónde está el ejecutable, `site` en el directorio de trabajo.
    assert_eq!(raiz_web(|_| None, None), std::path::Path::new("site"));
}

#[test]
fn escucha_como_la_version_anterior() {
    let c = con(&[
        ("SEMILLERO_LISTEN", "192.0.2.1"),
        ("SEMILLERO_PORT", "9000"),
    ])
    .unwrap();
    assert_eq!(c.escucha, "192.0.2.1:9000".parse().unwrap());
    let c = con(&[("SEMILLERO_LISTEN", "192.0.2.1:7000")]).unwrap();
    assert_eq!(c.escucha, "192.0.2.1:7000".parse().unwrap());
    assert!(con(&[("SEMILLERO_LISTEN", "nada")]).is_err());
    assert!(con(&[("SEMILLERO_LOGIN_DESDE", "nada")]).is_err());
}

#[test]
fn varios_origenes() {
    let c = con(&[(
        "SEMILLERO_ORIGEN",
        "https://a.example/, http://b.example:8090",
    )])
    .unwrap();
    assert_eq!(
        c.origenes,
        vec!["https://a.example", "http://b.example:8090"]
    );
}

#[test]
fn sin_jellyfin_por_defecto() {
    assert_eq!(con(&[]).unwrap().jellyfin, None);
}

#[test]
fn jellyfin_con_la_clave_en_un_fichero() {
    let c = con(&[("SEMILLERO_JELLYFIN", "http://192.0.2.9:8096/")]).unwrap();
    let j = c.jellyfin.unwrap();
    assert_eq!(j.url, "http://192.0.2.9:8096");
    // Junto a la base de datos.
    assert_eq!(j.clave, std::path::Path::new("jellyfin.clave"));
    let c = con(&[
        ("SEMILLERO_JELLYFIN", "http://192.0.2.9:8096"),
        ("SEMILLERO_DB", "/var/lib/semillero/semillero.db"),
    ])
    .unwrap();
    assert_eq!(
        c.jellyfin.unwrap().clave,
        std::path::Path::new("/var/lib/semillero/jellyfin.clave")
    );
    let c = con(&[
        ("SEMILLERO_JELLYFIN", "http://192.0.2.9:8096"),
        ("SEMILLERO_JELLYFIN_CLAVE", "/tmp/k"),
    ])
    .unwrap();
    assert_eq!(c.jellyfin.unwrap().clave, std::path::Path::new("/tmp/k"));
}

#[test]
fn sin_ruta_de_la_clave_de_jellyfin_vale_tambien_la_de_la_0_2() {
    let c = con(&[("SEMILLERO_JELLYFIN", "http://192.0.2.9:8096")]).unwrap();
    assert_eq!(
        c.jellyfin.unwrap().antigua.as_deref(),
        Some(std::path::Path::new(
            "/usr/local/etc/semillero/jellyfin.clave"
        ))
    );
    // Con la ruta puesta, solo vale esa.
    let c = con(&[
        ("SEMILLERO_JELLYFIN", "http://192.0.2.9:8096"),
        ("SEMILLERO_JELLYFIN_CLAVE", "/tmp/k"),
    ])
    .unwrap();
    assert_eq!(c.jellyfin.unwrap().antigua, None);
}

#[test]
fn la_clave_de_jellyfin_de_la_0_2_se_usa_si_no_hay_otra() {
    use semillero::config::ConfJellyfin;
    use semillero::mediateca::desde_config;
    let dir = tempfile::tempdir().unwrap();
    let nueva = dir.path().join("jellyfin.clave");
    let antigua = dir.path().join("antigua.clave");
    let j = ConfJellyfin {
        url: "http://127.0.0.1:1".into(),
        clave: nueva.clone(),
        antigua: Some(antigua.clone()),
    };
    // Ninguna de las dos: el error dice la ruta nueva.
    let e = desde_config(Some(&j)).err().unwrap();
    assert!(e.contains(nueva.to_str().unwrap()), "{e}");
    // Solo la antigua: se usa.
    std::fs::write(&antigua, "abc123\n").unwrap();
    assert!(desde_config(Some(&j)).is_ok());
    // Las dos: manda la nueva (vacía, así que falla).
    std::fs::write(&nueva, "\n").unwrap();
    let e = desde_config(Some(&j)).err().unwrap();
    assert!(e.contains("vacío"), "{e}");
    // Sin ruta antigua, la que falta no se sustituye.
    std::fs::remove_file(&nueva).unwrap();
    let sin = ConfJellyfin { antigua: None, ..j };
    assert!(desde_config(Some(&sin)).is_err());
}

#[test]
fn la_clave_de_jellyfin_se_lee_del_fichero() {
    use semillero::mediateca::desde_config;
    assert!(desde_config(None).is_ok());
    let dir = tempfile::tempdir().unwrap();
    let fichero = dir.path().join("jellyfin.clave");
    let c = con(&[
        ("SEMILLERO_JELLYFIN", "http://127.0.0.1:1"),
        ("SEMILLERO_JELLYFIN_CLAVE", fichero.to_str().unwrap()),
    ])
    .unwrap();
    let e = desde_config(c.jellyfin.as_ref()).err().unwrap();
    assert!(e.contains("jellyfin.clave"), "{e}");
    std::fs::write(&fichero, "\n").unwrap();
    let e = desde_config(c.jellyfin.as_ref()).err().unwrap();
    assert!(e.contains("vacío"), "{e}");
    // Una clave de la API es corta: un fichero grande no es la clave.
    std::fs::write(&fichero, "a".repeat(4096)).unwrap();
    let e = desde_config(c.jellyfin.as_ref()).err().unwrap();
    assert!(e.contains("demasiado grande"), "{e}");
    std::fs::write(&fichero, "abc123\n").unwrap();
    assert!(desde_config(c.jellyfin.as_ref()).is_ok());
}

// ---------------------------------------------------------------- modo con contraseña

#[test]
fn modo_con_contrasena() {
    let c = con(&[("SEMILLERO_QBT_CLAVE", "/etc/semillero/qbt.clave")]).unwrap();
    let cuenta = c.cuenta_qbt.clone().unwrap();
    assert_eq!(cuenta.usuario, "admin");
    assert_eq!(
        cuenta.clave,
        std::path::Path::new("/etc/semillero/qbt.clave")
    );
    // Sin SEMILLERO_LOGIN_DESDE, la comprobación no se ata a ninguna IP.
    assert_eq!(c.login_desde_efectivo(), None);
    let c = con(&[
        ("SEMILLERO_QBT_CLAVE", "/etc/semillero/qbt.clave"),
        ("SEMILLERO_QBT_USUARIO", " semillero "),
        ("SEMILLERO_LOGIN_DESDE", "192.0.2.3"),
    ])
    .unwrap();
    assert_eq!(c.cuenta_qbt.as_ref().unwrap().usuario, "semillero");
    assert_eq!(
        c.login_desde_efectivo(),
        Some(std::net::IpAddr::from([192, 0, 2, 3]))
    );
    // El usuario sin la contraseña no activa nada.
    let c = con(&[("SEMILLERO_QBT_USUARIO", "semillero")]).unwrap();
    assert_eq!(c.cuenta_qbt, None);
}

#[test]
fn aviso_si_qbittorrent_no_esta_en_este_equipo_y_no_hay_contrasena() {
    for qbt in [
        "http://192.0.2.5:8080",
        "http://qbittorrent:8080/",
        "https://[2001:db8::5]:8080",
    ] {
        let c = con(&[("SEMILLERO_QBT", qbt)]).unwrap();
        let aviso = c.aviso_login().unwrap_or_else(|| panic!("{qbt}"));
        assert!(aviso.contains("SEMILLERO_QBT_CLAVE"), "{aviso}");
        assert!(aviso.contains("SEMILLERO_LOGIN_DESDE"), "{aviso}");
        // Con la contraseña, o con la IP de origen elegida a mano, no.
        let c = con(&[("SEMILLERO_QBT", qbt), ("SEMILLERO_QBT_CLAVE", "/k")]).unwrap();
        assert_eq!(c.aviso_login(), None, "{qbt}");
        let c = con(&[
            ("SEMILLERO_QBT", qbt),
            ("SEMILLERO_LOGIN_DESDE", "192.0.2.3"),
        ])
        .unwrap();
        assert_eq!(c.aviso_login(), None, "{qbt}");
    }
    for qbt in [
        "http://127.0.0.1:8080",
        "http://localhost:8080",
        "http://[::1]:8080",
        "http://127.0.0.1",
    ] {
        assert_eq!(
            con(&[("SEMILLERO_QBT", qbt)]).unwrap().aviso_login(),
            None,
            "{qbt}"
        );
    }
}

#[test]
fn la_contrasena_de_qbittorrent_se_lee_del_fichero() {
    use semillero::config::leer_contrasena_qbt;
    let dir = tempfile::tempdir().unwrap();
    let fichero = dir.path().join("qbt.clave");
    let c = con(&[("SEMILLERO_QBT_CLAVE", fichero.to_str().unwrap())]).unwrap();
    let cuenta = c.cuenta_qbt.unwrap();
    let e = leer_contrasena_qbt(&cuenta).unwrap_err();
    assert!(e.contains("qbt.clave"), "{e}");
    std::fs::write(&fichero, "\n").unwrap();
    assert!(leer_contrasena_qbt(&cuenta).unwrap_err().contains("vacío"));
    std::fs::write(&fichero, "a".repeat(4096)).unwrap();
    let e = leer_contrasena_qbt(&cuenta).unwrap_err();
    assert!(e.contains("demasiado grande"), "{e}");
    assert!(!e.contains("aaaa"), "{e}");
    // Solo se quita el salto de línea del final: los espacios cuentan.
    std::fs::write(&fichero, " una clave \r\n").unwrap();
    assert_eq!(leer_contrasena_qbt(&cuenta).unwrap(), " una clave ");
    std::fs::write(&fichero, "otra").unwrap();
    assert_eq!(leer_contrasena_qbt(&cuenta).unwrap(), "otra");
}

#[cfg(unix)]
#[test]
fn un_fichero_de_contrasena_que_leen_otros_se_nota() {
    use semillero::config::legible_por_otros;
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let fichero = dir.path().join("qbt.clave");
    std::fs::write(&fichero, "clave").unwrap();
    std::fs::set_permissions(&fichero, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(!legible_por_otros(&fichero));
    std::fs::set_permissions(&fichero, std::fs::Permissions::from_mode(0o640)).unwrap();
    assert!(legible_por_otros(&fichero));
}

// ---------------------------------------------------------------- --env FICHERO

mod fichero_env {
    use semillero::config::{Config, con_fichero, leer_fichero_env};

    fn escribir(texto: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let ruta = dir.path().join("semillero.env");
        std::fs::write(&ruta, texto).unwrap();
        (dir, ruta)
    }

    #[test]
    fn lee_lo_que_escribe_configurar() {
        let (_d, ruta) = escribir(
            "# Configuración de semillero\r\n\
             \r\n\
             SEMILLERO_LISTEN=192.0.2.10:8090\r\n\
             SEMILLERO_QBT=http://192.0.2.20:8080\r\n\
             SEMILLERO_ORIGEN=https://semillero.example\r\n\
             SEMILLERO_QBT=http://192.0.2.21:8080\r\n\
             OTRA=a=b",
        );
        let f = leer_fichero_env(&ruta).unwrap();
        // La última línea de cada variable manda, como en systemd.
        assert_eq!(f["SEMILLERO_QBT"], "http://192.0.2.21:8080");
        // El valor llega tal cual, con lo que haya tras el primer «=».
        assert_eq!(f["OTRA"], "a=b");
        assert_eq!(f.len(), 4);
        let c = Config::desde(con_fichero(|_| None, f)).unwrap();
        assert_eq!(c.escucha, "192.0.2.10:8090".parse().unwrap());
        assert_eq!(c.qbt, "http://192.0.2.21:8080");
        assert_eq!(c.origenes, vec!["https://semillero.example"]);
    }

    #[test]
    fn las_variables_del_entorno_mandan_sobre_el_fichero() {
        let (_d, ruta) = escribir("SEMILLERO_QBT=http://192.0.2.20:8080\nSEMILLERO_PORT=9000\n");
        let f = leer_fichero_env(&ruta).unwrap();
        let entorno =
            |k: &str| (k == "SEMILLERO_QBT").then(|| "http://198.51.100.5:8080".to_owned());
        let c = Config::desde(con_fichero(entorno, f)).unwrap();
        assert_eq!(c.qbt, "http://198.51.100.5:8080");
        assert_eq!(c.escucha.port(), 9000);
    }

    #[test]
    fn una_linea_que_no_es_variable_para_y_no_ensena_el_valor() {
        let (_d, ruta) = escribir("SEMILLERO_PORT=9000\n# bien\nesto-es-secreto\n");
        let e = leer_fichero_env(&ruta).unwrap_err();
        assert!(e.contains(&ruta.display().to_string()), "{e}");
        assert!(e.contains("línea 3"), "{e}");
        assert!(!e.contains("esto-es-secreto"), "{e}");
        let (_d, ruta) = escribir("SEMILLERO PORT=9000\n");
        let e = leer_fichero_env(&ruta).unwrap_err();
        assert!(e.contains("línea 1"), "{e}");
        assert!(!e.contains("9000"), "{e}");
    }

    #[test]
    fn sin_fichero_o_demasiado_grande_lo_dice() {
        let dir = tempfile::tempdir().unwrap();
        let ruta = dir.path().join("no-esta.env");
        let e = leer_fichero_env(&ruta).unwrap_err();
        assert!(e.contains("no-esta.env"), "{e}");
        let (_d, ruta) = escribir(&format!("SEMILLERO_PORT={}\n", "9".repeat(70 * 1024)));
        let e = leer_fichero_env(&ruta).unwrap_err();
        assert!(e.contains("demasiado grande"), "{e}");
    }
}

// ---------------------------------------------------------------- contrato hasta la 2.0

mod contrato {
    use std::collections::BTreeSet;

    use semillero::config::{Config, RENOMBRADAS, VARIABLES, con_renombres};

    const AVISO: &str = "Las variables SEMILLERO_* son el contrato de configuración hasta la \
        2.0: no se quitan ni cambian de nombre. Si una cambia de nombre, el nombre viejo va \
        en config::RENOMBRADAS (sigue valiendo, con un aviso en el registro) durante al \
        menos una versión";

    /// La lista que no cambia hasta la 2.0.
    const CONTRATO: [&str; 13] = [
        "SEMILLERO_DB",
        "SEMILLERO_ESTADO",
        "SEMILLERO_JELLYFIN",
        "SEMILLERO_JELLYFIN_CLAVE",
        "SEMILLERO_LISTEN",
        "SEMILLERO_LOG",
        "SEMILLERO_LOGIN_DESDE",
        "SEMILLERO_ORIGEN",
        "SEMILLERO_PORT",
        "SEMILLERO_PROXY",
        "SEMILLERO_QBT",
        "SEMILLERO_QBT_CLAVE",
        "SEMILLERO_QBT_USUARIO",
    ];

    #[test]
    fn las_variables_son_las_del_contrato() {
        let hay: BTreeSet<&str> = VARIABLES.iter().copied().collect();
        assert_eq!(hay.len(), VARIABLES.len(), "variable repetida en VARIABLES");
        assert_eq!(hay, CONTRATO.into_iter().collect(), "{AVISO}");
    }

    #[test]
    fn los_nombres_viejos_no_son_variables_y_los_nuevos_si() {
        for (vieja, nueva) in RENOMBRADAS {
            assert!(!VARIABLES.contains(vieja), "{vieja}: {AVISO}");
            assert!(VARIABLES.contains(nueva), "{nueva}: {AVISO}");
        }
    }

    /// El servidor no lee ninguna `SEMILLERO_*` que no esté en la lista, y
    /// la lista no tiene ninguna que no lea (`SEMILLERO_LOG` la lee al
    /// arrancar, antes de la configuración).
    #[test]
    fn la_configuracion_lee_justo_las_variables_de_la_lista() {
        let pedidas = std::sync::Mutex::new(BTreeSet::new());
        let valor = |k: &str| match k {
            "SEMILLERO_PORT" => "8090",
            "SEMILLERO_LISTEN" => "127.0.0.1",
            "SEMILLERO_LOGIN_DESDE" | "SEMILLERO_PROXY" => "192.0.2.7",
            "SEMILLERO_QBT" => "http://192.0.2.20:8080",
            "SEMILLERO_JELLYFIN" => "http://192.0.2.30:8096",
            _ => "/x/y",
        };
        Config::desde(|k| {
            pedidas.lock().unwrap().insert(k.to_owned());
            k.starts_with("SEMILLERO_").then(|| valor(k).to_owned())
        })
        .unwrap();
        let pedidas: BTreeSet<String> = pedidas.into_inner().unwrap();
        let esperadas: BTreeSet<String> = VARIABLES
            .iter()
            .filter(|k| **k != "SEMILLERO_LOG")
            .map(|k| (*k).to_owned())
            .collect();
        assert_eq!(pedidas, esperadas, "{AVISO}");
    }

    /// Cada variable sale en la tabla de la configuración de los dos README.
    #[test]
    fn cada_variable_esta_documentada_en_los_readme() {
        let raiz = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for (readme, contrato) in [
            ("README.md", "contrato hasta la 2.0"),
            ("README.en.md", "contract until 2.0"),
        ] {
            let texto = std::fs::read_to_string(raiz.join(readme)).unwrap();
            for k in VARIABLES {
                assert!(
                    texto.lines().any(|l| l.starts_with(&format!("| `{k}` |"))),
                    "{k} no está en la tabla de {readme}"
                );
            }
            assert!(
                texto.contains(contrato),
                "{readme} no dice que la configuración es un contrato hasta la 2.0"
            );
        }
    }

    fn lector(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |k| vars.iter().find(|(c, _)| c == k).map(|(_, v)| v.clone())
    }

    /// Un cambio de nombre de prueba: hoy no hay ninguno.
    const PRUEBA: &[(&str, &str)] = &[("SEMILLERO_BASE", "SEMILLERO_DB")];

    #[test]
    fn hoy_no_cambia_de_nombre_ninguna_variable() {
        assert_eq!(RENOMBRADAS, &[] as &[(&str, &str)]);
    }

    #[test]
    fn con_solo_el_nombre_viejo_vale_y_avisa() {
        let (leer, avisos) = con_renombres(lector(&[("SEMILLERO_BASE", "/v/vieja.db")]), PRUEBA);
        let c = Config::desde(&leer).unwrap();
        assert_eq!(c.db, std::path::Path::new("/v/vieja.db"));
        assert_eq!(avisos.len(), 1, "{avisos:?}");
        assert!(avisos[0].contains("SEMILLERO_BASE"), "{avisos:?}");
        assert!(avisos[0].contains("SEMILLERO_DB"), "{avisos:?}");
        // El aviso nunca lleva el valor.
        assert!(!avisos[0].contains("/v/vieja.db"), "{avisos:?}");
    }

    #[test]
    fn con_los_dos_nombres_manda_el_nuevo_y_avisa_que_el_viejo_se_ignora() {
        let (leer, avisos) = con_renombres(
            lector(&[
                ("SEMILLERO_BASE", "/v/vieja.db"),
                ("SEMILLERO_DB", "/v/nueva.db"),
            ]),
            PRUEBA,
        );
        let c = Config::desde(&leer).unwrap();
        assert_eq!(c.db, std::path::Path::new("/v/nueva.db"));
        assert_eq!(avisos.len(), 1, "{avisos:?}");
        assert!(avisos[0].contains("SEMILLERO_BASE"), "{avisos:?}");
        assert!(avisos[0].contains("se ignora"), "{avisos:?}");
    }

    #[test]
    fn con_solo_el_nombre_nuevo_no_hay_aviso() {
        let (leer, avisos) = con_renombres(lector(&[("SEMILLERO_DB", "/v/nueva.db")]), PRUEBA);
        assert_eq!(
            Config::desde(&leer).unwrap().db,
            std::path::Path::new("/v/nueva.db")
        );
        assert_eq!(avisos, Vec::<String>::new());
    }

    /// Con la tabla de verdad, todo se lee como antes, sin avisos.
    #[test]
    fn con_la_tabla_de_verdad_se_lee_como_antes() {
        let vars = [
            ("SEMILLERO_LISTEN", "192.0.2.4"),
            ("SEMILLERO_PORT", "7000"),
            ("SEMILLERO_DB", " /var/db/semillero.db "),
            ("SEMILLERO_LOG", "debug"),
        ];
        let (leer, avisos) = con_renombres(lector(&vars), RENOMBRADAS);
        assert_eq!(avisos, Vec::<String>::new());
        assert_eq!(leer("SEMILLERO_LOG").as_deref(), Some("debug"));
        let c = Config::desde(&leer).unwrap();
        assert_eq!(c, Config::desde(lector(&vars)).unwrap());
        assert_eq!(c.escucha, "192.0.2.4:7000".parse().unwrap());
        assert_eq!(c.db, std::path::Path::new("/var/db/semillero.db"));
    }
}
