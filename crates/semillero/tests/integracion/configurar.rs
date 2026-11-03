//! `semillero configurar`, por su interfaz pública: el binario, con las
//! respuestas por la entrada estándar (sin terminal), contra `qbt-sim` y un
//! Jellyfin falso. Todo en directorios temporales fuera de la copia de
//! trabajo.
//!
//! Orden de las preguntas: dónde escucha, dirección en el navegador,
//! qBittorrent, modo (1 con contraseña, 2 lista blanca), [usuario y
//! contraseña], proxy, Jellyfin, [su clave], fichero de estado, base de
//! datos; y, si alguna comprobación falla, si se guarda de todos modos.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use axum::Router;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::get;
use semillero::config::Config;

const CLAVE_QBT: &str = "Qbt-Secreto-7731";
const CLAVE_JF: &str = "jf0123456789abcdef0123456789abcd";

fn sim_conf() -> qbt_sim::Conf {
    qbt_sim::Conf {
        clave: CLAVE_QBT.into(),
        ..Default::default()
    }
}

/// Jellyfin falso que solo acepta [`CLAVE_JF`].
async fn jellyfin_falso() -> String {
    let app = Router::new().route(
        "/System/Info",
        get(|h: HeaderMap| async move {
            let bien = format!("MediaBrowser Token=\"{CLAVE_JF}\"");
            if h.get("authorization").and_then(|a| a.to_str().ok()) == Some(bien.as_str()) {
                StatusCode::OK
            } else {
                StatusCode::UNAUTHORIZED
            }
        }),
    );
    let oyente = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir = oyente.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(oyente, app).await.unwrap() });
    format!("http://{dir}")
}

struct Salida {
    exito: bool,
    texto: String,
}

/// Ejecuta `semillero configurar --env <dir>/semillero.env` con `lineas`
/// por la entrada estándar y un entorno limpio (más `entorno`).
async fn configurar(dir: &Path, lineas: &[&str], entorno: &[(&str, &str)]) -> Salida {
    let entrada: String = lineas.iter().map(|l| format!("{l}\n")).collect();
    let env = dir.join("semillero.env");
    let entorno: Vec<(String, String)> = entorno
        .iter()
        .map(|(k, v)| ((*k).into(), (*v).into()))
        .collect();
    let dir = dir.to_owned();
    let o: Output = tokio::task::spawn_blocking(move || {
        use std::io::Write;
        let mut hijo = Command::new(env!("CARGO_BIN_EXE_semillero"))
            .arg("configurar")
            .arg("--env")
            .arg(&env)
            .current_dir(&dir)
            .env_clear()
            .envs(de_windows())
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .envs(entorno)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // Si termina antes de leerlo todo, la escritura puede fallar.
        let _ = hijo.stdin.take().unwrap().write_all(entrada.as_bytes());
        hijo.wait_with_output().unwrap()
    })
    .await
    .unwrap();
    let texto = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    Salida {
        exito: o.status.success(),
        texto,
    }
}

fn leer(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

/// Las variables del fichero, como las lee el servidor.
fn config_de(env: &Path) -> Config {
    let texto = leer(env);
    let vars: Vec<(String, String)> = texto
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect();
    Config::desde(move |k| vars.iter().find(|(c, _)| c == k).map(|(_, v)| v.clone())).unwrap()
}

fn sin_secretos(s: &Salida, env: &Path) {
    for secreto in [CLAVE_QBT, CLAVE_JF] {
        assert!(
            !s.texto.contains(secreto),
            "secreto en la salida: {}",
            s.texto
        );
        assert!(!leer(env).contains(secreto), "secreto en el .env");
    }
}

#[cfg(unix)]
fn modo(p: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).unwrap().permissions().mode() & 0o777
}

/// Todo de una vez, con contraseña y Jellyfin.
async fn completo(dir: &Path, qbt: &str, jf: &str) -> Salida {
    configurar(
        dir,
        &[
            "127.0.0.1:8091",
            "https://semillero.example",
            qbt,
            "1",
            "",
            CLAVE_QBT,
            "192.0.2.99",
            jf,
            CLAVE_JF,
            "",
            "",
        ],
        &[],
    )
    .await
}

#[tokio::test]
async fn con_contrasena_y_jellyfin_escribe_el_env_y_los_secretos_aparte() {
    let sim = qbt_sim::arrancar(sim_conf()).await;
    let jf = jellyfin_falso().await;
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join("semillero.env");
    let s = completo(dir.path(), &sim.url, &jf).await;
    assert!(s.exito, "{}", s.texto);
    assert!(s.texto.contains(&env.display().to_string()), "{}", s.texto);
    sin_secretos(&s, &env);

    let texto = leer(&env);
    let qbt_clave = dir.path().join("qbt.clave");
    let jf_clave = dir.path().join("jellyfin.clave");
    for linea in [
        "SEMILLERO_LISTEN=127.0.0.1:8091".to_owned(),
        "SEMILLERO_ORIGEN=https://semillero.example".to_owned(),
        format!("SEMILLERO_QBT={}", sim.url),
        format!("SEMILLERO_QBT_CLAVE={}", qbt_clave.display()),
        "SEMILLERO_PROXY=192.0.2.99".to_owned(),
        format!("SEMILLERO_JELLYFIN={jf}"),
        format!("SEMILLERO_JELLYFIN_CLAVE={}", jf_clave.display()),
    ] {
        assert!(texto.lines().any(|l| l == linea), "falta {linea}:\n{texto}");
    }
    // Lo que se deja en su valor por defecto no se escribe.
    for k in [
        "SEMILLERO_DB=",
        "SEMILLERO_ESTADO=",
        "SEMILLERO_QBT_USUARIO=",
    ] {
        assert!(!texto.contains(k), "{k} sin pedirlo:\n{texto}");
    }

    // El servidor lo entiende tal cual.
    let c = config_de(&env);
    assert_eq!(c.escucha, "127.0.0.1:8091".parse().unwrap());
    assert_eq!(c.origenes, vec!["https://semillero.example".to_owned()]);
    let cuenta = c.cuenta_qbt.unwrap();
    assert_eq!(cuenta.usuario, "admin");
    assert_eq!(
        semillero::config::leer_contrasena_qbt(&cuenta).unwrap(),
        CLAVE_QBT
    );
    assert!(semillero::mediateca::desde_config(c.jellyfin.as_ref()).is_ok());

    #[cfg(unix)]
    {
        assert_eq!(modo(&qbt_clave), 0o600);
        assert_eq!(modo(&jf_clave), 0o600);
    }
    // Sin restos de ficheros temporales.
    let mut nombres: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    nombres.sort();
    assert_eq!(
        nombres,
        ["jellyfin.clave", "qbt.clave", "semillero.env"],
        "{nombres:?}"
    );
}

#[tokio::test]
async fn otra_vez_solo_con_intro_no_cambia_nada() {
    let sim = qbt_sim::arrancar(sim_conf()).await;
    let jf = jellyfin_falso().await;
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join("semillero.env");
    assert!(completo(dir.path(), &sim.url, &jf).await.exito);
    // Un comentario y una variable que no pregunta: se conservan.
    let mut texto = leer(&env);
    texto.push_str("# mío\nSEMILLERO_LOG=debug\n");
    std::fs::write(&env, &texto).unwrap();
    let qbt_antes = leer(&dir.path().join("qbt.clave"));
    let jf_antes = leer(&dir.path().join("jellyfin.clave"));

    let s = configurar(dir.path(), &[""; 11], &[]).await;
    assert!(s.exito, "{}", s.texto);
    sin_secretos(&s, &env);
    assert_eq!(leer(&env), texto);
    assert_eq!(leer(&dir.path().join("qbt.clave")), qbt_antes);
    assert_eq!(leer(&dir.path().join("jellyfin.clave")), jf_antes);
    // Las propuestas son los valores que había.
    assert!(s.texto.contains("[127.0.0.1:8091]"), "{}", s.texto);
    assert!(
        s.texto.contains("[https://semillero.example]"),
        "{}",
        s.texto
    );
}

#[tokio::test]
async fn cambia_solo_lo_que_se_cambia() {
    let sim = qbt_sim::arrancar(sim_conf()).await;
    let jf = jellyfin_falso().await;
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join("semillero.env");
    assert!(completo(dir.path(), &sim.url, &jf).await.exito);
    let antes = leer(&env);
    // Otro proxy; Jellyfin fuera con «-».
    let s = configurar(
        dir.path(),
        &["", "", "", "", "", "", "192.0.2.98", "-", "", ""],
        &[],
    )
    .await;
    assert!(s.exito, "{}", s.texto);
    let despues = leer(&env);
    let esperado: String = antes
        .lines()
        .filter(|l| !l.starts_with("SEMILLERO_JELLYFIN"))
        .map(|l| {
            if l.starts_with("SEMILLERO_PROXY=") {
                "SEMILLERO_PROXY=192.0.2.98\n".to_owned()
            } else {
                format!("{l}\n")
            }
        })
        .collect();
    assert_eq!(despues, esperado);
}

#[tokio::test]
async fn lo_que_no_vale_se_vuelve_a_preguntar() {
    let sim = qbt_sim::arrancar(sim_conf()).await;
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join("semillero.env");
    let s = configurar(
        dir.path(),
        &[
            "no es una dirección",
            "127.0.0.1:8090",
            "http://192.0.2.5:8090",
            "https://semillero.example",
            "ftp://qbittorrent",
            &sim.url,
            "3",
            "1",
            "",
            "",
            CLAVE_QBT,
            "no-es-una-ip",
            "",
            "",
            "",
            "",
        ],
        &[],
    )
    .await;
    assert!(s.exito, "{}", s.texto);
    for aviso in [
        "no es una dirección IP:puerto",
        "https",
        "http:// o https://",
        "1 o 2",
        "Hace falta la contraseña",
        "no es una IP",
    ] {
        assert!(
            s.texto.contains(aviso),
            "falta el aviso «{aviso}»:\n{}",
            s.texto
        );
    }
    sin_secretos(&s, &env);
    let texto = leer(&env);
    assert!(texto.contains("SEMILLERO_ORIGEN=https://semillero.example\n"));
    assert!(!texto.contains("192.0.2.5"));
    // El origen http de este equipo sí vale.
    let dir2 = tempfile::tempdir().unwrap();
    let s = configurar(
        dir2.path(),
        &[
            "",
            "http://localhost:8090,http://[::1]:8090",
            &sim.url,
            "1",
            "",
            CLAVE_QBT,
            "",
            "",
            "",
            "",
        ],
        &[],
    )
    .await;
    assert!(s.exito, "{}", s.texto);
}

#[tokio::test]
async fn si_qbittorrent_no_acepta_lo_dice_y_pregunta_si_guardar() {
    let sim = qbt_sim::arrancar(sim_conf()).await;
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join("semillero.env");
    let respuestas = |guardar: &'static str| {
        vec![
            "".to_owned(),
            "".to_owned(),
            sim.url.clone(),
            "1".to_owned(),
            "".to_owned(),
            "otra-mala".to_owned(),
            "".to_owned(),
            "".to_owned(),
            "".to_owned(),
            "".to_owned(),
            guardar.to_owned(),
        ]
    };
    let r = respuestas("n");
    let r: Vec<&str> = r.iter().map(String::as_str).collect();
    let s = configurar(dir.path(), &r, &[]).await;
    assert!(!s.exito, "{}", s.texto);
    assert!(
        s.texto
            .contains("qBittorrent no acepta ese usuario y contraseña"),
        "{}",
        s.texto
    );
    assert!(!s.texto.contains("otra-mala"), "{}", s.texto);
    assert!(!env.exists());
    assert!(!dir.path().join("qbt.clave").exists());

    let r = respuestas("s");
    let r: Vec<&str> = r.iter().map(String::as_str).collect();
    let s = configurar(dir.path(), &r, &[]).await;
    assert!(s.exito, "{}", s.texto);
    assert!(env.exists() && dir.path().join("qbt.clave").exists());

    // qBittorrent apagado y Jellyfin con otra clave: lo dice también.
    let dir = tempfile::tempdir().unwrap();
    let jf = jellyfin_falso().await;
    let s = configurar(
        dir.path(),
        &[
            "",
            "",
            "http://127.0.0.1:1",
            "1",
            "",
            CLAVE_QBT,
            "",
            &jf,
            "otra-clave-de-jellyfin",
            "",
            "",
            "n",
        ],
        &[],
    )
    .await;
    assert!(!s.exito);
    assert!(s.texto.contains("qBittorrent no responde"), "{}", s.texto);
    assert!(
        s.texto.contains("Jellyfin no acepta la clave de la API"),
        "{}",
        s.texto
    );
    assert!(!s.texto.contains("otra-clave-de-jellyfin"));
}

#[tokio::test]
async fn lista_blanca_comprueba_que_la_api_contesta_y_quita_la_contrasena() {
    let sim = qbt_sim::arrancar(sim_conf()).await;
    let jf = jellyfin_falso().await;
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join("semillero.env");
    assert!(completo(dir.path(), &sim.url, &jf).await.exito);
    // Sin lista blanca, la API pide sesión: lo dice.
    let s = configurar(dir.path(), &["", "", "", "2", "", "", "", "", "", "n"], &[]).await;
    assert!(!s.exito);
    assert!(s.texto.contains("lista blanca"), "{}", s.texto);
    assert!(leer(&env).contains("SEMILLERO_QBT_CLAVE="));
    // Con la API abierta, vale y quita la contraseña del .env.
    let abierto = qbt_sim::arrancar(qbt_sim::Conf {
        api_abierta: true,
        ..sim_conf()
    })
    .await;
    let s = configurar(
        dir.path(),
        &["", "", &abierto.url, "2", "", "", "", "", ""],
        &[],
    )
    .await;
    assert!(s.exito, "{}", s.texto);
    let texto = leer(&env);
    assert!(!texto.contains("SEMILLERO_QBT_CLAVE"), "{texto}");
    assert_eq!(config_de(&env).cuenta_qbt, None);
}

/// En el contenedor, la imagen fija SEMILLERO_LISTEN y SEMILLERO_DB: no se
/// preguntan ni se escriben (en el .env mandarían sobre la imagen).
#[tokio::test]
async fn lo_que_fija_el_entorno_no_se_pregunta() {
    let sim = qbt_sim::arrancar(sim_conf()).await;
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join("semillero.env");
    let s = configurar(
        dir.path(),
        // Sin la pregunta de dónde escucha ni la de la base de datos.
        &["", &sim.url, "1", "", CLAVE_QBT, "", "", ""],
        &[
            ("SEMILLERO_LISTEN", "0.0.0.0:8090"),
            ("SEMILLERO_DB", "/data/semillero.db"),
        ],
    )
    .await;
    assert!(s.exito, "{}", s.texto);
    assert!(s.texto.contains("SEMILLERO_LISTEN"), "{}", s.texto);
    let texto = leer(&env);
    assert!(!texto.contains("SEMILLERO_LISTEN"), "{texto}");
    assert!(!texto.contains("SEMILLERO_DB"), "{texto}");
}

#[tokio::test]
async fn los_secretos_van_a_la_carpeta_de_secretos() {
    let sim = qbt_sim::arrancar(sim_conf()).await;
    let dir = tempfile::tempdir().unwrap();
    let secretos: PathBuf = dir.path().join("datos");
    let env = dir.path().join("semillero.env");
    let entrada = format!("\n\n{}\n1\n\n{CLAVE_QBT}\n\n\n\n\n", sim.url);
    let o = tokio::task::spawn_blocking({
        let env = env.clone();
        let secretos = secretos.clone();
        move || {
            use std::io::Write;
            let mut hijo = Command::new(env!("CARGO_BIN_EXE_semillero"))
                .args(["configurar", "--env"])
                .arg(&env)
                .arg("--secretos")
                .arg(&secretos)
                .env_clear()
                .envs(de_windows())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let _ = hijo.stdin.take().unwrap().write_all(entrada.as_bytes());
            hijo.wait_with_output().unwrap()
        }
    })
    .await
    .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let clave = secretos.join("qbt.clave");
    assert!(leer(&env).contains(&format!("SEMILLERO_QBT_CLAVE={}", clave.display())));
    #[cfg(unix)]
    {
        assert_eq!(modo(&clave), 0o600);
        assert_eq!(modo(&secretos), 0o700);
    }
}

/// Lo que un proceso de Windows necesita aunque se le vacíe el entorno: sin
/// `SystemRoot` no puede abrir conexiones (WSAStartup falla). En Unix no hay.
fn de_windows() -> Vec<(&'static str, std::ffi::OsString)> {
    std::env::var_os("SystemRoot")
        .map(|v| ("SystemRoot", v))
        .into_iter()
        .collect()
}

#[tokio::test]
async fn sin_configurar_sigue_siendo_el_servidor_y_una_orden_rara_falla() {
    let dir = tempfile::tempdir().unwrap();
    let o = tokio::task::spawn_blocking({
        let dir = dir.path().to_owned();
        move || {
            Command::new(env!("CARGO_BIN_EXE_semillero"))
                .args(["configurar", "--no-existe"])
                .current_dir(dir)
                .env_clear()
                .output()
                .unwrap()
        }
    })
    .await
    .unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("semillero configurar [--env FICHERO]"));
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

/// La ruta que pone `k` en el fichero de configuración.
fn ruta_en(env: &Path, k: &str) -> PathBuf {
    leer(env)
        .lines()
        .find_map(|l| l.strip_prefix(&format!("{k}=")).map(PathBuf::from))
        .unwrap()
}

#[tokio::test]
async fn al_quitar_jellyfin_ofrece_borrar_su_clave_y_por_defecto_la_deja() {
    let sim = qbt_sim::arrancar(sim_conf()).await;
    let jf = jellyfin_falso().await;
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join("semillero.env");
    assert!(completo(dir.path(), &sim.url, &jf).await.exito);
    let clave = ruta_en(&env, "SEMILLERO_JELLYFIN_CLAVE");
    assert!(clave.exists());
    // Jellyfin fuera con «-»; a la pregunta de borrar, Intro: se queda.
    let s = configurar(
        dir.path(),
        &["", "", "", "", "", "", "", "-", "", "", ""],
        &[],
    )
    .await;
    assert!(s.exito, "{}", s.texto);
    assert!(
        s.texto
            .contains(&format!("¿Borrar el fichero antiguo {}?", clave.display())),
        "{}",
        s.texto
    );
    assert!(clave.exists(), "sin «s» no se borra nada");
    assert!(!leer(&env).contains("SEMILLERO_JELLYFIN"));
    sin_secretos(&s, &env);
}

#[tokio::test]
async fn al_quitar_jellyfin_con_s_borra_su_clave() {
    let sim = qbt_sim::arrancar(sim_conf()).await;
    let jf = jellyfin_falso().await;
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join("semillero.env");
    assert!(completo(dir.path(), &sim.url, &jf).await.exito);
    let clave = ruta_en(&env, "SEMILLERO_JELLYFIN_CLAVE");
    let qbt = ruta_en(&env, "SEMILLERO_QBT_CLAVE");
    let s = configurar(
        dir.path(),
        &["", "", "", "", "", "", "", "-", "", "", "s"],
        &[],
    )
    .await;
    assert!(s.exito, "{}", s.texto);
    assert!(!clave.exists(), "{}", s.texto);
    // La de qBittorrent sigue en uso: ni se pregunta.
    assert!(qbt.exists());
    assert!(!s.texto.contains(&qbt.display().to_string()), "{}", s.texto);
    sin_secretos(&s, &env);
}

#[tokio::test]
async fn al_pasar_a_la_lista_blanca_ofrece_borrar_la_contrasena() {
    let sim = qbt_sim::arrancar(sim_conf()).await;
    let jf = jellyfin_falso().await;
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join("semillero.env");
    assert!(completo(dir.path(), &sim.url, &jf).await.exito);
    let qbt = ruta_en(&env, "SEMILLERO_QBT_CLAVE");
    let abierto = qbt_sim::arrancar(qbt_sim::Conf {
        api_abierta: true,
        ..sim_conf()
    })
    .await;
    let s = configurar(
        dir.path(),
        &["", "", &abierto.url, "2", "", "", "", "", "", "s"],
        &[],
    )
    .await;
    assert!(s.exito, "{}", s.texto);
    assert!(
        s.texto
            .contains(&format!("¿Borrar el fichero antiguo {}?", qbt.display())),
        "{}",
        s.texto
    );
    assert!(!qbt.exists());
    assert!(!leer(&env).contains("SEMILLERO_QBT_CLAVE"));
    sin_secretos(&s, &env);
}

/// El formato del fichero que escribe `semillero configurar` es parte del
/// contrato de configuración hasta la 2.0: con las mismas respuestas, los
/// mismos bytes. Si esta prueba falla, el cambio rompe los ficheros ya
/// escritos; un nombre que cambia va por `config::RENOMBRADAS`.
#[tokio::test]
async fn el_fichero_que_escribe_es_el_del_contrato() {
    let sim = qbt_sim::arrancar(qbt_sim::Conf {
        usuario: "semillero".into(),
        ..sim_conf()
    })
    .await;
    let jf = jellyfin_falso().await;
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join("semillero.env");
    let s = configurar(
        dir.path(),
        &[
            "192.0.2.10:8091",
            "https://semillero.example",
            &sim.url,
            "1",
            "semillero",
            CLAVE_QBT,
            "192.0.2.99",
            &jf,
            CLAVE_JF,
            "/run/semillero/estado",
            "/var/db/semillero/semillero.db",
        ],
        &[],
    )
    .await;
    assert!(s.exito, "{}", s.texto);
    let qbt_clave = dir.path().join("qbt.clave");
    let jf_clave = dir.path().join("jellyfin.clave");
    let esperado = format!(
        "# Configuración de semillero, escrita por «semillero configurar» (vuelve a\n\
         # ejecutarlo para cambiarla). Una variable por línea, VARIABLE=valor, sin\n\
         # comillas; la lista está en el README. La contraseña de qBittorrent y la\n\
         # clave de Jellyfin no van aquí: cada una en su fichero, solo para su dueño.\n\
         \n\
         SEMILLERO_LISTEN=192.0.2.10:8091\n\
         SEMILLERO_ORIGEN=https://semillero.example\n\
         SEMILLERO_QBT={qbt}\n\
         SEMILLERO_QBT_USUARIO=semillero\n\
         SEMILLERO_QBT_CLAVE={qbt_clave}\n\
         SEMILLERO_PROXY=192.0.2.99\n\
         SEMILLERO_JELLYFIN={jf}\n\
         SEMILLERO_JELLYFIN_CLAVE={jf_clave}\n\
         SEMILLERO_ESTADO=/run/semillero/estado\n\
         SEMILLERO_DB=/var/db/semillero/semillero.db\n",
        qbt = sim.url,
        qbt_clave = qbt_clave.display(),
        jf_clave = jf_clave.display(),
    );
    assert_eq!(
        std::fs::read(&env).unwrap(),
        esperado.into_bytes(),
        "el formato del fichero de semillero configurar es el contrato de configuración \
         hasta la 2.0:\n{}",
        leer(&env)
    );
    sin_secretos(&s, &env);
}
