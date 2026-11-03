//! Servidor de semillero. La configuración, por variables de entorno o con
//! `--env FICHERO` (ver
//! `semillero::config`); `semillero configurar` la pregunta y la escribe
//! (ver `semillero::configurar`).

#[cfg(feature = "ssr")]
const AYUDA: &str = "\
Uso:
  semillero               arranca el servidor (configuración en variables de entorno)
  semillero --env FICHERO arranca el servidor con la configuración de FICHERO
                          (el que escribe semillero configurar); las variables
                          de entorno que ya estén puestas mandan sobre él
  semillero configurar    pregunta las direcciones y las claves y las guarda
                          (semillero configurar --help para sus opciones)
  semillero --version     dice la versión
";

#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() -> std::process::ExitCode {
    use std::net::SocketAddr;
    use std::process::ExitCode;

    use leptos::config::get_configuration;
    use semillero::{auth, config::Config, construir_router, estado::AppState};

    let mut args = std::env::args_os().skip(1);
    let mut fichero = None;
    match args.next() {
        None => {}
        Some(a) if a == "--env" => match (args.next(), args.next()) {
            (Some(f), None) => fichero = Some(std::path::PathBuf::from(f)),
            _ => {
                eprintln!("semillero: uso: semillero --env FICHERO\n\n{AYUDA}");
                return ExitCode::from(2);
            }
        },
        Some(a) if a == "configurar" => {
            return semillero::configurar::ejecutar(args.collect()).await;
        }
        Some(a) if a == "--version" || a == "-V" => {
            println!("semillero {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Some(a) if a == "--help" || a == "-h" => {
            print!("{AYUDA}");
            return ExitCode::SUCCESS;
        }
        Some(a) => {
            eprintln!("semillero: no conozco «{}»\n\n{AYUDA}", a.to_string_lossy());
            return ExitCode::from(2);
        }
    }
    let fichero = match fichero.as_deref().map(semillero::config::leer_fichero_env) {
        None => Default::default(),
        Some(Ok(f)) => f,
        Some(Err(e)) => {
            eprintln!("semillero: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (leer, avisos) = semillero::config::con_renombres(
        semillero::config::con_fichero(|k| std::env::var(k).ok(), fichero),
        semillero::config::RENOMBRADAS,
    );
    iniciar_registro(leer("SEMILLERO_LOG"));
    for aviso in &avisos {
        tracing::warn!("{aviso}");
    }
    let cfg = match Config::desde(&leer) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("semillero: configuración no válida: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut leptos = match get_configuration(None) {
        Ok(conf) => conf.leptos_options,
        Err(e) => {
            eprintln!("semillero: configuración de Leptos no válida: {e}");
            return ExitCode::FAILURE;
        }
    };
    // `SEMILLERO_LISTEN` manda sobre `site-addr` de cargo-leptos, y la web
    // está en `site` junto al ejecutable salvo que diga otra cosa
    // `LEPTOS_SITE_ROOT`.
    leptos.site_addr = cfg.escucha;
    let exe = std::env::current_exe().ok();
    let raiz = semillero::config::raiz_web(&leer, exe.as_deref());
    leptos.site_root = raiz.to_string_lossy().as_ref().into();
    configurar_hash(&mut leptos);
    let escucha = cfg.escucha;
    let mediateca = match semillero::mediateca::desde_config(cfg.jellyfin.as_ref()) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("semillero: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut state = match AppState::nuevo(cfg, leptos).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("semillero: {e}");
            return ExitCode::FAILURE;
        }
    };
    state.mediateca = mediateca;
    let oyente = match tokio::net::TcpListener::bind(escucha).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("semillero: no se puede escuchar en {escucha}: {e}");
            return ExitCode::FAILURE;
        }
    };
    // Canario del acceso, una vez por arranque y en segundo plano (si
    // qBittorrent no contesta, se repite en el siguiente intento de entrar).
    let canario = state.clone();
    tokio::spawn(async move {
        auth::comprobar_canario(&canario).await;
    });
    // Tarea horaria: pausa, avisos y borrados.
    tokio::spawn(semillero::tarea::vigilar(
        state.clone(),
        state.mediateca.clone(),
    ));
    eprintln!("semillero escuchando en http://{escucha}");
    let app = construir_router(state).into_make_service_with_connect_info::<SocketAddr>();
    if let Err(e) = axum::serve(oyente, app)
        .with_graceful_shutdown(al_parar())
        .await
    {
        eprintln!("semillero: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// Espera a SIGTERM (`service stop`, systemd, podman) o Ctrl+C. Sin esto,
/// como PID 1 en un contenedor el proceso no atendía SIGTERM.
#[cfg(feature = "ssr")]
async fn al_parar() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = term => {}
    }
    eprintln!("semillero: parando");
}

/// Registros a stderr; nivel según `SEMILLERO_LOG`, por defecto `info`.
#[cfg(feature = "ssr")]
fn iniciar_registro(nivel: Option<String>) {
    use tracing_subscriber::EnvFilter;
    let filtro = nivel
        .and_then(|n| EnvFilter::try_new(n).ok())
        .unwrap_or_else(|| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filtro)
        .with_writer(std::io::stderr)
        .init();
}

/// Usa los nombres con hash de cargo-leptos si la web trae `hash.txt`.
#[cfg(feature = "ssr")]
fn configurar_hash(opciones: &mut leptos::prelude::LeptosOptions) {
    let ruta = std::path::Path::new(opciones.site_root.as_ref()).join("hash.txt");
    let ruta = std::path::absolute(&ruta).unwrap_or(ruta);
    opciones.hash_files = ruta.is_file();
    opciones.hash_file = ruta.to_string_lossy().as_ref().into();
}

#[cfg(not(feature = "ssr"))]
fn main() {}
