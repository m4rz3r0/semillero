//! Tarea horaria: pausa, avisos, «solo aviso», borrado e historial.
//!
//! Toda la lógica está en [`pasada`], que recibe la hora, el estado de la red
//! y la medida de los discos: se prueba sin reloj, discos ni fichero de estado de
//! verdad. [`vigilar`] solo la llama cada hora y cuenta el tiempo caído.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use qbt::TorrentQbt;
use reglas::{Aviso, Disco, JuegoNormas, NormasTracker, ReglasCategoria, TipoRegla, Torrent};
use sqlx::SqlitePool;

use crate::db::{self, Apunte, BorradoPedido, Evento, Modo};
use crate::estado::AppState;
use crate::estado_red::EstadoRed;
use crate::mediateca::Mediateca;

/// Mide el disco de una ruta (ver [`crate::disco::medir`]).
pub type MedirDisco<'a> = dyn Fn(&Path) -> Option<Disco> + Sync + 'a;

/// Qué hizo una pasada (hashes de los torrents afectados).
#[derive(Debug, Default, PartialEq)]
pub struct Informe {
    /// qBittorrent contestó; si no, no se hizo nada.
    pub qbt_ok: bool,
    pub pausados: Vec<String>,
    pub avisados: Vec<String>,
    pub retirados: Vec<String>,
    pub se_habrian_borrado: Vec<String>,
    pub borrados: Vec<String>,
    pub errores: Vec<String>,
    /// Los avisos ya llevan sumado `caido_s`: el vigía puede ponerlo a cero.
    /// Si la pasada se corta antes (qBittorrent o la base de datos), el tiempo
    /// caído se guarda para la siguiente.
    pub caido_aplicado: bool,
}

/// Una pasada a la hora `ahora` con la red en `red`; `caido_s` son los
/// segundos que la VPN o el puerto han estado caídos desde la anterior.
///
/// En orden: lee los torrents (si qBittorrent no contesta, no hace nada más),
/// guarda una muestra de subida por torrent, pausa los que llevan los días
/// de siembra y la ratio para parar de las normas del tracker, confirma los borrados pedidos sin respuesta clara, crea, retira
/// y vence avisos y, con cada aviso vencido, vuelve a leer el torrent y
/// comprueba que aún se puede borrar antes de borrarlo (o de apuntar «se
/// habría borrado»). Con la red caída nada vence.
///
/// Un aviso vencido o retirado se quita de la base de datos solo cuando su
/// final está apuntado en el historial; si algo falla, se queda para la
/// pasada siguiente y la pasada sigue con los demás.
///
/// Hay una sola pasada a la vez en todo el proceso: dos a la vez verían el
/// mismo aviso vencido y pedirían dos veces el mismo borrado.
pub async fn pasada<M: Mediateca>(
    estado: &AppState,
    mediateca: &M,
    medir: &MedirDisco<'_>,
    ahora: i64,
    red: &EstadoRed,
    caido_s: i64,
) -> Informe {
    let _turno = UNA_A_LA_VEZ.lock().await;
    let mut inf = Informe::default();
    let lista = match estado.qbt.torrents().await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!("pasada sin hacer: {e}");
            inf.errores.push(e.to_string());
            return inf;
        }
    };
    inf.qbt_ok = true;
    let mut p = Pasada {
        estado,
        mediateca,
        ahora,
        inf,
    };
    if let Err(e) = p.hacer(&lista, medir, red, caido_s).await {
        tracing::error!("pasada a medias, error de la base de datos: {e}");
        p.inf.errores.push(format!("base de datos: {e}"));
    }
    p.inf
}

/// Ver [`pasada`].
static UNA_A_LA_VEZ: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Días mínimos de muestras de subida que se guardan.
const MUESTRAS_DIAS: i64 = 30;
/// Días que se guardan las muestras de un torrent que ya no aparece en
/// qBittorrent: una lista vacía o a medias (qBittorrent arrancando) no borra
/// el historial de inactividad.
const MUESTRAS_GRACIA_DIAS: i64 = 7;

/// «24 horas», «1 hora».
fn horas(h: u32) -> String {
    if h == 1 {
        "1 hora".into()
    } else {
        format!("{h} horas")
    }
}

/// Lo que se apunta al parar un torrent por la pausa automática.
fn texto_pausa(n: &NormasTracker) -> String {
    let ratio = format!("{:?}", n.ratio_pausa);
    format!(
        "Parado: {} días sembrando y ratio {ratio} o más",
        n.siembra_dias
    )
}

/// Motivo de una regla, en palabras.
fn motivo(r: TipoRegla) -> &'static str {
    crate::api::motivo_regla(r)
}

/// El host del tracker de `t`: el que da qBittorrent o, si no da ninguno
/// (tracker caído), el último que se le vio (`vistos`, por hash).
pub fn host_de<'a>(t: &'a TorrentQbt, vistos: &'a HashMap<String, String>) -> &'a str {
    if t.tracker.is_empty() {
        vistos.get(&t.hash).map_or("", String::as_str)
    } else {
        &t.tracker
    }
}

/// Las normas de `t`: las de su tracker (ver [`host_de`]) o las de por
/// defecto.
pub async fn normas_de(db: &SqlitePool, t: &TorrentQbt) -> Result<NormasTracker, sqlx::Error> {
    let juego = db::juego_normas(db).await?;
    let vistos = db::trackers_vistos(db).await?;
    Ok(*juego.para(host_de(t, &vistos)))
}

/// El torrent de qBittorrent como lo ven las reglas (con la última subida
/// según las muestras).
pub async fn torrent_de_reglas(db: &SqlitePool, t: &TorrentQbt) -> Result<Torrent, sqlx::Error> {
    Ok(Torrent {
        hash: t.hash.clone(),
        categoria: t.category.clone(),
        estado_qbt: t.state.clone(),
        descargado: t.downloaded,
        tamano: t.total_size,
        siembra_s: t.seeding_time,
        ratio: t.ratio,
        anadido_en: t.added_on,
        ultima_subida_en: db::ultima_subida_en(db, &t.hash).await?,
    })
}

/// Cómo acaba un aviso vencido.
enum Fin {
    Resuelto,
    /// No se pudo resolver (qBittorrent falló): se reintenta en la siguiente.
    Reintentar,
}

struct Pasada<'a, M> {
    estado: &'a AppState,
    mediateca: &'a M,
    ahora: i64,
    inf: Informe,
}

impl<M: Mediateca> Pasada<'_, M> {
    fn db(&self) -> &SqlitePool {
        &self.estado.db
    }

    async fn apuntar(
        &self,
        hash: &str,
        nombre: Option<&str>,
        evento: Evento,
        regla: Option<TipoRegla>,
        detalle: String,
    ) -> Result<(), sqlx::Error> {
        let nombre = match nombre {
            Some(n) => n.to_owned(),
            None => db::ultimo_nombre(self.db(), hash).await?,
        };
        db::apuntar(
            self.db(),
            &Apunte {
                ts: self.ahora,
                hash: hash.into(),
                nombre,
                evento,
                regla,
                detalle,
            },
        )
        .await
    }

    async fn a_reglas(&self, t: &TorrentQbt) -> Result<Torrent, sqlx::Error> {
        torrent_de_reglas(self.db(), t).await
    }

    async fn hacer(
        &mut self,
        lista: &[TorrentQbt],
        medir: &MedirDisco<'_>,
        red: &EstadoRed,
        caido_s: i64,
    ) -> Result<(), sqlx::Error> {
        let reglas_cat = db::reglas(self.db()).await?;

        // El tracker de cada torrent, y con él sus normas.
        let presentes: Vec<(&str, &str)> = lista
            .iter()
            .map(|t| (t.hash.as_str(), t.tracker.as_str()))
            .collect();
        db::recordar_trackers(self.db(), &presentes, self.ahora).await?;
        db::olvidar_trackers(self.db(), self.ahora - MUESTRAS_GRACIA_DIAS * 86_400).await?;
        let juego = db::juego_normas(self.db()).await?;
        let vistos = db::trackers_vistos(self.db()).await?;
        let normas_de: HashMap<String, NormasTracker> = lista
            .iter()
            .map(|t| (t.hash.clone(), *juego.para(host_de(t, &vistos))))
            .collect();
        let normas = |h: &str| normas_de.get(h).unwrap_or(&juego.defecto);

        // Muestras de subida (para la inactividad).
        for t in lista {
            db::apuntar_muestra(self.db(), &t.hash, self.ahora, t.uploaded).await?;
        }
        let dias = reglas_cat
            .values()
            .filter_map(|r| r.inactividad_dias)
            .map(i64::from)
            .max()
            .unwrap_or(0)
            .max(MUESTRAS_DIAS);
        db::olvidar_muestras(self.db(), self.ahora - (dias + 1) * 86_400).await?;
        let hashes: Vec<String> = lista.iter().map(|t| t.hash.clone()).collect();
        db::olvidar_muestras_salvo(
            self.db(),
            &hashes,
            self.ahora - MUESTRAS_GRACIA_DIAS * 86_400,
        )
        .await?;

        let mut torrents = Vec::with_capacity(lista.len());
        for t in lista {
            torrents.push(self.a_reglas(t).await?);
        }
        let nombre = |h: &str| lista.iter().find(|t| t.hash == h).map(|t| t.name.clone());

        // Pausa: parar nunca borra.
        let pausar = reglas::a_pausar_con(&torrents, |t| normas(&t.hash));
        if !pausar.is_empty() {
            match self.estado.qbt.parar(&pausar).await {
                Ok(()) => {
                    for h in &pausar {
                        self.apuntar(
                            h,
                            nombre(h).as_deref(),
                            Evento::Pausado,
                            None,
                            texto_pausa(normas(h)),
                        )
                        .await?;
                    }
                    self.inf.pausados = pausar;
                }
                Err(e) => self.inf.errores.push(format!("no se pudo pausar: {e}")),
            }
        }

        // Borrados pedidos sin respuesta clara, antes de leer los avisos: el
        // aviso de uno confirmado ya no está.
        self.revisar_borrados_pedidos(lista).await?;
        let pedidos: HashSet<String> = db::borrados_pedidos(self.db())
            .await?
            .into_iter()
            .map(|b| b.hash)
            .collect();

        // Avisos.
        let discos = medir_discos(lista, &reglas_cat, medir);
        let exenciones = db::exenciones(self.db()).await?;
        let cands = reglas::candidatos_con(
            &torrents,
            |t| normas(&t.hash),
            &reglas_cat,
            &discos,
            &exenciones,
            self.ahora,
        );
        let avisos = db::avisos(self.db()).await?;
        let paso = reglas::avanzar_con(&avisos, &cands, self.ahora, caido_s, red.red_ok(), normas);
        // Primero lo que no depende de qBittorrent, por si la pasada se corta.
        // Vencidos y retirados se quedan guardados hasta apuntar su final.
        let guardar: Vec<Aviso> = paso
            .vigentes
            .iter()
            .chain(&paso.vencidos)
            .chain(&paso.retirados)
            .cloned()
            .collect();
        db::guardar_avisos(self.db(), &guardar).await?;
        self.inf.caido_aplicado = true;
        for a in &paso.nuevos {
            self.apuntar(
                &a.hash,
                nombre(&a.hash).as_deref(),
                Evento::Aviso,
                Some(a.regla),
                format!(
                    "Se borrará en {}: {}",
                    horas(normas(&a.hash).aviso_horas),
                    motivo(a.regla)
                ),
            )
            .await?;
            self.inf.avisados.push(a.hash.clone());
        }
        for a in &paso.retirados {
            // Con un borrado pedido sin confirmar y el torrent fuera de la
            // lista, lo resuelve ese borrado. Si sigue en la lista, el borrado
            // no se hizo: se retira como cualquiera (y `retirar` olvida el
            // pedido), para que el aviso no quede atascado ni venza en el acto
            // si la regla vuelve a cumplirse.
            if pedidos.contains(&a.hash) && !lista.iter().any(|t| t.hash == a.hash) {
                continue;
            }
            let fin = self
                .retirar(a, nombre(&a.hash).as_deref(), "Ya no cumple la regla")
                .await
                .map(|()| Fin::Resuelto);
            self.cerrar(a, fin).await;
        }
        // `avanzar` no vence nada con la red caída; aun así, sin red no se
        // resuelve ningún aviso (siguen guardados).
        if !red.red_ok() {
            return Ok(());
        }
        for a in &paso.vencidos {
            let fin = self
                .vencer(a, (&juego, &vistos), &reglas_cat, &discos)
                .await;
            self.cerrar(a, fin).await;
        }
        Ok(())
    }

    /// Quita el aviso si su final quedó apuntado; si no, se queda guardado
    /// para la pasada siguiente. Un error no corta los demás avisos.
    async fn cerrar(&mut self, a: &Aviso, fin: Result<Fin, sqlx::Error>) {
        let r = match fin {
            Ok(Fin::Resuelto) => db::quitar_aviso(self.db(), &a.hash, a.regla)
                .await
                .map(|_| ()),
            Ok(Fin::Reintentar) => Ok(()),
            Err(e) => Err(e),
        };
        if let Err(e) = r {
            tracing::error!(hash = %a.hash, "aviso sin resolver, se reintentará: {e}");
            self.inf
                .errores
                .push(format!("base de datos, aviso de {}: {e}", a.hash));
        }
    }

    /// Borrados pedidos cuya respuesta no llegó clara: si el torrent ya no
    /// está en qBittorrent, se borró; si está y ya no tiene aviso (un «No
    /// borrar»), no se borró y no queda nada pendiente.
    async fn revisar_borrados_pedidos(&mut self, lista: &[TorrentQbt]) -> Result<(), sqlx::Error> {
        let con_aviso: HashSet<String> = db::avisos(self.db())
            .await?
            .into_iter()
            .map(|a| a.hash)
            .collect();
        for b in db::borrados_pedidos(self.db()).await? {
            if lista.iter().any(|t| t.hash == b.hash) {
                if !con_aviso.contains(&b.hash)
                    && let Err(e) = db::olvidar_borrado_pedido(self.db(), &b.hash).await
                {
                    self.inf.errores.push(format!("base de datos: {e}"));
                }
                continue;
            }
            // La lista puede venir a medias: se pregunta por el torrent.
            match self.estado.qbt.obtener(&b.hash).await {
                Ok(None) => {
                    self.confirmar_borrado(&b).await;
                }
                Ok(Some(_)) => {}
                Err(e) => self
                    .inf
                    .errores
                    .push(format!("no se pudo comprobar {}: {e}", b.hash)),
            }
        }
        Ok(())
    }

    /// El borrado pedido se hizo: se apunta, se avisa a la mediateca y se
    /// quita su aviso. Devuelve si quedó apuntado.
    async fn confirmar_borrado(&mut self, b: &BorradoPedido) -> bool {
        let t = match serde_json::from_str::<TorrentQbt>(&b.torrent) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(hash = %b.hash, "borrado pedido ilegible: {e}");
                torrent_minimo(&b.hash, db::ultimo_nombre(self.db(), &b.hash).await)
            }
        };
        self.inf.borrados.push(b.hash.clone());
        if !self
            .apuntar_borrado(
                &t,
                b.regla,
                "Borrado con sus ficheros (confirmado en la pasada siguiente: \
                 qBittorrent no contestó claro al pedirlo)",
            )
            .await
        {
            return false;
        }
        if let Err(e) = db::quitar_avisos_de(self.db(), &b.hash).await {
            self.inf.errores.push(format!("base de datos: {e}"));
        }
        true
    }

    /// Tras borrar `t`: se pide a la mediateca que lo olvide y se apunta
    /// «Borrado» (con un reintento). Si no se puede apuntar, queda el borrado
    /// pedido y la pasada siguiente lo confirma. Devuelve si quedó apuntado.
    async fn apuntar_borrado(&mut self, t: &TorrentQbt, regla: TipoRegla, que: &str) -> bool {
        let mut detalle = format!("{que}: {}", motivo(regla));
        if let Err(e) = self.mediateca.olvidar(t).await {
            tracing::warn!("mediateca: {e}");
            detalle.push_str(&format!(". No se pudo quitar de la mediateca: {e}"));
        }
        let mut r = Err(sqlx::Error::PoolClosed);
        for _ in 0..2 {
            r = self
                .apuntar(
                    &t.hash,
                    Some(&t.name),
                    Evento::Borrado,
                    Some(regla),
                    detalle.clone(),
                )
                .await;
            if r.is_ok() {
                break;
            }
        }
        if r.is_ok() {
            r = db::olvidar_borrado_pedido(self.db(), &t.hash).await;
        }
        match r {
            Ok(()) => true,
            Err(e) => {
                tracing::error!(
                    hash = %t.hash,
                    "BORRADO SIN APUNTAR en el historial ({e}); se reintentará en la pasada siguiente"
                );
                self.inf.errores.push(format!(
                    "borrado {} sin apuntar en el historial: {e}",
                    t.hash
                ));
                false
            }
        }
    }

    async fn retirar(
        &mut self,
        a: &Aviso,
        nombre: Option<&str>,
        detalle: &str,
    ) -> Result<(), sqlx::Error> {
        self.apuntar(
            &a.hash,
            nombre,
            Evento::Retirado,
            Some(a.regla),
            detalle.into(),
        )
        .await?;
        // Si había un borrado pedido, no se hizo: el torrent sigue o lo quitó otro.
        db::olvidar_borrado_pedido(self.db(), &a.hash).await?;
        self.inf.retirados.push(a.hash.clone());
        Ok(())
    }

    /// Aviso vencido: se vuelve a leer el torrent, a clasificar y a comprobar
    /// que alguna regla aún lo elige; solo entonces se borra (o se apunta).
    async fn vencer(
        &mut self,
        a: &Aviso,
        (juego, vistos): (&JuegoNormas, &HashMap<String, String>),
        reglas_cat: &HashMap<String, ReglasCategoria>,
        discos: &HashMap<String, Disco>,
    ) -> Result<Fin, sqlx::Error> {
        let t = match self.estado.qbt.obtener(&a.hash).await {
            Ok(Some(t)) => t,
            Ok(None) => {
                let pedido = db::borrados_pedidos(self.db())
                    .await?
                    .into_iter()
                    .find(|b| b.hash == a.hash);
                if let Some(b) = pedido {
                    return Ok(if self.confirmar_borrado(&b).await {
                        Fin::Resuelto
                    } else {
                        Fin::Reintentar
                    });
                }
                self.retirar(a, None, "Ya no está en qBittorrent").await?;
                return Ok(Fin::Resuelto);
            }
            Err(e) => {
                self.inf
                    .errores
                    .push(format!("no se pudo comprobar {}: {e}", a.hash));
                return Ok(Fin::Reintentar);
            }
        };
        let rt = self.a_reglas(&t).await?;
        let normas = juego.para(host_de(&t, vistos));
        // Exenciones recién leídas: un «No borrar» de última hora cuenta. Y
        // solo vale la regla del aviso que se enseñó: las demás se dan por
        // exentas aquí, para que ninguna borre sin sus propias 24 horas.
        let mut exenciones = db::exenciones(self.db()).await?;
        for r in [
            TipoRegla::Ratio,
            TipoRegla::Siembra,
            TipoRegla::Inactividad,
            TipoRegla::Disco,
        ] {
            if r != a.regla {
                exenciones.insert((a.hash.clone(), r));
            }
        }
        let regla = reglas::candidatos(
            std::slice::from_ref(&rt),
            normas,
            reglas_cat,
            discos,
            &exenciones,
            self.ahora,
        )
        .first()
        .map(|c| c.regla);
        let Some(regla) = regla else {
            self.retirar(
                a,
                Some(&t.name),
                "Al ir a borrarlo ya no cumplía la regla del aviso",
            )
            .await?;
            return Ok(Fin::Resuelto);
        };
        match db::modo(self.db()).await? {
            Modo::SoloAviso => {
                self.apuntar(
                    &t.hash,
                    Some(&t.name),
                    Evento::SeHabriaBorrado,
                    Some(regla),
                    format!(
                        "Se habría recogido: el torrent, sus ficheros y sus datos de Jellyfin ({}); \
                         está activado «Solo aviso»",
                        motivo(regla)
                    ),
                )
                .await?;
                self.inf.se_habrian_borrado.push(t.hash.clone());
                Ok(Fin::Resuelto)
            }
            Modo::BorradoReal => {
                // Antes de pedirlo: un error o un plazo agotado no dicen si
                // qBittorrent borró, y la pasada siguiente lo comprobará.
                db::pedir_borrado(
                    self.db(),
                    &BorradoPedido {
                        hash: t.hash.clone(),
                        regla,
                        pedido_en: self.ahora,
                        torrent: a_json(&t),
                    },
                )
                .await?;
                if let Err(e) = self.estado.qbt.borrar(&t.hash, true).await {
                    self.inf
                        .errores
                        .push(format!("no se pudo borrar {}: {e}", t.hash));
                    return Ok(Fin::Reintentar);
                }
                self.inf.borrados.push(t.hash.clone());
                Ok(
                    if self
                        .apuntar_borrado(&t, regla, "Borrado con sus ficheros")
                        .await
                    {
                        Fin::Resuelto
                    } else {
                        Fin::Reintentar
                    },
                )
            }
        }
    }
}

/// El torrent en JSON de `torrents/info`, para [`BorradoPedido::torrent`].
fn a_json(t: &TorrentQbt) -> String {
    serde_json::json!({
        "hash": t.hash, "name": t.name, "category": t.category, "state": t.state,
        "downloaded": t.downloaded, "total_size": t.total_size, "uploaded": t.uploaded,
        "seeding_time": t.seeding_time, "ratio": t.ratio, "added_on": t.added_on,
        "save_path": t.save_path, "upspeed": t.upspeed, "dlspeed": t.dlspeed,
        "progress": t.progress,
    })
    .to_string()
}

/// Lo poco que se sabe de un torrent ya borrado cuyo JSON no se pudo leer.
fn torrent_minimo(hash: &str, nombre: Result<String, sqlx::Error>) -> TorrentQbt {
    TorrentQbt {
        hash: hash.into(),
        name: nombre.unwrap_or_default(),
        category: String::new(),
        state: String::new(),
        downloaded: 0,
        total_size: 0,
        uploaded: 0,
        seeding_time: 0,
        ratio: 0.0,
        added_on: 0,
        save_path: String::new(),
        upspeed: 0,
        dlspeed: 0,
        progress: 0.0,
        tracker: String::new(),
    }
}

/// Disco de cada categoría con la regla de espacio activa: el del directorio
/// de descarga de sus torrents (el primero por hash, para que sea estable).
fn medir_discos(
    lista: &[TorrentQbt],
    reglas_cat: &HashMap<String, ReglasCategoria>,
    medir: &MedirDisco<'_>,
) -> HashMap<String, Disco> {
    reglas_cat
        .iter()
        .filter(|(_, r)| r.disco_libre_pct.is_some())
        .filter_map(|(cat, _)| {
            let ruta = ruta_de_categoria(lista, cat)?;
            Some((cat.clone(), medir(Path::new(ruta))?))
        })
        .collect()
}

/// Directorio de descarga que representa a la categoría `cat`: el del
/// primero de sus torrents por hash, para que sea estable.
pub fn ruta_de_categoria<'a>(lista: &'a [TorrentQbt], cat: &str) -> Option<&'a str> {
    lista
        .iter()
        .filter(|t| t.category == cat && !t.save_path.is_empty())
        .min_by(|a, b| a.hash.cmp(&b.hash))
        .map(|t| t.save_path.as_str())
}

/// Por qué no se quitó un torrent a mano.
#[derive(Debug)]
pub enum NoQuitado {
    /// Hash no válido.
    HashMalo,
    /// Ya no está en qBittorrent.
    NoEsta,
    /// Obligado o En espera: aún tiene que sembrar (según sus normas).
    Protegido(Proteccion),
    Qbt(qbt::Error),
    Db(sqlx::Error),
}

/// Por qué un torrent aún no se puede quitar: su clasificación y las normas
/// con que se clasificó (las de su tracker).
#[derive(Debug)]
pub struct Proteccion {
    pub clasificacion: reglas::Clasificacion,
    pub normas: NormasTracker,
}

impl From<sqlx::Error> for NoQuitado {
    fn from(e: sqlx::Error) -> Self {
        NoQuitado::Db(e)
    }
}

/// Quita un torrent a mano, con sus ficheros si `ficheros`, con la misma
/// seguridad que la tarea horaria: en el turno de las pasadas, vuelve a leer
/// el torrent y a clasificarlo, y solo pide el borrado si es Cumplido o
/// Libre. Después, con ficheros, pide a la mediateca que lo olvide; apunta
/// «Borrado» o «Quitado» en el historial y quita sus avisos.
///
/// La reautenticación la comprueba quien llama.
pub async fn quitar_a_mano<M: Mediateca>(
    estado: &AppState,
    mediateca: &M,
    hash: &str,
    ficheros: bool,
    ahora: i64,
) -> Result<(), NoQuitado> {
    if !qbt::hash_valido(hash) {
        return Err(NoQuitado::HashMalo);
    }
    // Ni a la vez que una pasada (que puede estar borrándolo) ni que otro
    // borrado a mano.
    let _turno = UNA_A_LA_VEZ.lock().await;
    let db = &estado.db;
    let t = match estado.qbt.obtener(hash).await {
        Ok(Some(t)) => t,
        Ok(None) => return Err(NoQuitado::NoEsta),
        Err(e) => return Err(NoQuitado::Qbt(e)),
    };
    let normas = normas_de(db, &t).await?;
    let c = reglas::clasificar(&torrent_de_reglas(db, &t).await?, &normas);
    if !c.borrable {
        return Err(NoQuitado::Protegido(Proteccion {
            clasificacion: c,
            normas,
        }));
    }
    estado
        .qbt
        .borrar(&t.hash, ficheros)
        .await
        .map_err(NoQuitado::Qbt)?;
    let (evento, mut detalle) = if ficheros {
        (
            Evento::Borrado,
            "Borrado a mano con sus ficheros".to_owned(),
        )
    } else {
        (
            Evento::Quitado,
            "Quitado a mano de qBittorrent; los ficheros siguen en el disco".to_owned(),
        )
    };
    if ficheros && let Err(e) = mediateca.olvidar(&t).await {
        tracing::warn!("mediateca: {e}");
        detalle.push_str(&format!(". No se pudo quitar de la mediateca: {e}"));
    }
    let apunte = Apunte {
        ts: ahora,
        hash: t.hash.clone(),
        nombre: t.name.clone(),
        evento,
        regla: None,
        detalle,
    };
    // Ya está borrado: un fallo al apuntarlo no lo deshace, se registra.
    let r = async {
        db::apuntar(db, &apunte).await?;
        db::quitar_avisos_de(db, &t.hash).await?;
        db::olvidar_borrado_pedido(db, &t.hash).await
    }
    .await;
    if let Err(e) = r {
        tracing::error!(hash = %t.hash, "quitado a mano SIN APUNTAR en el historial: {e}");
    }
    Ok(())
}

/// «No borrar»: exime al torrent de `regla` y quita su aviso si lo causó esa
/// regla.
pub async fn no_borrar(
    db: &SqlitePool,
    hash: &str,
    regla: TipoRegla,
    ahora: i64,
) -> Result<(), sqlx::Error> {
    db::eximir(db, hash, regla, ahora).await?;
    db::quitar_aviso(db, hash, regla).await?;
    apuntar_ui(
        db,
        hash,
        Evento::Cancelado,
        Some(regla),
        "«No recoger»: esta regla ya no lo recogerá",
        ahora,
    )
    .await
}

/// Deshace un «No borrar».
pub async fn quitar_exencion(
    db: &SqlitePool,
    hash: &str,
    regla: TipoRegla,
    ahora: i64,
) -> Result<(), sqlx::Error> {
    db::quitar_exencion(db, hash, regla).await?;
    apuntar_ui(
        db,
        hash,
        Evento::ExencionQuitada,
        Some(regla),
        "Se deshizo «No recoger»: esta regla vuelve a contar",
        ahora,
    )
    .await
}

/// Cambia entre «solo aviso» y «borrado real» y lo apunta. Pasar al borrado
/// real exige haber vuelto a escribir la contraseña: lo comprueba quien llama.
pub async fn cambiar_modo(db: &SqlitePool, modo: Modo, ahora: i64) -> Result<(), sqlx::Error> {
    if db::modo(db).await? == modo {
        return Ok(());
    }
    db::fijar_modo(db, modo).await?;
    let detalle = match modo {
        Modo::SoloAviso => "Solo aviso",
        Modo::BorradoReal => "Borrado real",
    };
    apuntar_ui(db, "", Evento::ModoCambiado, None, detalle, ahora).await
}

async fn apuntar_ui(
    db: &SqlitePool,
    hash: &str,
    evento: Evento,
    regla: Option<TipoRegla>,
    detalle: &str,
    ahora: i64,
) -> Result<(), sqlx::Error> {
    let nombre = if hash.is_empty() {
        String::new()
    } else {
        db::ultimo_nombre(db, hash).await?
    };
    db::apuntar(
        db,
        &Apunte {
            ts: ahora,
            hash: hash.into(),
            nombre,
            evento,
            regla,
            detalle: detalle.into(),
        },
    )
    .await
}

/// Cada cuánto se mira el estado de la red.
pub const COMPROBAR_S: i64 = 60;
/// Cada cuánto hay pasada.
pub const PASADA_S: i64 = 3600;

/// Reloj del vigilante: suma el tiempo caído y dice cuándo toca pasada.
#[derive(Debug, Default)]
pub struct Vigia {
    caido_s: i64,
    desde_pasada_s: i64,
    hecha_alguna: bool,
}

impl Vigia {
    /// Han pasado `paso_s` segundos y ahora la red está así: ¿toca pasada?
    /// La primera, en la primera comprobación.
    pub fn tic(&mut self, paso_s: i64, red_ok: bool) -> bool {
        if !red_ok {
            self.caido_s += paso_s;
        }
        self.desde_pasada_s += paso_s;
        !self.hecha_alguna || self.desde_pasada_s >= PASADA_S
    }

    /// Segundos caídos desde la última pasada que los aplicó.
    pub fn caido_s(&self) -> i64 {
        self.caido_s
    }

    /// Pasada hecha; si qBittorrent contestó, el tiempo caído ya se aplicó.
    pub fn hecha(&mut self, aplicada: bool) {
        self.hecha_alguna = true;
        self.desde_pasada_s = 0;
        if aplicada {
            self.caido_s = 0;
        }
    }
}

/// Hora Unix actual, en segundos.
pub fn ahora_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Bucle de la tarea horaria: mira el estado de la red cada minuto, suma el tiempo con
/// la red caída y hace una pasada al poco de arrancar y luego cada hora.
pub async fn vigilar<M: Mediateca>(estado: AppState, mediateca: M) {
    use std::time::Duration;
    use tokio::time::{Instant, MissedTickBehavior, interval_at};

    let cada = Duration::from_secs(COMPROBAR_S as u64);
    let mut reloj = interval_at(Instant::now() + cada, cada);
    reloj.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut vigia = Vigia::default();
    let mut antes = Instant::now();
    loop {
        reloj.tick().await;
        let paso_s = antes.elapsed().as_secs() as i64;
        antes = Instant::now();
        let ahora = ahora_unix();
        let red = crate::estado_red::leer(estado.cfg.estado_red.as_deref(), ahora as f64);
        if !vigia.tic(paso_s, red.red_ok()) {
            continue;
        }
        let inf = pasada(
            &estado,
            &mediateca,
            &crate::disco::medir,
            ahora,
            &red,
            vigia.caido_s(),
        )
        .await;
        tracing::info!(
            qbt = inf.qbt_ok,
            pausados = inf.pausados.len(),
            avisados = inf.avisados.len(),
            retirados = inf.retirados.len(),
            se_habrian_borrado = inf.se_habrian_borrado.len(),
            borrados = inf.borrados.len(),
            errores = inf.errores.len(),
            "pasada horaria"
        );
        vigia.hecha(inf.caido_aplicado);
    }
}
