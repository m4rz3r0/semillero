//! Lo que la interfaz pide y manda al servidor: los datos de cada página y
//! las acciones, como server functions bajo `/api` (tras el guardián: sesión
//! y token CSRF). En el navegador viajan con [`ClienteCsrf`].
//!
//! Los datos ya van en palabras de la interfaz: ningún nombre ni código de
//! la API de qBittorrent llega al navegador.
//!
//! Sin features: los tipos y las declaraciones compilan igual para el
//! servidor y para el navegador; lo que hacen está en `api/servidor.rs`.

use leptos::prelude::*;
use leptos::server_fn::codec::Json;
use reglas::{Estado, NormasTracker, ReglasCategoria, TipoRegla};
use serde::{Deserialize, Serialize};

use crate::cliente::ClienteCsrf;

#[cfg(feature = "ssr")]
mod servidor;

#[cfg(feature = "ssr")]
pub use servidor::descargar_preset;

/// Código de error cuando hace falta volver a escribir la contraseña (la
/// ventana de 5 minutos está cerrada).
pub const ERROR_REAUTH: &str = "falta-contrasena";

/// Dónde se descarga un preset (`?id=N`): un `GET` normal, con la sesión,
/// que devuelve el archivo para guardarlo.
pub const RUTA_DESCARGA: &str = "/api/descargar_preset";

/// El id del preset incluido (no está en la base de datos).
pub const ID_INCLUIDO: i64 = 0;

/// Presets guardados como mucho.
pub const MAX_PRESETS: i64 = 50;

/// Categorías con su tarjeta de reglas, en orden: código y nombre.
pub const CATEGORIAS: [(&str, &str); 3] = [
    ("peliculas", "Películas"),
    ("series", "Series"),
    ("", "Sin categoría"),
];

/// Nombre de una categoría para la interfaz.
pub fn nombre_categoria(codigo: &str) -> String {
    CATEGORIAS
        .iter()
        .find(|(c, _)| *c == codigo)
        .map(|(_, n)| (*n).to_owned())
        .unwrap_or_else(|| codigo.to_owned())
}

/// Qué hace el torrent ahora, en palabras.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Actividad {
    Sembrando,
    Descargando,
    Parado,
    Comprobando,
    EnCola,
    ConErrores,
    Desconocida,
}

impl Actividad {
    /// Traduce el estado de qBittorrent.
    pub fn de_qbt(estado: &str) -> Self {
        match estado {
            "uploading" | "stalledUP" | "forcedUP" => Actividad::Sembrando,
            "downloading" | "stalledDL" | "forcedDL" | "metaDL" | "forcedMetaDL" => {
                Actividad::Descargando
            }
            "stoppedUP" | "stoppedDL" | "pausedUP" | "pausedDL" => Actividad::Parado,
            "checkingUP" | "checkingDL" | "checkingResumeData" | "moving" | "allocating" => {
                Actividad::Comprobando
            }
            "queuedUP" | "queuedDL" => Actividad::EnCola,
            "error" | "missingFiles" => Actividad::ConErrores,
            _ => Actividad::Desconocida,
        }
    }

    pub fn texto(self) -> &'static str {
        match self {
            Actividad::Sembrando => "Sembrando",
            Actividad::Descargando => "Descargando",
            Actividad::Parado => "Parado",
            Actividad::Comprobando => "Comprobando",
            Actividad::EnCola => "En cola",
            Actividad::ConErrores => "Con errores",
            Actividad::Desconocida => "Sin datos",
        }
    }

    /// ¿Se puede reanudar (si no, se puede parar)?
    pub fn parado(self) -> bool {
        self == Actividad::Parado
    }
}

/// Un torrent en la pantalla principal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FichaTorrent {
    pub hash: String,
    pub nombre: String,
    pub categoria: String,
    pub estado: Estado,
    pub actividad: Actividad,
    pub ratio: f64,
    pub siembra_s: i64,
    /// Siembra que le falta a un recién sembrado o a uno que está creciendo.
    pub falta_s: i64,
    pub tamano: i64,
    /// Regla del aviso pendiente, si lo tiene.
    pub aviso: Option<TipoRegla>,
    /// Reglas con «No recoger» para este torrent (ya no lo recogen).
    pub exenciones: Vec<TipoRegla>,
}

/// Espacio libre del disco de una categoría.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FichaDisco {
    pub categoria: String,
    pub libre: i64,
    pub total: i64,
    /// Por debajo del mínimo (el de su regla de espacio o, sin ella, 10 %).
    pub bajo: bool,
}

/// VPN, puerto y discos.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Franja {
    /// Hay fichero de estado de la red; si no, `vpn` y `puerto` no cuentan.
    pub vigilada: bool,
    pub vpn: bool,
    pub puerto: bool,
    pub qbt: bool,
    pub discos: Vec<FichaDisco>,
}

impl Franja {
    /// La red está bien: las cuentas atrás avanzan.
    pub fn red_ok(&self) -> bool {
        !self.vigilada || (self.vpn && self.puerto)
    }
}

/// Un aviso de borrado pendiente.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FichaAviso {
    pub hash: String,
    pub nombre: String,
    pub regla: TipoRegla,
    pub vence_en: i64,
}

/// La pantalla principal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DatosPortada {
    pub ahora: i64,
    /// Las normas del tracker: los textos de lo que falta salen de ellas.
    pub normas: NormasTracker,
    pub franja: Franja,
    pub modo_real: bool,
    pub torrents: Vec<FichaTorrent>,
    pub avisos: Vec<FichaAviso>,
}

/// Reglas de una categoría.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TarjetaReglas {
    /// Código (`""` es sin categoría).
    pub categoria: String,
    pub reglas: ReglasCategoria,
}

/// Un «No recoger» vigente.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FichaExencion {
    pub hash: String,
    pub nombre: String,
    pub regla: TipoRegla,
}

/// Un preset en la lista.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FichaPreset {
    pub id: i64,
    pub nombre: String,
    pub descripcion: String,
    /// El incluido: se aplica y se descarga, pero no se cambia ni se elimina.
    pub incluido: bool,
}

/// La página de reglas.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DatosReglas {
    pub modo_real: bool,
    pub normas: NormasTracker,
    pub tarjetas: Vec<TarjetaReglas>,
    pub exenciones: Vec<FichaExencion>,
    /// El incluido primero; luego los guardados, por nombre.
    pub presets: Vec<FichaPreset>,
}

/// Qué pasó, en el historial.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Suceso {
    Aviso,
    Cancelado,
    Retirado,
    SeHabriaBorrado,
    Borrado,
    Quitado,
    Pausado,
    ExencionQuitada,
    ModoCambiado,
    PresetAplicado,
}

impl Suceso {
    pub fn texto(self) -> &'static str {
        match self {
            Suceso::Aviso => "Aviso de recogida",
            Suceso::Cancelado => "No recoger",
            Suceso::Retirado => "Aviso retirado",
            Suceso::SeHabriaBorrado => "Se habría recogido",
            Suceso::Borrado => "Recogido",
            Suceso::Quitado => "Quitado sin borrar los ficheros",
            Suceso::Pausado => "Parado",
            Suceso::ExencionQuitada => "Vuelve a contar",
            Suceso::ModoCambiado => "Cambio de modo",
            Suceso::PresetAplicado => "Preset aplicado",
        }
    }
}

/// Una línea del historial.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineaHistorial {
    pub ts: i64,
    /// Vacío en los sucesos generales (cambios de modo).
    pub nombre: String,
    pub general: bool,
    pub suceso: Suceso,
    pub regla: Option<TipoRegla>,
    pub detalle: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DatosHistorial {
    pub ahora: i64,
    pub lineas: Vec<LineaHistorial>,
}

/// Nombre de una regla para la interfaz.
pub fn nombre_regla(r: TipoRegla) -> &'static str {
    match r {
        TipoRegla::Ratio => "Ratio",
        TipoRegla::Siembra => "Tiempo sembrando",
        TipoRegla::Inactividad => "Sin subir nada",
        TipoRegla::Disco => "Espacio en disco",
    }
}

/// Por qué la regla elige un torrent, en una frase.
pub fn motivo_regla(r: TipoRegla) -> &'static str {
    match r {
        TipoRegla::Ratio => "ha llegado a la ratio de la regla",
        TipoRegla::Siembra => "lleva sembrando el tiempo de la regla",
        TipoRegla::Inactividad => "lleva el tiempo de la regla sin subir nada",
        TipoRegla::Disco => "queda poco espacio en el disco",
    }
}

// ---------------------------------------------------------------- server functions

/// Datos de la pantalla principal.
#[server(endpoint = "portada", input = Json, client = ClienteCsrf)]
pub async fn portada() -> Result<DatosPortada, ServerFnError> {
    servidor::portada().await
}

/// Para (`parar`) o reanuda un torrent.
#[server(endpoint = "pausar", input = Json, client = ClienteCsrf)]
pub async fn pausar(hash: String, parar: bool) -> Result<(), ServerFnError> {
    servidor::pausar(hash, parar).await
}

/// Quita un torrent de qBittorrent, con sus ficheros si `ficheros`. Solo
/// los listos para recoger y los sin arraigar, y con la contraseña escrita hace menos de 5 minutos
/// (o `clave`, que la vuelve a comprobar).
#[server(endpoint = "quitar", input = Json, client = ClienteCsrf)]
pub async fn quitar(
    hash: String,
    ficheros: bool,
    clave: Option<String>,
) -> Result<(), ServerFnError> {
    servidor::quitar(hash, ficheros, clave).await
}

/// «No recoger»: exime al torrent de la regla.
#[server(endpoint = "no_borrar", input = Json, client = ClienteCsrf)]
pub async fn no_borrar(hash: String, regla: TipoRegla) -> Result<(), ServerFnError> {
    servidor::no_borrar(hash, regla).await
}

/// Deshace un «No recoger».
#[server(endpoint = "quitar_exencion", input = Json, client = ClienteCsrf)]
pub async fn quitar_exencion(hash: String, regla: TipoRegla) -> Result<(), ServerFnError> {
    servidor::quitar_exencion(hash, regla).await
}

/// Datos de la página de reglas.
#[server(endpoint = "reglas", input = Json, client = ClienteCsrf)]
pub async fn pagina_reglas() -> Result<DatosReglas, ServerFnError> {
    servidor::pagina_reglas().await
}

/// Guarda las reglas de una categoría. Con el borrado real activado pide
/// la contraseña escrita hace menos de 5 minutos (o `clave`), como aplicar
/// un preset.
#[server(endpoint = "guardar_reglas", input = Json, client = ClienteCsrf)]
pub async fn guardar_reglas(
    categoria: String,
    reglas: ReglasCategoria,
    clave: Option<String>,
) -> Result<(), ServerFnError> {
    servidor::guardar_reglas(categoria, reglas, clave).await
}

/// Guarda las normas del tracker. Con el borrado real activado pide la
/// contraseña escrita hace menos de 5 minutos (o `clave`), como aplicar un
/// preset.
#[server(endpoint = "guardar_normas", input = Json, client = ClienteCsrf)]
pub async fn guardar_normas(
    normas: NormasTracker,
    clave: Option<String>,
) -> Result<(), ServerFnError> {
    servidor::guardar_normas(normas, clave).await
}

/// Guarda las normas y las reglas de ahora como un preset nuevo.
#[server(endpoint = "guardar_preset", input = Json, client = ClienteCsrf)]
pub async fn guardar_preset(nombre: String, descripcion: String) -> Result<(), ServerFnError> {
    servidor::guardar_preset(nombre, descripcion).await
}

/// Guarda un preset a partir del contenido de un archivo (64 KiB como
/// mucho). Si algo falla, no se guarda nada y el error dice qué.
#[server(endpoint = "subir_preset", input = Json, client = ClienteCsrf)]
pub async fn subir_preset(archivo: String) -> Result<(), ServerFnError> {
    servidor::subir_preset(archivo).await
}

/// Cambia el nombre y la descripción de un preset y, con `sobrescribir`,
/// pone en él las normas y las reglas de ahora.
#[server(endpoint = "cambiar_preset", input = Json, client = ClienteCsrf)]
pub async fn cambiar_preset(
    id: i64,
    nombre: String,
    descripcion: String,
    sobrescribir: bool,
) -> Result<(), ServerFnError> {
    servidor::cambiar_preset(id, nombre, descripcion, sobrescribir).await
}

/// Aplica un preset: sus normas y sus reglas sustituyen a las de ahora.
/// Con el borrado real activado pide la contraseña escrita hace menos de 5
/// minutos (o `clave`), como activarlo.
#[server(endpoint = "aplicar_preset", input = Json, client = ClienteCsrf)]
pub async fn aplicar_preset(id: i64, clave: Option<String>) -> Result<(), ServerFnError> {
    servidor::aplicar_preset(id, clave).await
}

/// Elimina un preset guardado (el incluido no).
#[server(endpoint = "eliminar_preset", input = Json, client = ClienteCsrf)]
pub async fn eliminar_preset(id: i64) -> Result<(), ServerFnError> {
    servidor::eliminar_preset(id).await
}

/// «Solo aviso» o «Borrado real». Pasar al borrado real pide la contraseña
/// escrita hace menos de 5 minutos (o `clave`).
#[server(endpoint = "cambiar_modo", input = Json, client = ClienteCsrf)]
pub async fn cambiar_modo(real: bool, clave: Option<String>) -> Result<(), ServerFnError> {
    servidor::cambiar_modo(real, clave).await
}

/// Las últimas líneas del historial.
#[server(endpoint = "historial", input = Json, client = ClienteCsrf)]
pub async fn historial() -> Result<DatosHistorial, ServerFnError> {
    servidor::historial().await
}
