//! Base de datos SQLite: apertura con migraciones y accesos con tipos.
//!
//! Las sesiones y el freno del login están en [`crate::auth`]; aquí, lo que
//! usa la tarea horaria y la interfaz: modo, normas del tracker (las de por
//! defecto y las de cada tracker), reglas, presets, avisos, exenciones,
//! historial, muestras de subida y el último tracker visto de cada torrent.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use reglas::preset::Preset;
use reglas::trackers::normalizar_host;
use reglas::{Aviso, JuegoNormas, NormasTracker, ReglasCategoria, TipoRegla};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{SqliteExecutor, SqlitePool};

/// Abre (o crea) la base de datos en `ruta` y aplica las migraciones. En
/// Unix la deja solo para su dueño (0600) aunque la umask sea otra: guarda
/// nombres de torrents, el historial y las sesiones. SQLite da a los
/// ficheros `-wal` y `-shm` los permisos de la base.
pub async fn abrir(ruta: &Path) -> Result<SqlitePool, sqlx::Error> {
    #[cfg(unix)]
    solo_dueno(ruta)?;
    let opciones = SqliteConnectOptions::new()
        .filename(ruta)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(opciones)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

#[cfg(unix)]
fn solo_dueno(ruta: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(ruta)
    {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    // `mode` pasa por la umask (que solo quita permisos) y una base vieja
    // pudo crearse con otros: se fijan siempre.
    std::fs::set_permissions(ruta, std::fs::Permissions::from_mode(0o600))
}

fn codigo_regla(r: TipoRegla) -> &'static str {
    match r {
        TipoRegla::Ratio => "ratio",
        TipoRegla::Siembra => "siembra",
        TipoRegla::Inactividad => "inactividad",
        TipoRegla::Disco => "disco",
    }
}

fn regla_de(codigo: &str) -> Result<TipoRegla, sqlx::Error> {
    Ok(match codigo {
        "ratio" => TipoRegla::Ratio,
        "siembra" => TipoRegla::Siembra,
        "inactividad" => TipoRegla::Inactividad,
        "disco" => TipoRegla::Disco,
        otro => {
            return Err(sqlx::Error::Decode(
                format!("regla desconocida: {otro}").into(),
            ));
        }
    })
}

/// Qué pasa cuando vence un aviso.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Modo {
    /// Se apunta «se habría borrado» y no se borra nada (al principio).
    #[default]
    SoloAviso,
    BorradoReal,
}

impl Modo {
    fn codigo(self) -> &'static str {
        match self {
            Modo::SoloAviso => "solo_aviso",
            Modo::BorradoReal => "borrado_real",
        }
    }
}

pub async fn modo(db: &SqlitePool) -> Result<Modo, sqlx::Error> {
    let v: Option<String> = sqlx::query_scalar("SELECT valor FROM ajustes WHERE clave = 'modo'")
        .fetch_optional(db)
        .await?;
    // Cualquier valor que no sea exactamente el del borrado real es «solo aviso».
    Ok(match v.as_deref() {
        Some("borrado_real") => Modo::BorradoReal,
        _ => Modo::SoloAviso,
    })
}

/// Cambia el modo. Pasar al borrado real exige haber vuelto a escribir la
/// contraseña (lo comprueba quien llama, con [`crate::auth::reautenticacion_vigente`]).
pub async fn fijar_modo(db: &SqlitePool, m: Modo) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO ajustes (clave, valor) VALUES ('modo', ?)
         ON CONFLICT (clave) DO UPDATE SET valor = excluded.valor",
    )
    .bind(m.codigo())
    .execute(db)
    .await?;
    Ok(())
}

/// Valores que la base de datos no guarda: fuera de rango.
fn fuera_de_rango(msg: &str) -> sqlx::Error {
    sqlx::Error::Protocol(format!("valor fuera de rango: {msg}"))
}

type FilaNormas = (f64, i64, f64, i64, f64, i64);

/// Las normas del tracker guardadas (las de por defecto si no hay fila).
pub async fn normas(db: impl SqliteExecutor<'_>) -> Result<NormasTracker, sqlx::Error> {
    let fila: Option<FilaNormas> = sqlx::query_as(
        "SELECT umbral_pct, siembra_dias, ratio_min, plazo_largo_dias, ratio_pausa, aviso_horas
         FROM normas WHERE id = 1",
    )
    .fetch_optional(db)
    .await?;
    let Some(fila) = fila else {
        return Ok(NormasTracker::default());
    };
    normas_de_fila(fila)
}

fn normas_de_fila(fila: FilaNormas) -> Result<NormasTracker, sqlx::Error> {
    let (umbral_pct, siembra, ratio_min, plazo, ratio_pausa, aviso) = fila;
    let entero = |v: i64| {
        u32::try_from(v).map_err(|_| sqlx::Error::Decode("normas: entero fuera de rango".into()))
    };
    let n = NormasTracker {
        umbral_pct,
        siembra_dias: entero(siembra)?,
        ratio_min,
        plazo_largo_dias: entero(plazo)?,
        ratio_pausa,
        aviso_horas: entero(aviso)?,
    };
    n.validar()
        .map_err(|e| sqlx::Error::Decode(format!("normas: {e}").into()))?;
    Ok(n)
}

/// Guarda las normas del tracker. Fuera de rango, error y nada cambia.
pub async fn guardar_normas(
    db: impl SqliteExecutor<'_>,
    n: &NormasTracker,
) -> Result<(), sqlx::Error> {
    n.validar().map_err(fuera_de_rango)?;
    sqlx::query(
        "INSERT INTO normas
           (id, umbral_pct, siembra_dias, ratio_min, plazo_largo_dias, ratio_pausa, aviso_horas)
         VALUES (1, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (id) DO UPDATE SET
           umbral_pct = excluded.umbral_pct, siembra_dias = excluded.siembra_dias,
           ratio_min = excluded.ratio_min, plazo_largo_dias = excluded.plazo_largo_dias,
           ratio_pausa = excluded.ratio_pausa, aviso_horas = excluded.aviso_horas",
    )
    .bind(n.umbral_pct)
    .bind(i64::from(n.siembra_dias))
    .bind(n.ratio_min)
    .bind(i64::from(n.plazo_largo_dias))
    .bind(n.ratio_pausa)
    .bind(i64::from(n.aviso_horas))
    .execute(db)
    .await?;
    Ok(())
}

/// Las normas propias de cada tracker, por host.
pub async fn normas_trackers(
    db: impl SqliteExecutor<'_>,
) -> Result<BTreeMap<String, NormasTracker>, sqlx::Error> {
    let filas: Vec<(String, f64, i64, f64, i64, f64, i64)> = sqlx::query_as(
        "SELECT host, umbral_pct, siembra_dias, ratio_min, plazo_largo_dias, ratio_pausa,
                aviso_horas
         FROM normas_tracker ORDER BY host",
    )
    .fetch_all(db)
    .await?;
    filas
        .into_iter()
        .map(|(host, a, b, c, d, e, f)| Ok((host, normas_de_fila((a, b, c, d, e, f))?)))
        .collect()
}

/// Las normas por defecto y las de cada tracker.
pub async fn juego_normas(db: &SqlitePool) -> Result<JuegoNormas, sqlx::Error> {
    Ok(JuegoNormas {
        defecto: normas(db).await?,
        trackers: normas_trackers(db).await?,
    })
}

/// Guarda las normas propias del tracker `host` (ya en su forma de guardar,
/// ver [`normalizar_host`]). Fuera de rango o con un host que no vale,
/// error y nada cambia.
pub async fn guardar_normas_tracker(
    db: impl SqliteExecutor<'_>,
    host: &str,
    n: &NormasTracker,
) -> Result<(), sqlx::Error> {
    if normalizar_host(host).as_deref() != Some(host) {
        return Err(fuera_de_rango("host de tracker"));
    }
    n.validar().map_err(fuera_de_rango)?;
    sqlx::query(
        "INSERT INTO normas_tracker
           (host, umbral_pct, siembra_dias, ratio_min, plazo_largo_dias, ratio_pausa, aviso_horas)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (host) DO UPDATE SET
           umbral_pct = excluded.umbral_pct, siembra_dias = excluded.siembra_dias,
           ratio_min = excluded.ratio_min, plazo_largo_dias = excluded.plazo_largo_dias,
           ratio_pausa = excluded.ratio_pausa, aviso_horas = excluded.aviso_horas",
    )
    .bind(host)
    .bind(n.umbral_pct)
    .bind(i64::from(n.siembra_dias))
    .bind(n.ratio_min)
    .bind(i64::from(n.plazo_largo_dias))
    .bind(n.ratio_pausa)
    .bind(i64::from(n.aviso_horas))
    .execute(db)
    .await?;
    Ok(())
}

/// Quita las normas propias del tracker `host`: vuelve a las de por
/// defecto. Devuelve si las tenía.
pub async fn quitar_normas_tracker(
    db: impl SqliteExecutor<'_>,
    host: &str,
) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("DELETE FROM normas_tracker WHERE host = ?")
        .bind(host)
        .execute(db)
        .await?;
    Ok(r.rows_affected() > 0)
}

/// Apunta que los torrents `presentes` (hash y host del tracker, vacío si
/// qBittorrent no da ninguno) están en qBittorrent a la hora `ahora`. Con
/// host, se recuerda; sin él, se queda el último que se vio.
pub async fn recordar_trackers(
    db: &SqlitePool,
    presentes: &[(&str, &str)],
    ahora: i64,
) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    for (hash, host) in presentes {
        let host = normalizar_host(host).unwrap_or_default();
        if host.is_empty() {
            sqlx::query("UPDATE trackers_vistos SET visto = ? WHERE hash = ?")
                .bind(ahora)
                .bind(hash)
                .execute(&mut *tx)
                .await?;
        } else {
            sqlx::query(
                "INSERT INTO trackers_vistos (hash, host, visto) VALUES (?, ?, ?)
                 ON CONFLICT (hash) DO UPDATE SET host = excluded.host, visto = excluded.visto",
            )
            .bind(hash)
            .bind(&host)
            .bind(ahora)
            .execute(&mut *tx)
            .await?;
        }
    }
    tx.commit().await
}

/// El último host de tracker visto de cada torrent (por hash).
pub async fn trackers_vistos(db: &SqlitePool) -> Result<HashMap<String, String>, sqlx::Error> {
    let filas: Vec<(String, String)> = sqlx::query_as("SELECT hash, host FROM trackers_vistos")
        .fetch_all(db)
        .await?;
    Ok(filas.into_iter().collect())
}

/// Olvida el tracker de los torrents que no se ven en qBittorrent desde
/// antes de `antes_de` (los que están se apuntan en cada pasada).
pub async fn olvidar_trackers(db: &SqlitePool, antes_de: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM trackers_vistos WHERE visto < ?")
        .bind(antes_de)
        .execute(db)
        .await?;
    Ok(())
}

type FilaReglas = (String, Option<f64>, Option<i64>, Option<i64>, Option<f64>);

/// Reglas de las categorías que tienen alguna guardada. Las que no están,
/// todas desactivadas.
pub async fn reglas(
    db: impl SqliteExecutor<'_>,
) -> Result<HashMap<String, ReglasCategoria>, sqlx::Error> {
    let filas: Vec<FilaReglas> = sqlx::query_as(
        "SELECT categoria, ratio, siembra_dias, inactividad_dias, disco_libre_pct FROM reglas",
    )
    .fetch_all(db)
    .await?;
    let dias = |d: Option<i64>| d.and_then(|d| u32::try_from(d).ok());
    Ok(filas
        .into_iter()
        .map(|(c, ratio, siembra, inactividad, disco)| {
            (
                c,
                ReglasCategoria {
                    ratio,
                    siembra_dias: dias(siembra),
                    inactividad_dias: dias(inactividad),
                    disco_libre_pct: disco,
                },
            )
        })
        .collect())
}

pub async fn guardar_reglas(
    db: impl SqliteExecutor<'_>,
    categoria: &str,
    r: &ReglasCategoria,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO reglas (categoria, ratio, siembra_dias, inactividad_dias, disco_libre_pct)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT (categoria) DO UPDATE SET
           ratio = excluded.ratio, siembra_dias = excluded.siembra_dias,
           inactividad_dias = excluded.inactividad_dias,
           disco_libre_pct = excluded.disco_libre_pct",
    )
    .bind(categoria)
    .bind(r.ratio)
    .bind(r.siembra_dias.map(i64::from))
    .bind(r.inactividad_dias.map(i64::from))
    .bind(r.disco_libre_pct)
    .execute(db)
    .await?;
    Ok(())
}

/// Un preset guardado, para la lista.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilaPreset {
    pub id: i64,
    pub nombre: String,
    pub descripcion: String,
    /// Trackers con normas propias en el preset.
    pub trackers: usize,
}

/// Los presets guardados, por nombre.
pub async fn presets(db: &SqlitePool) -> Result<Vec<FilaPreset>, sqlx::Error> {
    let filas: Vec<(i64, String, String, String)> =
        sqlx::query_as("SELECT id, nombre, descripcion, archivo FROM presets ORDER BY nombre, id")
            .fetch_all(db)
            .await?;
    Ok(filas
        .into_iter()
        .map(|(id, nombre, descripcion, archivo)| FilaPreset {
            id,
            nombre,
            descripcion,
            trackers: Preset::leer(&archivo).map_or(0, |p| p.trackers.len()),
        })
        .collect())
}

/// Un preset guardado, leído y comprobado otra vez. `None` si no está.
pub async fn preset(db: &SqlitePool, id: i64) -> Result<Option<Preset>, sqlx::Error> {
    let archivo: Option<String> = sqlx::query_scalar("SELECT archivo FROM presets WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await?;
    archivo
        .map(|a| {
            Preset::leer(&a).map_err(|e| sqlx::Error::Decode(format!("preset {id}: {e}").into()))
        })
        .transpose()
}

pub async fn cuantos_presets(db: &SqlitePool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT COUNT(*) FROM presets")
        .fetch_one(db)
        .await
}

/// Guarda un preset nuevo y devuelve su id. Un preset que no pasa
/// [`Preset::validar`] no se guarda; un nombre repetido (sin mirar
/// mayúsculas) es un error de unicidad.
pub async fn crear_preset(db: &SqlitePool, p: &Preset, ahora: i64) -> Result<i64, sqlx::Error> {
    p.validar().map_err(|e| fuera_de_rango(&e))?;
    let r = sqlx::query(
        "INSERT INTO presets (nombre, descripcion, archivo, cambiado) VALUES (?, ?, ?, ?)",
    )
    .bind(&p.nombre)
    .bind(&p.descripcion)
    .bind(p.escribir())
    .bind(ahora)
    .execute(db)
    .await?;
    Ok(r.last_insert_rowid())
}

/// Sustituye el preset `id` por `p`. Devuelve si estaba.
pub async fn cambiar_preset(
    db: &SqlitePool,
    id: i64,
    p: &Preset,
    ahora: i64,
) -> Result<bool, sqlx::Error> {
    p.validar().map_err(|e| fuera_de_rango(&e))?;
    let r = sqlx::query(
        "UPDATE presets SET nombre = ?, descripcion = ?, archivo = ?, cambiado = ? WHERE id = ?",
    )
    .bind(&p.nombre)
    .bind(&p.descripcion)
    .bind(p.escribir())
    .bind(ahora)
    .bind(id)
    .execute(db)
    .await?;
    Ok(r.rows_affected() > 0)
}

/// Borra el preset `id`. Devuelve si estaba.
pub async fn borrar_preset(db: &SqlitePool, id: i64) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("DELETE FROM presets WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;
    Ok(r.rows_affected() > 0)
}

/// Aplica un preset: sus normas, las de sus trackers y sus reglas
/// sustituyen a las de ahora (las categorías que no trae quedan con todo
/// apagado y los trackers que no trae, con las normas por defecto) y se
/// apunta en el historial, todo en una transacción.
pub async fn aplicar_preset(db: &SqlitePool, p: &Preset, ahora: i64) -> Result<(), sqlx::Error> {
    p.validar().map_err(|e| fuera_de_rango(&e))?;
    let mut tx = db.begin().await?;
    guardar_normas(&mut *tx, &p.normas).await?;
    let quitadas = sqlx::query("DELETE FROM normas_tracker")
        .execute(&mut *tx)
        .await?
        .rows_affected();
    for (host, n) in &p.trackers {
        guardar_normas_tracker(&mut *tx, host, n).await?;
    }
    sqlx::query("DELETE FROM reglas").execute(&mut *tx).await?;
    for (cat, r) in &p.categorias {
        guardar_reglas(&mut *tx, cat, r).await?;
    }
    let mut detalle = format!(
        "Preset «{}» aplicado: normas del tracker y reglas nuevas",
        p.nombre
    );
    if quitadas > 0 || !p.trackers.is_empty() {
        detalle.push_str(&format!(
            "; normas propias para {} (antes, {quitadas})",
            trackers_en_palabras(p.trackers.len())
        ));
    }
    apuntar(
        &mut *tx,
        &Apunte {
            ts: ahora,
            hash: String::new(),
            nombre: String::new(),
            evento: Evento::PresetAplicado,
            regla: None,
            detalle,
        },
    )
    .await?;
    tx.commit().await
}

/// «1 tracker», «3 trackers», «ningún tracker».
pub fn trackers_en_palabras(n: usize) -> String {
    match n {
        0 => "ningún tracker".into(),
        1 => "1 tracker".into(),
        n => format!("{n} trackers"),
    }
}

pub async fn avisos(db: &SqlitePool) -> Result<Vec<Aviso>, sqlx::Error> {
    let filas: Vec<(String, String, i64, i64)> = sqlx::query_as(
        "SELECT hash, regla, creado_en, vence_en FROM avisos ORDER BY vence_en, hash",
    )
    .fetch_all(db)
    .await?;
    filas
        .into_iter()
        .map(|(hash, regla, creado_en, vence_en)| {
            Ok(Aviso {
                hash,
                regla: regla_de(&regla)?,
                creado_en,
                vence_en,
            })
        })
        .collect()
}

/// Sustituye todos los avisos pendientes por `vigentes` (el resultado de una
/// pasada de [`reglas::avanzar`]), en una sola transacción.
pub async fn guardar_avisos(db: &SqlitePool, vigentes: &[Aviso]) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM avisos").execute(&mut *tx).await?;
    for a in vigentes {
        sqlx::query("INSERT INTO avisos (hash, regla, creado_en, vence_en) VALUES (?, ?, ?, ?)")
            .bind(&a.hash)
            .bind(codigo_regla(a.regla))
            .bind(a.creado_en)
            .bind(a.vence_en)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await
}

/// Quita el aviso del torrent si lo causó `regla`. Devuelve si había uno.
pub async fn quitar_aviso(
    db: &SqlitePool,
    hash: &str,
    regla: TipoRegla,
) -> Result<bool, sqlx::Error> {
    let r = sqlx::query("DELETE FROM avisos WHERE hash = ? AND regla = ?")
        .bind(hash)
        .bind(codigo_regla(regla))
        .execute(db)
        .await?;
    Ok(r.rows_affected() > 0)
}

/// «No borrar»: exime al torrent de esa regla para siempre.
pub async fn eximir(
    db: &SqlitePool,
    hash: &str,
    regla: TipoRegla,
    ahora: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT OR IGNORE INTO exenciones (hash, regla, creada) VALUES (?, ?, ?)")
        .bind(hash)
        .bind(codigo_regla(regla))
        .bind(ahora)
        .execute(db)
        .await?;
    Ok(())
}

/// Deshace un «No borrar».
pub async fn quitar_exencion(
    db: &SqlitePool,
    hash: &str,
    regla: TipoRegla,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM exenciones WHERE hash = ? AND regla = ?")
        .bind(hash)
        .bind(codigo_regla(regla))
        .execute(db)
        .await?;
    Ok(())
}

pub async fn exenciones(db: &SqlitePool) -> Result<HashSet<(String, TipoRegla)>, sqlx::Error> {
    let filas: Vec<(String, String)> = sqlx::query_as("SELECT hash, regla FROM exenciones")
        .fetch_all(db)
        .await?;
    filas
        .into_iter()
        .map(|(h, r)| Ok((h, regla_de(&r)?)))
        .collect()
}

/// Qué pasó con un torrent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evento {
    /// Una regla lo eligió: empieza la cuenta atrás.
    Aviso,
    /// «No borrar».
    Cancelado,
    /// Dejó de cumplir la regla antes de vencer.
    Retirado,
    /// Venció en modo «solo aviso».
    SeHabriaBorrado,
    /// Borrado con sus ficheros.
    Borrado,
    /// Quitado de qBittorrent sin borrar los ficheros.
    Quitado,
    /// Parado por la pausa automática.
    Pausado,
    /// Se deshizo un «No borrar».
    ExencionQuitada,
    /// Cambio entre «solo aviso» y «borrado real».
    ModoCambiado,
    /// Se aplicó un preset: normas y reglas nuevas.
    PresetAplicado,
    /// Añadido a qBittorrent desde semillero.
    Anadido,
}

impl Evento {
    fn codigo(self) -> &'static str {
        match self {
            Evento::Aviso => "aviso",
            Evento::Cancelado => "cancelado",
            Evento::Retirado => "retirado",
            Evento::SeHabriaBorrado => "se_habria_borrado",
            Evento::Borrado => "borrado",
            Evento::Quitado => "quitado",
            Evento::Pausado => "pausado",
            Evento::ExencionQuitada => "exencion_quitada",
            Evento::ModoCambiado => "modo_cambiado",
            Evento::PresetAplicado => "preset_aplicado",
            Evento::Anadido => "anadido",
        }
    }

    fn de(codigo: &str) -> Result<Self, sqlx::Error> {
        Ok(match codigo {
            "aviso" => Evento::Aviso,
            "cancelado" => Evento::Cancelado,
            "retirado" => Evento::Retirado,
            "se_habria_borrado" => Evento::SeHabriaBorrado,
            "borrado" => Evento::Borrado,
            "quitado" => Evento::Quitado,
            "pausado" => Evento::Pausado,
            "exencion_quitada" => Evento::ExencionQuitada,
            "modo_cambiado" => Evento::ModoCambiado,
            "preset_aplicado" => Evento::PresetAplicado,
            "anadido" => Evento::Anadido,
            otro => {
                return Err(sqlx::Error::Decode(
                    format!("evento desconocido: {otro}").into(),
                ));
            }
        })
    }
}

/// Caracteres que se guardan, como mucho, del nombre de una línea del
/// historial (el nombre del torrent lo decide quien lo creó).
pub const MAX_NOMBRE: usize = 500;
/// Caracteres que se guardan, como mucho, del detalle de una línea.
pub const MAX_DETALLE: usize = 2000;

/// Los `max` primeros caracteres de `s`.
fn acotar(s: &str, max: usize) -> &str {
    s.char_indices().nth(max).map_or(s, |(i, _)| &s[..i])
}

/// Una línea del historial.
#[derive(Clone, Debug, PartialEq)]
pub struct Apunte {
    pub ts: i64,
    pub hash: String,
    pub nombre: String,
    pub evento: Evento,
    pub regla: Option<TipoRegla>,
    pub detalle: String,
}

pub async fn apuntar(db: impl SqliteExecutor<'_>, a: &Apunte) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO historial (ts, hash, nombre, evento, regla, detalle) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(a.ts)
    .bind(&a.hash)
    .bind(acotar(&a.nombre, MAX_NOMBRE))
    .bind(a.evento.codigo())
    .bind(a.regla.map(codigo_regla))
    .bind(acotar(&a.detalle, MAX_DETALLE))
    .execute(db)
    .await?;
    Ok(())
}

/// Las `limite` líneas más recientes, de la más nueva a la más antigua.
pub async fn historial(db: &SqlitePool, limite: i64) -> Result<Vec<Apunte>, sqlx::Error> {
    type Fila = (i64, String, String, String, Option<String>, String);
    let filas: Vec<Fila> = sqlx::query_as(
        "SELECT ts, hash, nombre, evento, regla, detalle FROM historial
         ORDER BY ts DESC, id DESC LIMIT ?",
    )
    .bind(limite)
    .fetch_all(db)
    .await?;
    filas
        .into_iter()
        .map(|(ts, hash, nombre, evento, regla, detalle)| {
            Ok(Apunte {
                ts,
                hash,
                nombre,
                evento: Evento::de(&evento)?,
                regla: regla.as_deref().map(regla_de).transpose()?,
                detalle,
            })
        })
        .collect()
}

/// El último nombre con que el historial conoce al torrent (vacío si ninguno):
/// sirve cuando el torrent ya no está en qBittorrent.
pub async fn ultimo_nombre(db: &SqlitePool, hash: &str) -> Result<String, sqlx::Error> {
    let n: Option<String> = sqlx::query_scalar(
        "SELECT nombre FROM historial WHERE hash = ? AND nombre <> '' ORDER BY id DESC LIMIT 1",
    )
    .bind(hash)
    .fetch_optional(db)
    .await?;
    Ok(n.unwrap_or_default())
}

/// Guarda el contador de subida `subido` (bytes) del torrent a la hora `ts`.
pub async fn apuntar_muestra(
    db: &SqlitePool,
    hash: &str,
    ts: i64,
    subido: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT OR REPLACE INTO muestras_subida (hash, ts, subido) VALUES (?, ?, ?)")
        .bind(hash)
        .bind(ts)
        .bind(subido)
        .execute(db)
        .await?;
    Ok(())
}

/// Desde cuándo tiene el torrent su contador de subida actual: la primera
/// muestra con el valor de la última. Sin muestras, `None`. Si solo se ha
/// visto un valor, cuenta desde la primera muestra (no se sabe nada antes).
pub async fn ultima_subida_en(db: &SqlitePool, hash: &str) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT MIN(ts) FROM muestras_subida
         WHERE hash = ?1 AND ts > COALESCE(
           (SELECT MAX(ts) FROM muestras_subida
            WHERE hash = ?1 AND subido <> (
              SELECT subido FROM muestras_subida WHERE hash = ?1 ORDER BY ts DESC LIMIT 1)),
           -1)",
    )
    .bind(hash)
    .fetch_one(db)
    .await
}

/// Olvida las muestras anteriores a `antes_de`.
pub async fn olvidar_muestras(db: &SqlitePool, antes_de: i64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM muestras_subida WHERE ts < ?")
        .bind(antes_de)
        .execute(db)
        .await?;
    Ok(())
}

/// Olvida las muestras de los torrents que no están en `presentes` y cuya
/// muestra más nueva es anterior a `antes_de`: una lista vacía o a medias
/// (qBittorrent arrancando) no borra el historial de inactividad.
pub async fn olvidar_muestras_salvo(
    db: &SqlitePool,
    presentes: &[String],
    antes_de: i64,
) -> Result<(), sqlx::Error> {
    let viejos: Vec<String> =
        sqlx::query_scalar("SELECT hash FROM muestras_subida GROUP BY hash HAVING MAX(ts) < ?")
            .bind(antes_de)
            .fetch_all(db)
            .await?;
    for h in viejos.iter().filter(|h| !presentes.contains(h)) {
        sqlx::query("DELETE FROM muestras_subida WHERE hash = ?")
            .bind(h)
            .execute(db)
            .await?;
    }
    Ok(())
}

/// Quita el aviso del torrent, lo causara la regla que fuera.
pub async fn quitar_avisos_de(db: &SqlitePool, hash: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM avisos WHERE hash = ?")
        .bind(hash)
        .execute(db)
        .await?;
    Ok(())
}

/// Borrado pedido a qBittorrent sin confirmar todavía.
#[derive(Clone, Debug, PartialEq)]
pub struct BorradoPedido {
    pub hash: String,
    pub regla: TipoRegla,
    pub pedido_en: i64,
    /// El torrent al pedirlo, en JSON de `torrents/info`.
    pub torrent: String,
}

/// Apunta, antes de pedirlo, que se va a borrar el torrent: si qBittorrent
/// no contesta claro, la pasada siguiente sabrá que pudo borrarse.
pub async fn pedir_borrado(db: &SqlitePool, b: &BorradoPedido) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT OR REPLACE INTO borrados_pedidos (hash, regla, pedido_en, torrent)
         VALUES (?, ?, ?, ?)",
    )
    .bind(&b.hash)
    .bind(codigo_regla(b.regla))
    .bind(b.pedido_en)
    .bind(&b.torrent)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn borrados_pedidos(db: &SqlitePool) -> Result<Vec<BorradoPedido>, sqlx::Error> {
    let filas: Vec<(String, String, i64, String)> = sqlx::query_as(
        "SELECT hash, regla, pedido_en, torrent FROM borrados_pedidos ORDER BY pedido_en, hash",
    )
    .fetch_all(db)
    .await?;
    filas
        .into_iter()
        .map(|(hash, regla, pedido_en, torrent)| {
            Ok(BorradoPedido {
                hash,
                regla: regla_de(&regla)?,
                pedido_en,
                torrent,
            })
        })
        .collect()
}

/// El borrado pedido ya se resolvió (se borró o no hacía falta).
pub async fn olvidar_borrado_pedido(db: &SqlitePool, hash: &str) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM borrados_pedidos WHERE hash = ?")
        .bind(hash)
        .execute(db)
        .await?;
    Ok(())
}
