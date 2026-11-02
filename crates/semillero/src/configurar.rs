//! `semillero configurar`: pregunta en la terminal las direcciones y las
//! claves y escribe el fichero `VARIABLE=valor` que leen todas las formas de
//! instalar (systemd y podman con `EnvironmentFile`, `semillero.cmd` en
//! Windows y `semillero_env_file` en el rc.d de FreeBSD).
//!
//! - Propone el valor que ya hay en el fichero (o el de por defecto) e Intro
//!   lo deja; lo que se deja en su valor por defecto no se escribe. El resto
//!   del fichero (comentarios, otras variables) se queda como está.
//! - La contraseña de qBittorrent y la clave de Jellyfin van cada una en su
//!   fichero, solo para su dueño (0600 en Unix), escrito de golpe (fichero
//!   temporal y `rename`). Nunca se muestran ni van al fichero de
//!   configuración; en una terminal se leen sin eco.
//! - Lo que ya fija el entorno (en el contenedor, `SEMILLERO_LISTEN` y
//!   `SEMILLERO_DB` de la imagen) no se pregunta.
//! - Antes de guardar comprueba qBittorrent y Jellyfin con esos valores; si
//!   algo falla, lo dice y pregunta si guardar de todos modos.
//!
//! Sin terminal, las respuestas se leen línea a línea de la entrada
//! estándar (las contraseñas también), para las pruebas y la automatización.

use std::ffi::OsString;
use std::io::{self, BufRead, IsTerminal, Write};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use qbt::{Cuenta, Qbt};

use crate::config::{Config, es_local, leer_secreto};

const USO: &str = "uso: semillero configurar [--env FICHERO] [--secretos CARPETA]

  --env FICHERO       fichero de configuración (por defecto, semillero.env
                      en la carpeta actual)
  --secretos CARPETA  dónde guardar la contraseña de qBittorrent y la clave
                      de Jellyfin (por defecto, la carpeta del fichero)";

/// Más que esto no es una contraseña ni una clave de API.
const MAX_SECRETO: usize = 1024;

/// Ejecuta `semillero configurar` con `args` (lo que va detrás).
pub async fn ejecutar(args: Vec<OsString>) -> ExitCode {
    let op = match Opciones::leer(args) {
        Ok(Some(op)) => op,
        Ok(None) => {
            println!("{USO}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("semillero configurar: {e}\n{USO}");
            return ExitCode::from(2);
        }
    };
    match configurar(&op).await {
        Ok(codigo) => codigo,
        Err(e) => {
            eprintln!("semillero configurar: {e}");
            ExitCode::FAILURE
        }
    }
}

struct Opciones {
    env: PathBuf,
    secretos: PathBuf,
}

impl Opciones {
    /// `None`: se pidió la ayuda.
    fn leer(args: Vec<OsString>) -> Result<Option<Self>, String> {
        let mut env = None;
        let mut secretos = None;
        let mut args = args.into_iter();
        while let Some(a) = args.next() {
            let destino = match a.to_str() {
                Some("-h" | "--help" | "--ayuda") => return Ok(None),
                Some("--env") => &mut env,
                Some("--secretos") => &mut secretos,
                _ => return Err(format!("opción desconocida: {}", a.to_string_lossy())),
            };
            let valor = args
                .next()
                .ok_or_else(|| format!("falta el valor de {}", a.to_string_lossy()))?;
            *destino = Some(PathBuf::from(valor));
        }
        let env = absoluta(env.unwrap_or_else(|| PathBuf::from("semillero.env")))?;
        let secretos = match secretos {
            Some(s) => absoluta(s)?,
            None => env.parent().map(Path::to_owned).unwrap_or_default(),
        };
        Ok(Some(Self { env, secretos }))
    }
}

fn absoluta(p: PathBuf) -> Result<PathBuf, String> {
    std::path::absolute(&p).map_err(|e| format!("ruta no válida {}: {e}", p.display()))
}

// ---------------------------------------------------------------- entrada

/// La entrada se acabó antes de terminar.
struct SinEntrada;

impl From<SinEntrada> for String {
    fn from(_: SinEntrada) -> Self {
        "la entrada se acabó antes de terminar: no se ha guardado nada".into()
    }
}

struct Entrada {
    terminal: bool,
}

impl Entrada {
    fn linea(&mut self, pregunta: &str) -> Result<String, SinEntrada> {
        print!("{pregunta}: ");
        let _ = io::stdout().flush();
        let mut l = String::new();
        match io::stdin().lock().read_line(&mut l) {
            Ok(0) | Err(_) => {
                println!();
                Err(SinEntrada)
            }
            Ok(_) => {
                if !self.terminal {
                    println!();
                }
                Ok(l.trim_end_matches(['\n', '\r']).to_owned())
            }
        }
    }

    /// Como [`Entrada::linea`], sin eco en una terminal. Lo escrito nunca
    /// se muestra.
    fn secreta(&mut self, pregunta: &str) -> Result<String, SinEntrada> {
        if self.terminal {
            rpassword::prompt_password(format!("{pregunta}: ")).map_err(|_| SinEntrada)
        } else {
            self.linea(pregunta)
        }
    }

    fn si(&mut self, pregunta: &str) -> Result<bool, SinEntrada> {
        let r = self.linea(&format!("{pregunta} (s/N)"))?;
        Ok(matches!(
            r.trim().to_lowercase().as_str(),
            "s" | "si" | "sí" | "y" | "yes"
        ))
    }
}

enum Respuesta {
    /// Intro: se queda lo que hay.
    Igual,
    /// «-»: se quita (solo en lo opcional).
    Quitar,
    Valor(String),
}

/// Pregunta hasta que la respuesta valga. `propuesta` se enseña entre
/// corchetes; `validar` da el valor que se guarda o por qué no vale.
fn pedir(
    e: &mut Entrada,
    pregunta: &str,
    propuesta: Option<&str>,
    opcional: bool,
    validar: impl Fn(&str) -> Result<String, String>,
) -> Result<Respuesta, SinEntrada> {
    let texto = match propuesta {
        Some(p) if opcional => format!("{pregunta} («-» para quitarlo) [{p}]"),
        Some(p) => format!("{pregunta} [{p}]"),
        None if opcional => format!("{pregunta} (Intro: ninguno)"),
        None => pregunta.to_owned(),
    };
    loop {
        let r = e.linea(&texto)?;
        let r = r.trim();
        if r.is_empty() {
            return Ok(Respuesta::Igual);
        }
        if opcional && r == "-" {
            return Ok(Respuesta::Quitar);
        }
        match validar(r) {
            Ok(v) => return Ok(Respuesta::Valor(v)),
            Err(m) => println!("  {m}"),
        }
    }
}

// ---------------------------------------------------------------- validación

fn validar_escucha(v: &str) -> Result<String, String> {
    v.parse::<SocketAddr>().map(|a| a.to_string()).map_err(|_| {
        format!("«{v}» no es una dirección IP:puerto (por ejemplo 127.0.0.1:8090 o [::1]:8090)")
    })
}

/// Parte `http(s)://máquina[:puerto][/]` en el esquema y la máquina con el
/// puerto; sin ruta, sin espacios.
fn partir_url(v: &str) -> Option<(&str, &str)> {
    let (esquema, resto) = v.split_once("://")?;
    let autoridad = resto.strip_suffix('/').unwrap_or(resto);
    let valida = !autoridad.is_empty()
        && !autoridad.contains(['/', '?', '#', '@'])
        && !autoridad
            .chars()
            .any(|c| c.is_whitespace() || c.is_control());
    (matches!(esquema, "http" | "https") && valida).then_some((esquema, autoridad))
}

fn validar_url(v: &str, ejemplo: &str) -> Result<String, String> {
    match partir_url(v) {
        Some((esquema, autoridad)) => Ok(format!("{esquema}://{autoridad}")),
        None => Err(format!(
            "«{v}» tiene que ser http:// o https:// seguido de la máquina y el puerto, sin \
             ruta (por ejemplo {ejemplo})"
        )),
    }
}

fn validar_origenes(v: &str) -> Result<String, String> {
    let mut lista = Vec::new();
    for o in v.split(',').map(str::trim).filter(|o| !o.is_empty()) {
        let Some((esquema, autoridad)) = partir_url(o) else {
            return Err(format!(
                "«{o}» tiene que ser https://máquina (o http:// solo para este equipo), sin ruta"
            ));
        };
        if esquema == "http" && !es_local(o) {
            return Err(format!(
                "«{o}» va por http y no es este equipo: el navegador no guardaría la cookie \
                 de sesión. Desde otro equipo, usa https:// (con un proxy inverso); por http \
                 solo valen 127.0.0.1, [::1] y localhost"
            ));
        }
        lista.push(format!("{esquema}://{autoridad}"));
    }
    if lista.is_empty() {
        return Err("hace falta al menos una dirección".into());
    }
    Ok(lista.join(","))
}

fn validar_ip(v: &str) -> Result<String, String> {
    v.parse::<IpAddr>()
        .map(|ip| ip.to_string())
        .map_err(|_| format!("«{v}» no es una IP (por ejemplo 192.0.2.1 o 2001:db8::1)"))
}

fn validar_texto(v: &str) -> Result<String, String> {
    if v.chars().any(char::is_control) {
        return Err("no puede llevar caracteres de control".into());
    }
    Ok(v.to_owned())
}

fn validar_secreto(v: &str) -> Result<(), String> {
    if v.len() > MAX_SECRETO {
        return Err(format!("demasiado larga (más de {MAX_SECRETO} bytes)"));
    }
    if v.chars().any(char::is_control) {
        return Err("no puede llevar caracteres de control".into());
    }
    Ok(())
}

// ---------------------------------------------------------------- fichero

/// El fichero de configuración, línea a línea.
struct Fichero {
    lineas: Vec<String>,
    crlf: bool,
    existe: bool,
}

const CABECERA: &str = "\
# Configuración de semillero, escrita por «semillero configurar» (vuelve a
# ejecutarlo para cambiarla). Una variable por línea, VARIABLE=valor, sin
# comillas; la lista está en el README. La contraseña de qBittorrent y la
# clave de Jellyfin no van aquí: cada una en su fichero, solo para su dueño.";

impl Fichero {
    fn leer(ruta: &Path) -> Result<Self, String> {
        match std::fs::read(ruta) {
            Ok(bytes) => {
                if bytes.len() > 64 * 1024 {
                    return Err(format!("{} es demasiado grande", ruta.display()));
                }
                let texto = String::from_utf8(bytes)
                    .map_err(|_| format!("{} no es texto UTF-8", ruta.display()))?;
                Ok(Self {
                    crlf: texto.contains("\r\n"),
                    lineas: texto.lines().map(str::to_owned).collect(),
                    existe: true,
                })
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self {
                lineas: CABECERA
                    .lines()
                    .map(str::to_owned)
                    .chain([String::new()])
                    .collect(),
                crlf: false,
                existe: false,
            }),
            Err(e) => Err(format!("no se puede leer {}: {e}", ruta.display())),
        }
    }

    fn es(linea: &str, k: &str) -> bool {
        linea.strip_prefix(k).is_some_and(|r| r.starts_with('='))
    }

    /// El valor de `k` (el de la última línea, como systemd).
    fn valor(&self, k: &str) -> Option<String> {
        self.lineas
            .iter()
            .rev()
            .find(|l| Self::es(l, k))
            .map(|l| l[k.len() + 1..].trim().to_owned())
            .filter(|v| !v.is_empty())
    }

    fn poner(&mut self, k: &str, v: &str) {
        let nueva = format!("{k}={v}");
        match self.lineas.iter().position(|l| Self::es(l, k)) {
            Some(i) => {
                self.lineas[i] = nueva;
                let mut j = 0;
                self.lineas.retain(|l| {
                    j += 1;
                    j - 1 <= i || !Self::es(l, k)
                });
            }
            None => self.lineas.push(nueva),
        }
    }

    fn quitar(&mut self, k: &str) {
        self.lineas.retain(|l| !Self::es(l, k));
    }

    fn aplicar(&mut self, k: &str, r: Respuesta) {
        match r {
            Respuesta::Igual => {}
            Respuesta::Quitar => self.quitar(k),
            Respuesta::Valor(v) => self.poner(k, &v),
        }
    }

    fn texto(&self) -> String {
        let fin = if self.crlf { "\r\n" } else { "\n" };
        self.lineas.iter().map(|l| format!("{l}{fin}")).collect()
    }
}

// ---------------------------------------------------------------- escritura

/// Escribe `contenido` en `ruta` de golpe: un temporal en la misma carpeta,
/// creado ya con `modo` (Unix) y, si se da, con `dueno`, y luego `rename`.
/// Nunca queda a medias ni, siendo un secreto, legible por otros.
fn escribir(
    ruta: &Path,
    contenido: &[u8],
    #[cfg_attr(not(unix), allow(unused_variables))] modo: u32,
    #[cfg_attr(not(unix), allow(unused_variables))] dueno: Option<(u32, u32)>,
) -> io::Result<()> {
    let nombre = ruta.file_name().map(|n| n.to_string_lossy().into_owned());
    let tmp = ruta.with_file_name(format!(
        ".{}.{}.tmp",
        nombre.unwrap_or_default(),
        std::process::id()
    ));
    let hacer = || -> io::Result<()> {
        let mut op = std::fs::OpenOptions::new();
        op.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            op.mode(modo);
        }
        let mut f = op.open(&tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            f.set_permissions(std::fs::Permissions::from_mode(modo))?;
            if let Some((uid, gid)) = dueno {
                std::os::unix::fs::fchown(&f, Some(uid), Some(gid))?;
            }
        }
        f.write_all(contenido)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, ruta)
    };
    hacer().inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Dueño que han de tener los secretos de la carpeta `dir`: siendo root, el
/// de la carpeta (el usuario del servicio, que tiene que leerlos); si no,
/// quien lo ejecuta.
#[cfg(unix)]
fn dueno_de(dir: &Path) -> Option<(u32, u32)> {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: geteuid no tiene precondiciones.
    if unsafe { libc::geteuid() } != 0 {
        return None;
    }
    let m = std::fs::metadata(dir).ok()?;
    (m.uid() != 0).then_some((m.uid(), m.gid()))
}

#[cfg(not(unix))]
fn dueno_de(_dir: &Path) -> Option<(u32, u32)> {
    None
}

/// Crea la carpeta de los secretos si falta, solo para su dueño.
fn crear_carpeta_secretos(dir: &Path) -> io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o700);
    }
    b.create(dir)
}

#[cfg(unix)]
fn modo_y_dueno(ruta: &Path) -> (u32, Option<(u32, u32)>) {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    match std::fs::metadata(ruta) {
        Ok(m) => (m.permissions().mode() & 0o7777, Some((m.uid(), m.gid()))),
        Err(_) => (0o600, None),
    }
}

#[cfg(not(unix))]
fn modo_y_dueno(_ruta: &Path) -> (u32, Option<(u32, u32)>) {
    (0o600, None)
}

// ---------------------------------------------------------------- preguntas

/// Un secreto que pedir: la variable con su fichero, el nombre del fichero
/// por defecto y cómo se llama.
struct Secreto {
    var: &'static str,
    fichero: &'static str,
    nombre: &'static str,
}

const CLAVE_QBT: Secreto = Secreto {
    var: "SEMILLERO_QBT_CLAVE",
    fichero: "qbt.clave",
    nombre: "la contraseña de qBittorrent",
};

const CLAVE_JELLYFIN: Secreto = Secreto {
    var: "SEMILLERO_JELLYFIN_CLAVE",
    fichero: "jellyfin.clave",
    nombre: "la clave de la API de Jellyfin",
};

/// Un secreto ya respondido: dónde va y, si es nuevo, qué se escribe.
struct Respondido {
    ruta: PathBuf,
    valor: String,
    nuevo: bool,
    nombre: &'static str,
}

/// Pide el secreto `s`. Si el fichero de configuración ya apunta a uno que
/// se puede leer, Intro lo deja.
fn pedir_secreto(
    e: &mut Entrada,
    f: &mut Fichero,
    s: &Secreto,
    pregunta: &str,
    secretos: &Path,
) -> Result<Respondido, SinEntrada> {
    let ruta = f
        .valor(s.var)
        .map(PathBuf::from)
        .unwrap_or_else(|| secretos.join(s.fichero));
    let actual = leer_secreto(&ruta, s.nombre).ok();
    let texto = if actual.is_some() {
        format!("{pregunta} (Intro: deja la que hay)")
    } else {
        pregunta.to_owned()
    };
    let (valor, nuevo) = loop {
        let v = e.secreta(&texto)?;
        if v.is_empty() {
            match &actual {
                Some(a) => break (a.clone(), false),
                None => {
                    println!("  Hace falta {}.", s.nombre);
                    continue;
                }
            }
        }
        match validar_secreto(&v) {
            Ok(()) => break (v, true),
            Err(m) => println!("  {m}"),
        }
    };
    f.poner(s.var, &ruta.display().to_string());
    Ok(Respondido {
        ruta,
        valor,
        nuevo,
        nombre: s.nombre,
    })
}

/// Variable que ya fija el entorno de este proceso: no se pregunta.
fn fijada(k: &str) -> bool {
    match std::env::var(k) {
        Ok(v) => {
            println!("{k} lo fija el entorno ({v}): no se pregunta.");
            true
        }
        Err(_) => false,
    }
}

/// Lo que hace falta para las comprobaciones.
struct Respuestas {
    qbt: String,
    cuenta: Option<(String, String)>,
    jellyfin: Option<(String, String)>,
    secretos: Vec<Respondido>,
    /// Ficheros de claves que ya no se usan (se ofrece borrarlos).
    sueltos: Vec<PathBuf>,
}

fn preguntar(e: &mut Entrada, f: &mut Fichero, secretos: &Path) -> Result<Respuestas, SinEntrada> {
    // Dónde escucha.
    if !fijada("SEMILLERO_LISTEN") {
        let actual = f.valor("SEMILLERO_LISTEN");
        let r = pedir(
            e,
            "Dónde escucha semillero (IP:puerto)",
            Some(actual.as_deref().unwrap_or("127.0.0.1:8090")),
            false,
            validar_escucha,
        )?;
        f.aplicar("SEMILLERO_LISTEN", r);
    }
    let puerto = std::env::var("SEMILLERO_LISTEN")
        .ok()
        .or_else(|| f.valor("SEMILLERO_LISTEN"))
        .and_then(|v| v.parse::<SocketAddr>().ok())
        .map_or(8090, |a| a.port());

    // Dirección en el navegador.
    if !fijada("SEMILLERO_ORIGEN") {
        let propuesta = f
            .valor("SEMILLERO_ORIGEN")
            .unwrap_or_else(|| format!("http://127.0.0.1:{puerto},http://localhost:{puerto}"));
        let r = pedir(
            e,
            "Dirección con la que abres semillero en el navegador (varias, separadas por \
             comas; desde otro equipo, https://)",
            Some(&propuesta),
            false,
            validar_origenes,
        )?;
        f.aplicar("SEMILLERO_ORIGEN", r);
    }

    // qBittorrent.
    let por_defecto_qbt = "http://127.0.0.1:8080";
    if !fijada("SEMILLERO_QBT") {
        let actual = f.valor("SEMILLERO_QBT");
        let r = pedir(
            e,
            "Dirección de la interfaz web de qBittorrent",
            Some(actual.as_deref().unwrap_or(por_defecto_qbt)),
            false,
            |v| validar_url(v, por_defecto_qbt),
        )?;
        f.aplicar("SEMILLERO_QBT", r);
    }
    let qbt = std::env::var("SEMILLERO_QBT")
        .ok()
        .or_else(|| f.valor("SEMILLERO_QBT"))
        .unwrap_or_else(|| por_defecto_qbt.into());

    let mut respondidos = Vec::new();
    let mut sueltos = Vec::new();
    let con_contrasena = if f.existe {
        f.valor("SEMILLERO_QBT_CLAVE").is_some()
    } else {
        true
    };
    println!(
        "Cómo entra semillero en qBittorrent:\n  \
         1) con contraseña (recomendado: vale en el mismo equipo, en otro, en podman y en Windows)\n  \
         2) lista blanca (sin contraseña: qBittorrent en este mismo equipo)"
    );
    let propuesta = if con_contrasena { "1" } else { "2" };
    let modo = match pedir(e, "Modo", Some(propuesta), false, |v| match v {
        "1" | "2" => Ok(v.to_owned()),
        _ => Err("Contesta 1 o 2.".into()),
    })? {
        Respuesta::Valor(v) => v,
        _ => propuesta.to_owned(),
    };
    let cuenta = if modo == "1" {
        let actual = f.valor("SEMILLERO_QBT_USUARIO");
        let r = pedir(
            e,
            "Usuario de la interfaz web de qBittorrent",
            Some(actual.as_deref().unwrap_or("admin")),
            false,
            validar_texto,
        )?;
        f.aplicar("SEMILLERO_QBT_USUARIO", r);
        let usuario = f
            .valor("SEMILLERO_QBT_USUARIO")
            .unwrap_or_else(|| "admin".into());
        let s = pedir_secreto(e, f, &CLAVE_QBT, "Contraseña de qBittorrent", secretos)?;
        let clave = s.valor.clone();
        respondidos.push(s);
        Some((usuario, clave))
    } else {
        sueltos.extend(f.valor("SEMILLERO_QBT_CLAVE").map(PathBuf::from));
        f.quitar("SEMILLERO_QBT_CLAVE");
        f.quitar("SEMILLERO_QBT_USUARIO");
        None
    };

    // Proxy inverso.
    if !fijada("SEMILLERO_PROXY") {
        let actual = f.valor("SEMILLERO_PROXY");
        let r = pedir(
            e,
            "IP del proxy inverso, si lo hay",
            actual.as_deref(),
            true,
            validar_ip,
        )?;
        f.aplicar("SEMILLERO_PROXY", r);
    }

    // Jellyfin.
    let mut jellyfin = None;
    if !fijada("SEMILLERO_JELLYFIN") {
        let actual = f.valor("SEMILLERO_JELLYFIN");
        let r = pedir(
            e,
            "Dirección de Jellyfin, si lo usas",
            actual.as_deref(),
            true,
            |v| validar_url(v, "http://192.0.2.20:8096"),
        )?;
        if matches!(r, Respuesta::Quitar) {
            sueltos.extend(f.valor("SEMILLERO_JELLYFIN_CLAVE").map(PathBuf::from));
            f.quitar("SEMILLERO_JELLYFIN_CLAVE");
        }
        f.aplicar("SEMILLERO_JELLYFIN", r);
        if let Some(url) = f.valor("SEMILLERO_JELLYFIN") {
            let s = pedir_secreto(
                e,
                f,
                &CLAVE_JELLYFIN,
                "Clave de la API de Jellyfin",
                secretos,
            )?;
            jellyfin = Some((url, s.valor.clone()));
            respondidos.push(s);
        }
    }

    // Fichero de estado de la VPN y del puerto.
    if !fijada("SEMILLERO_ESTADO") {
        let actual = f.valor("SEMILLERO_ESTADO");
        let r = pedir(
            e,
            "Fichero de estado de la VPN y del puerto, si lo hay",
            actual.as_deref(),
            true,
            validar_texto,
        )?;
        f.aplicar("SEMILLERO_ESTADO", r);
    }

    // Base de datos.
    if !fijada("SEMILLERO_DB") {
        let actual = f.valor("SEMILLERO_DB");
        let r = pedir(
            e,
            "Base de datos",
            Some(actual.as_deref().unwrap_or("semillero.db")),
            false,
            validar_texto,
        )?;
        f.aplicar("SEMILLERO_DB", r);
    }

    Ok(Respuestas {
        qbt,
        cuenta,
        jellyfin,
        secretos: respondidos,
        sueltos,
    })
}

// ---------------------------------------------------------------- comprobaciones

async fn comprobar_qbt(url: &str, cuenta: Option<&(String, String)>) -> Result<String, String> {
    let no_responde = || format!("qBittorrent no responde en {url}");
    match cuenta {
        Some((usuario, clave)) => {
            let q = Qbt::con_cuenta(url, None, Cuenta::new(usuario, clave));
            match q.credenciales_validas(usuario, clave).await {
                Ok(true) => Ok("qBittorrent acepta el usuario y la contraseña".into()),
                Ok(false) => Err("qBittorrent no acepta ese usuario y contraseña".into()),
                Err(qbt::Error::NoResponde(_)) => Err(no_responde()),
                Err(qbt::Error::Rechazado(403)) => Err(
                    "qBittorrent rechaza el intento (puede que haya bloqueado esta IP tras \
                     varios fallos seguidos)"
                        .into(),
                ),
                Err(e) => Err(format!("qBittorrent: {e}")),
            }
        }
        None => match Qbt::new(url, IpAddr::from([127, 0, 0, 1])).torrents().await {
            Ok(_) => Ok("qBittorrent contesta sin contraseña desde este equipo".into()),
            Err(qbt::Error::NoResponde(_)) => Err(no_responde()),
            Err(qbt::Error::Rechazado(403)) => Err(
                "qBittorrent pide contraseña: este equipo no está en su lista blanca. Añade su \
                 IP a la lista blanca o usa el modo con contraseña"
                    .into(),
            ),
            Err(e) => Err(format!("qBittorrent: {e}")),
        },
    }
}

async fn comprobar_jellyfin(url: &str, clave: &str) -> Result<String, String> {
    match jellyfin::Jellyfin::new(url, clave).comprobar().await {
        Ok(()) => Ok("Jellyfin acepta la clave de la API".into()),
        Err(jellyfin::Error::NoResponde) => Err(format!("Jellyfin no responde en {url}")),
        Err(e) => Err(e.to_string()),
    }
}

/// Comprueba lo respondido; dice cada cosa y devuelve si todo fue bien.
async fn comprobar(f: &Fichero, r: &Respuestas) -> bool {
    println!("Comprobando…");
    let mut resultados = vec![comprobar_qbt(&r.qbt, r.cuenta.as_ref()).await];
    if let Some((url, clave)) = &r.jellyfin {
        resultados.push(comprobar_jellyfin(url, clave).await);
    }
    // La configuración entera, como la leerá el servidor.
    let mut aviso = None;
    match Config::desde(|k| std::env::var(k).ok().or_else(|| f.valor(k))) {
        Ok(c) => aviso = c.aviso_login(),
        Err(e) => resultados.push(Err(e)),
    }
    let mut todo_bien = true;
    for r in resultados {
        match r {
            Ok(m) => println!("  bien: {m}"),
            Err(m) => {
                todo_bien = false;
                println!("  MAL:  {m}");
            }
        }
    }
    if let Some(a) = aviso {
        println!("  aviso: {a}");
    }
    todo_bien
}

/// Ofrece borrar cada fichero de clave que la configuración ya no usa. Sin
/// «s» (también sin respuesta) se queda.
fn borrar_sueltos(e: &mut Entrada, r: &Respuestas) {
    for ruta in &r.sueltos {
        if !ruta.is_file() || r.secretos.iter().any(|s| s.ruta == *ruta) {
            continue;
        }
        let pregunta = format!(
            "La configuración ya no usa {}. ¿Borrar el fichero antiguo {}?",
            ruta.file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
            ruta.display()
        );
        if !matches!(e.si(&pregunta), Ok(true)) {
            println!("Se queda {}.", ruta.display());
            continue;
        }
        match std::fs::remove_file(ruta) {
            Ok(()) => println!("Borrado {}.", ruta.display()),
            Err(err) => println!("No se puede borrar {}: {err}", ruta.display()),
        }
    }
}

// ---------------------------------------------------------------- todo junto

async fn configurar(op: &Opciones) -> Result<ExitCode, String> {
    let mut f = Fichero::leer(&op.env)?;
    let original = f.texto();
    if f.existe {
        println!(
            "Configuración de {} (Intro deja lo que hay).",
            op.env.display()
        );
    } else {
        println!(
            "Configuración nueva en {} (Intro deja el valor propuesto).",
            op.env.display()
        );
    }
    let mut e = Entrada {
        terminal: io::stdin().is_terminal(),
    };
    let r = preguntar(&mut e, &mut f, &op.secretos)?;
    if !comprobar(&f, &r).await && !e.si("¿Guardar de todos modos?")? {
        println!("No se ha guardado nada.");
        return Ok(ExitCode::FAILURE);
    }

    for s in r.secretos.iter().filter(|s| s.nuevo) {
        let dir = s.ruta.parent().unwrap_or(Path::new("."));
        crear_carpeta_secretos(dir)
            .map_err(|e| format!("no se puede crear {}: {e}", dir.display()))?;
        escribir(
            &s.ruta,
            format!("{}\n", s.valor).as_bytes(),
            0o600,
            dueno_de(dir),
        )
        .map_err(|e| {
            format!(
                "no se puede guardar {} en {}: {e}",
                s.nombre,
                s.ruta.display()
            )
        })?;
        println!(
            "Guardada {} en {} (solo para su dueño).",
            s.nombre,
            s.ruta.display()
        );
    }
    let texto = f.texto();
    if f.existe && texto == original {
        println!("Sin cambios en {}.", op.env.display());
    } else {
        if let Some(dir) = op.env.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("no se puede crear {}: {e}", dir.display()))?;
        }
        let (modo, dueno) = modo_y_dueno(&op.env);
        escribir(&op.env, texto.as_bytes(), modo, dueno)
            .map_err(|e| format!("no se puede guardar {}: {e}", op.env.display()))?;
        println!("Configuración guardada en {}.", op.env.display());
    }
    borrar_sueltos(&mut e, &r);
    println!("Reinicia semillero para que la use.");
    Ok(ExitCode::SUCCESS)
}
