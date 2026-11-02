//! El jardín (pantalla principal), como la pantalla de un juego: la
//! cabecera dice el estado en una frase y cuántas macetas se recogerán
//! solas pronto; debajo, de borde a borde, un bancal (uno solo a la vez)
//! con sus macetas sobre la tierra. Cuatro pestañas grandes, cada una un
//! objeto del huerto (un letrero de madera, una hoja, una piedra y un
//! sobre de semillas), cambian de bancal.
//!
//! El bancal elegido va en la dirección (`?bancal=listos`): el servidor lo
//! pinta ya elegido y los enlaces funcionan sin JavaScript. Cada maceta
//! lleva su etiqueta de herbolario y abre su ficha, con todo lo que se
//! puede hacer con ella.

use leptos::prelude::*;
use leptos_router::NavigateOptions;
use leptos_router::hooks::{use_location, use_navigate, use_query_map};
use reglas::{Estado, NormasTracker, TipoRegla};

use super::formato::{bytes, cifra, decimal, duracion, falta};
use super::maceta::{GlifoPlanta, Maceta, semilla};
use super::tiempo::Tiempo;
use super::{Acciones, Delicada, Marco, Peticion, Recarga, Saliendo, pedir_confirmacion};
use crate::api::{
    DatosPortada, FichaAviso, FichaTorrent, no_borrar, nombre_regla, pausar, portada,
    quitar_exencion,
};

/// La pestaña de un bancal: cada uno, un objeto del huerto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Forma {
    /// Un letrero de madera clavado en la tierra.
    Letrero,
    /// Una hoja.
    Hoja,
    /// Una piedra.
    Piedra,
    /// Un sobre de semillas, con el borde dentado.
    Sobre,
}

impl Forma {
    fn clase(self) -> &'static str {
        match self {
            Forma::Letrero => "letrero",
            Forma::Hoja => "hoja",
            Forma::Piedra => "piedra",
            Forma::Sobre => "sobre",
        }
    }

    /// La silueta (en una caja de 200 × 100, que se estira) y sus detalles:
    /// vetas, nervios, motas o el doblez del sobre. Hechos a mano, siempre
    /// iguales.
    fn trazos(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Forma::Letrero => (
                "M9 13 Q58 7 104 10 Q150 6 193 12 Q197 50 192 87 Q140 93 98 89 \
                 Q50 94 7 88 Q3 50 9 13 Z",
                &[
                    "M22 25 Q64 21 108 26",
                    "M94 78 Q136 73 180 78",
                    "M16 20 q3 -3 6 0 q-3 3 -6 0 Z",
                    "M178 20 q3 -3 6 0 q-3 3 -6 0 Z",
                ],
            ),
            Forma::Hoja => (
                "M10 52 Q30 12 100 9 Q166 8 194 48 Q168 90 100 93 Q34 94 10 52 Z",
                &[
                    "M10 52 Q5 55 1 62",
                    "M158 17 Q152 26 144 31",
                    "M160 84 Q153 76 145 71",
                    "M42 22 Q48 28 50 33",
                ],
            ),
            Forma::Piedra => (
                "M24 15 Q70 4 128 9 Q182 12 195 44 Q199 78 160 89 Q104 97 48 91 \
                 Q6 86 5 52 Q6 24 24 15 Z",
                &[
                    "M28 78 q4 -3 8 0 q-4 3 -8 0 Z",
                    "M168 24 q4 -3 8 0 q-4 3 -8 0 Z",
                    "M176 70 q3 -2 6 0 q-3 2 -6 0 Z",
                ],
            ),
            Forma::Sobre => (
                "M7 19 L17 10 L27 19 L37 10 L47 19 L57 10 L67 19 L77 10 L87 19 \
                 L97 10 L107 19 L117 10 L127 19 L137 10 L147 19 L157 10 L167 19 \
                 L177 10 L187 19 L193 15 L194 89 Q100 94 6 89 Z",
                &["M8 27 Q100 25 193 27"],
            ),
        }
    }
}

/// Un bancal: los torrents de un estado, con sus nombres «del huerto».
pub struct Grupo {
    pub estado: Estado,
    /// Para clases e identificadores.
    pub clase: &'static str,
    /// Su valor en la dirección: `?bancal=listos`.
    pub bancal: &'static str,
    /// Uno solo («Listo para recoger») y el nombre del bancal («Listos
    /// para recoger»).
    pub uno: &'static str,
    pub varios: &'static str,
    /// El nombre corto, para la pestaña en el móvil.
    pub corto: &'static str,
    /// Qué significa, en una frase (ver [`Grupo::explicacion`]).
    pub explica: &'static str,
    pub forma: Forma,
}

/// Los bancales, en el orden de las pestañas: primero lo que se puede
/// recoger.
pub const GRUPOS: [Grupo; 4] = [
    Grupo {
        estado: Estado::Cumplido,
        clase: "recoger",
        bancal: "listos",
        uno: "Listo para recoger",
        varios: "Listos para recoger",
        corto: "Listos",
        explica: "Ya se pueden recoger, a mano o con las reglas.",
        forma: Forma::Letrero,
    },
    Grupo {
        estado: Estado::EnEspera,
        clase: "creciendo",
        bancal: "creciendo",
        uno: "Creciendo",
        varios: "Creciendo",
        corto: "Creciendo",
        explica: "Les falta ratio. Siguen sembrando hasta llegar a la ratio mínima o al plazo \
                  máximo de las normas del tracker, y hasta entonces no se pueden recoger.",
        forma: Forma::Hoja,
    },
    Grupo {
        estado: Estado::Obligado,
        clase: "sembrado",
        bancal: "recien",
        uno: "Recién sembrado",
        varios: "Recién sembrados",
        corto: "Recién",
        explica: "Aún no se pueden recoger: llevan menos de los días de siembra de las normas \
                  del tracker.",
        forma: Forma::Piedra,
    },
    Grupo {
        estado: Estado::Libre,
        clase: "arraigar",
        bancal: "sin-arraigar",
        uno: "Sin arraigar",
        varios: "Sin arraigar",
        corto: "Sin arraigar",
        explica: "Apenas descargados: no obligan a nada y se pueden recoger.",
        forma: Forma::Sobre,
    },
];

impl Grupo {
    /// Qué significa el bancal, con las cifras de las normas del tracker.
    pub fn explicacion(&self, n: &NormasTracker) -> String {
        match self.estado {
            Estado::EnEspera => format!(
                "Les falta ratio. Siguen sembrando hasta llegar a ratio {} o a los {} días, y \
                 hasta entonces no se pueden recoger.",
                cifra(n.ratio_min),
                n.plazo_largo_dias
            ),
            Estado::Obligado => format!(
                "Aún no se pueden recoger: llevan menos de {} días sembrando.",
                n.siembra_dias
            ),
            _ => self.explica.to_owned(),
        }
    }
}

/// El bancal de un estado.
pub fn grupo(e: Estado) -> &'static Grupo {
    GRUPOS.iter().find(|g| g.estado == e).unwrap_or(&GRUPOS[0])
}

/// El bancal que se ve: el de la dirección si existe; si no, el primero
/// con macetas (y «Listos para recoger» si están todos vacíos).
pub fn bancal_elegido(valor: Option<&str>, cuentas: [usize; 4]) -> usize {
    valor
        .and_then(|v| GRUPOS.iter().position(|g| g.bancal == v))
        .or_else(|| cuentas.iter().position(|c| *c > 0))
        .unwrap_or(0)
}

/// ¿Se puede recoger (o quitar) a mano?
fn recogible(e: Estado) -> bool {
    matches!(e, Estado::Cumplido | Estado::Libre)
}

/// Lo que pide «Recoger»: quitar el torrent con sus ficheros (y lo suyo en
/// Jellyfin), como las reglas.
pub fn peticion_recoger(hash: &str, nombre: &str) -> Peticion {
    Peticion {
        titulo: format!("¿Recoger «{nombre}»?"),
        texto: "Se quita de qBittorrent y se borran sus ficheros del disco y sus datos de \
                Jellyfin. No se puede deshacer."
            .into(),
        boton: "Recoger",
        que: Delicada::Quitar {
            hash: hash.into(),
            ficheros: true,
        },
    }
}

/// Lo que pide «Quitar sin borrar los ficheros».
pub fn peticion_quitar_sin_ficheros(hash: &str, nombre: &str) -> Peticion {
    Peticion {
        titulo: "¿Quitar sin borrar los ficheros?".into(),
        texto: format!("Se quita «{nombre}» de qBittorrent. Los ficheros se quedan en el disco."),
        boton: "Quitar",
        que: Delicada::Quitar {
            hash: hash.into(),
            ficheros: false,
        },
    }
}

/// La maceta cuya ficha está abierta (su hash).
#[derive(Clone, Copy)]
struct Abierta(RwSignal<Option<String>>);

/// Lo que hace falta para contar los avisos.
#[derive(Clone, Copy)]
struct Jardin {
    ahora: i64,
    red_ok: bool,
    modo_real: bool,
    normas: NormasTracker,
}

impl Jardin {
    fn de(p: &DatosPortada) -> Self {
        Self {
            ahora: p.ahora,
            red_ok: p.franja.red_ok(),
            modo_real: p.modo_real,
            normas: p.normas,
        }
    }
}

/// Solo cuentan los avisos de macetas que siguen en el jardín.
fn avisos_vigentes(p: &DatosPortada) -> Vec<FichaAviso> {
    p.avisos
        .iter()
        .filter(|a| p.torrents.iter().any(|t| t.hash == a.hash))
        .cloned()
        .collect()
}

#[component]
pub fn PaginaPortada() -> impl IntoView {
    let recarga = expect_context::<Recarga>().0;
    let datos = Resource::new(move || recarga.get(), |_| portada());
    provide_context(Abierta(RwSignal::new(None)));
    // El estado va en la cabecera; si falla la carga, lo dice el jardín.
    let cabeza = move || {
        view! {
            <Transition fallback=|| ()>
                {move || Suspend::new(async move {
                    datos.await.ok().map(|p| view! { <Cabeza p /> })
                })}
            </Transition>
        }
    };
    view! {
        <Marco cabeza escena=true>
            <h1 class="solo-lectores">"Jardín"</h1>
            <Transition fallback=|| view! { <p class="cargando">"Cargando…"</p> }>
                {move || Suspend::new(async move {
                    // Las plantas crecen solo la primera vez, no tras cada acción.
                    let crecer = recarga.get_untracked() == 0;
                    match datos.await {
                        Ok(p) => view! { <Contenido p crecer /> }.into_any(),
                        Err(e) => {
                            view! { <p class="aviso-error">{super::texto_error(&e)}</p> }
                                .into_any()
                        }
                    }
                })}
            </Transition>
        </Marco>
    }
}

/// Lo que va en la cabecera: el estado y los avisos que vencen pronto.
#[component]
fn Cabeza(p: DatosPortada) -> impl IntoView {
    let j = Jardin::de(&p);
    let avisos = avisos_vigentes(&p);
    // El bancal de la primera maceta con aviso, para llevar hasta ella.
    let primera = avisos.first().map(|a| {
        let bancal = p
            .torrents
            .iter()
            .find(|t| t.hash == a.hash)
            .map_or(GRUPOS[0].bancal, |t| grupo(t.estado).bancal);
        format!("?bancal={bancal}#t-{}", a.hash)
    });
    let cercanas = primera.map(|enlace| view! { <Cercanas n=avisos.len() enlace j /> });
    view! {
        <Tiempo franja=p.franja />
        {cercanas}
    }
}

#[component]
fn Contenido(p: DatosPortada, crecer: bool) -> impl IntoView {
    let j = Jardin::de(&p);
    let avisos = avisos_vigentes(&p);
    let torrents = p.torrents;
    let listas: [Vec<FichaTorrent>; 4] = std::array::from_fn(|i| {
        torrents
            .iter()
            .filter(|t| t.estado == GRUPOS[i].estado)
            .cloned()
            .collect()
    });
    let cuentas: [usize; 4] = std::array::from_fn(|i| listas[i].len());
    let consulta = use_query_map();
    let elegido =
        Memo::new(move |_| consulta.with(|q| bancal_elegido(q.get_str("bancal"), cuentas)));
    // Hacia dónde se ha cambiado de bancal (para deslizarlo); `None` al
    // llegar, cuando las plantas crecen.
    let direccion = RwSignal::new(None::<bool>);
    let listas = StoredValue::new(listas);
    let avisos_panel = StoredValue::new(avisos.clone());
    view! {
        <div class="escena">
            <Pestanas cuentas elegido direccion />
            <div class="escena-bancal">
                <Panel listas avisos=avisos_panel elegido direccion j crecer />
            </div>
        </div>
        <Ficha torrents avisos j />
    }
    .into_any()
}

/// «N macetas se recogerán solas en menos de 24 h», con enlace a la
/// primera (en su bancal).
#[component]
fn Cercanas(n: usize, enlace: String, j: Jardin) -> impl IntoView {
    let tienen = if n == 1 {
        "maceta tiene"
    } else {
        "macetas tienen"
    };
    let texto = if !j.modo_real {
        format!(
            "{n} {tienen} aviso de recogida. Está activado «Solo aviso»: al vencer se apunta \
             en el historial y no se recoge nada."
        )
    } else if !j.red_ok {
        // Que las cuentas atrás están paradas ya lo dice el tiempo.
        format!("{n} {tienen} aviso de recogida.")
    } else if n == 1 {
        "1 maceta se recogerá sola en menos de 24 h.".to_owned()
    } else {
        format!("{n} macetas se recogerán solas en menos de 24 h.")
    };
    view! {
        <p class="cercanas">
            <a href=enlace>{texto}</a>
        </p>
    }
}

/// Regla en minúscula, para las etiquetas: «(regla: ratio)».
fn regla_en_etiqueta(r: TipoRegla) -> String {
    nombre_regla(r).to_lowercase()
}

/// Lo que dice la etiqueta de un aviso pendiente.
fn texto_aviso(j: Jardin, a: &FichaAviso) -> String {
    let regla = regla_en_etiqueta(a.regla);
    let falta = a.vence_en.saturating_sub(j.ahora);
    let cuando = if falta <= 0 {
        "en la próxima pasada".to_owned()
    } else {
        format!("en {}", duracion(falta))
    };
    if !j.red_ok {
        format!("Cuenta atrás parada (regla: {regla})")
    } else if j.modo_real {
        format!("Se recoge sola {cuando} (regla: {regla})")
    } else {
        format!("Se recogería sola {cuando} (regla: {regla})")
    }
}

/// Qué le falta a la maceta, en una línea corta para su etiqueta.
pub fn estado_etiqueta(t: &FichaTorrent, n: &NormasTracker) -> String {
    match t.estado {
        Estado::EnEspera => format!(
            "Le falta ratio {} (o {})",
            cifra(n.ratio_min),
            falta(t.falta_s)
        ),
        _ => estado_planta(t, n),
    }
}

/// Qué le falta a la maceta, con todas las palabras (en su ficha).
pub fn estado_planta(t: &FichaTorrent, n: &NormasTracker) -> String {
    match t.estado {
        Estado::Cumplido => "Lista para recoger".into(),
        Estado::Obligado => format!("Le {}", falta(t.falta_s)),
        Estado::EnEspera => format!(
            "Le falta llegar a ratio {} o a los {} días ({})",
            cifra(n.ratio_min),
            n.plazo_largo_dias,
            falta(t.falta_s)
        ),
        Estado::Libre => "No obliga a nada".into(),
    }
}

/// «9 días sembrando».
fn siembra(t: &FichaTorrent) -> String {
    if t.siembra_s < 60 {
        "Aún sin sembrar".into()
    } else {
        format!("{} sembrando", duracion(t.siembra_s))
    }
}

/// «Ratio 1,80».
fn ratio(t: &FichaTorrent) -> String {
    if t.ratio.is_finite() {
        format!("Ratio {}", decimal(t.ratio, 2))
    } else {
        "Ratio sin datos".into()
    }
}

/// El nombre, para la interfaz.
fn nombre(t: &FichaTorrent) -> String {
    if t.nombre.trim().is_empty() {
        "(sin nombre)".into()
    } else {
        t.nombre.clone()
    }
}

/// «2 macetas».
fn macetas(n: usize) -> String {
    if n == 1 {
        "1 maceta".into()
    } else {
        format!("{n} macetas")
    }
}

/// Las pestañas de los bancales: pestañas de verdad (flechas, Inicio y
/// Fin) y, a la vez, enlaces a `?bancal=…`. Con JavaScript, cambiar de
/// bancal cambia la dirección sin recargar la página.
#[component]
fn Pestanas(
    cuentas: [usize; 4],
    elegido: Memo<usize>,
    direccion: RwSignal<Option<bool>>,
) -> impl IntoView {
    let navegar = use_navigate();
    let lugar = use_location();
    let enlaces: [NodeRef<leptos::html::A>; 4] = std::array::from_fn(|_| NodeRef::new());
    let ir = move |i: usize| {
        let antes = elegido.get_untracked();
        if i == antes {
            return;
        }
        direccion.set(Some(i > antes));
        let ruta = format!(
            "{}?bancal={}",
            lugar.pathname.get_untracked(),
            GRUPOS[i].bancal
        );
        navegar(
            &ruta,
            NavigateOptions {
                replace: true,
                scroll: false,
                ..Default::default()
            },
        );
    };
    let teclas = {
        let ir = ir.clone();
        move |ev: leptos::ev::KeyboardEvent| {
            let actual = elegido.get_untracked();
            let destino = match ev.key().as_str() {
                "ArrowRight" => (actual + 1) % GRUPOS.len(),
                "ArrowLeft" => (actual + GRUPOS.len() - 1) % GRUPOS.len(),
                "Home" => 0,
                "End" => GRUPOS.len() - 1,
                _ => return,
            };
            ev.prevent_default();
            ir(destino);
            if let Some(a) = enlaces[destino].get_untracked() {
                let _ = a.focus();
            }
        }
    };
    let pestanas = GRUPOS
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let elegida = move || elegido.get() == i;
            let ir = ir.clone();
            let pulsar = move |ev: leptos::ev::MouseEvent| {
                // Abrir en otra pestaña del navegador sigue funcionando.
                if ev.button() != 0 || ev.ctrl_key() || ev.meta_key() || ev.shift_key() {
                    return;
                }
                ev.prevent_default();
                ir(i);
            };
            let (silueta, detalles) = g.forma.trazos();
            let detalles = detalles
                .iter()
                .map(|d| view! { <path class="pestana-detalle" d=*d /> })
                .collect_view();
            view! {
                <a
                    id=format!("pestana-{}", g.bancal)
                    role="tab"
                    class=format!("pestana pestana-{}", g.forma.clase())
                    href=format!("?bancal={}", g.bancal)
                    aria-selected=move || if elegida() { "true" } else { "false" }
                    aria-controls="bancal-elegido"
                    aria-label=format!("{}, {}", g.varios, macetas(cuentas[i]))
                    tabindex=move || if elegida() { "0" } else { "-1" }
                    node_ref=enlaces[i]
                    on:click=pulsar
                >
                    <svg
                        class="pestana-forma"
                        viewBox="0 0 200 100"
                        preserveAspectRatio="none"
                        aria-hidden="true"
                        focusable="false"
                    >
                        <path class="pestana-sombra" d=silueta />
                        <path class="pestana-silueta" d=silueta />
                        {detalles}
                    </svg>
                    <GlifoPlanta estado=g.estado />
                    <span class="pestana-texto">
                        <span class="pestana-nombre">
                            <span class="nombre-largo">{g.varios}</span>
                            <span class="nombre-corto">{g.corto}</span>
                        </span>
                        <span class="pestana-cuenta">{cuentas[i]}</span>
                    </span>
                </a>
            }
        })
        .collect_view();
    // Con el tipo borrado: la escena entera anida demasiados tipos para el
    // compilador en la versión de producción.
    view! {
        <div class="pestanas" role="tablist" aria-label="Bancales" on:keydown=teclas>
            {pestanas}
        </div>
    }
    .into_any()
}

/// El bancal elegido: su explicación y sus macetas en filas sobre la
/// tierra. Al cambiar de bancal, entra deslizándose desde ese lado.
#[component]
fn Panel(
    listas: StoredValue<[Vec<FichaTorrent>; 4]>,
    avisos: StoredValue<Vec<FichaAviso>>,
    elegido: Memo<usize>,
    direccion: RwSignal<Option<bool>>,
    j: Jardin,
    crecer: bool,
) -> impl IntoView {
    let cuerpo = move || {
        let i = elegido.get();
        let g = &GRUPOS[i];
        let llegada = direccion.get_untracked();
        let clase = match llegada {
            None => "bancal-cuerpo",
            Some(true) => "bancal-cuerpo entra-derecha",
            Some(false) => "bancal-cuerpo entra-izquierda",
        };
        let lista = listas.with_value(|l| l[i].clone());
        let macetas = if lista.is_empty() {
            view! {
                <div class="cama cama-vacia">
                    <p class="vacio">"Ninguna maceta en este bancal."</p>
                </div>
            }
            .into_any()
        } else {
            let crecer = crecer && llegada.is_none();
            let macetas = lista
                .into_iter()
                .map(|t| {
                    let aviso = avisos.with_value(|v| v.iter().find(|a| a.hash == t.hash).cloned());
                    view! { <UnaMaceta t aviso j crecer /> }
                })
                .collect_view();
            view! { <ul class="cama macetas">{macetas}</ul> }.into_any()
        };
        view! {
            <div class=clase>
                <h2 class="solo-lectores">{g.varios}</h2>
                // La explicación, en un letrero de madera clavado en el bancal.
                <p class="letrero-bancal">{g.explicacion(&j.normas)}</p>
                {macetas}
            </div>
        }
    };
    view! {
        <section
            id="bancal-elegido"
            role="tabpanel"
            class=move || format!("bancal bancal-de-{}", GRUPOS[elegido.get()].clase)
            aria-labelledby=move || format!("pestana-{}", GRUPOS[elegido.get()].bancal)
        >
            {cuerpo}
        </section>
    }
    .into_any()
}

/// Una maceta con su etiqueta de herbolario.
#[component]
fn UnaMaceta(t: FichaTorrent, aviso: Option<FichaAviso>, j: Jardin, crecer: bool) -> impl IntoView {
    let acciones = Acciones::del_contexto();
    let abierta = expect_context::<Abierta>().0;
    let saliendo = expect_context::<Saliendo>().0;
    let hash = t.hash.clone();
    let nombre = nombre(&t);
    let g = grupo(t.estado);
    let clase_li = {
        let hash = hash.clone();
        let base = format!("maceta maceta-{}", g.clase);
        move || {
            if saliendo.with(|s| s.as_deref() == Some(hash.as_str())) {
                format!("{base} maceta-sale")
            } else {
                base.clone()
            }
        }
    };
    let abrir = {
        let hash = hash.clone();
        move || abierta.set(Some(hash.clone()))
    };
    let abrir2 = abrir.clone();
    // Cada etiqueta, algo torcida a su manera (siempre la misma).
    let giro = semilla(&hash) % 5;
    let clase_etiqueta = if aviso.is_some() {
        format!("etiqueta etiqueta-aviso giro-{giro}")
    } else {
        format!("etiqueta giro-{giro}")
    };
    let pausa = t
        .actividad
        .parado()
        .then(|| view! { <p class="pausa">"En pausa"</p> });
    let texto_aviso = aviso.as_ref().map(|a| texto_aviso(j, a));
    let no_recoger = aviso.as_ref().map(|a| {
        let (hash, regla) = (a.hash.clone(), a.regla);
        view! {
            <button
                type="button"
                class="boton boton-texto"
                on:click=move |_| acciones.lanzar(no_borrar(hash.clone(), regla))
            >
                "No recoger"
            </button>
        }
    });
    let recoger = recogible(t.estado).then(|| {
        let (hash, nombre) = (hash.clone(), nombre.clone());
        view! {
            <button
                type="button"
                class="boton boton-texto boton-peligro"
                on:click=move |_| pedir_confirmacion(peticion_recoger(&hash, &nombre))
            >
                "Recoger"
            </button>
        }
    });
    let hay_botones = no_recoger.is_some() || recoger.is_some();
    let botones = hay_botones.then(|| {
        view! {
            <p class="acciones acciones-planta">
                {no_recoger}
                {recoger}
            </p>
        }
    });
    view! {
        <li class=clase_li id=format!("t-{hash}")>
            // El tiesto también abre la ficha con el ratón; con el teclado,
            // el nombre de la etiqueta.
            <div class="maceta-tiesto" on:click=move |_| abrir()>
                <Maceta t=t.clone() normas=j.normas crecer />
            </div>
            // La etiqueta va clavada en una estaca delante de la maceta: el
            // nombre y una o dos líneas cortas; los datos, en la ficha (y
            // aquí, para los lectores de pantalla).
            <span class="estaca" aria-hidden="true"></span>
            <div class=clase_etiqueta>
                <h3 class="nombre-planta">
                    <button
                        type="button"
                        class="abrir-ficha"
                        aria-haspopup="dialog"
                        on:click=move |_| abrir2()
                    >
                        <span class="nombre-texto">{nombre.clone()}</span>
                    </button>
                </h3>
                <p class="estado-planta">{estado_etiqueta(&t, &j.normas)}</p>
                {pausa}
                {texto_aviso.map(|a| view! { <p class="aviso-planta">{a}</p> })}
                <p class="solo-lectores">{format!("{}. {}.", siembra(&t), ratio(&t))}</p>
                {botones}
            </div>
        </li>
    }
}

/// La ficha de la maceta abierta: un pliego de herbario con la planta
/// montada, su etiqueta con todos los datos (cada uno una vez) y todo lo
/// que se puede hacer con ella. Se cierra con «Cerrar», con Escape o
/// pulsando fuera; el foco vuelve al nombre de la maceta.
#[component]
fn Ficha(torrents: Vec<FichaTorrent>, avisos: Vec<FichaAviso>, j: Jardin) -> impl IntoView {
    let abierta = expect_context::<Abierta>().0;
    let confirmar = expect_context::<super::Confirmar>().0;
    let dialogo = NodeRef::<leptos::html::Dialog>::new();
    let torrents = StoredValue::new(torrents);
    let avisos = StoredValue::new(avisos);
    // La última maceta abierta, para devolverle el foco al cerrar.
    let ultima = StoredValue::new(None::<String>);
    let actual = move || {
        let hash = abierta.get()?;
        torrents.with_value(|v| v.iter().find(|t| t.hash == hash).cloned())
    };

    // Abrir y cerrar el diálogo según la maceta elegida. Si ya no está (se
    // ha recogido), se cierra.
    Effect::new(move |_| {
        let elegida = abierta.get();
        let Some(d) = dialogo.get() else { return };
        let esta = elegida
            .as_ref()
            .is_some_and(|h| torrents.with_value(|v| v.iter().any(|t| &t.hash == h)));
        if esta {
            ultima.set_value(elegida);
            if !d.open() {
                let _ = d.show_modal();
            }
            return;
        }
        if elegida.is_some() {
            abierta.set(None);
        }
        if d.open() {
            d.close();
        }
        // El foco vuelve a la maceta, salvo que se pase a confirmar algo.
        let Some(hash) = ultima.try_update_value(Option::take).flatten() else {
            return;
        };
        if confirmar.with_untracked(Option::is_none) {
            devolver_foco(&hash);
        }
    });
    let cerrar = move || {
        if abierta.with_untracked(Option::is_some) {
            abierta.set(None);
        }
    };
    // Pulsar fuera del pliego (en el velo) también cierra: entonces el
    // destino del clic es el propio diálogo.
    let fuera = move |ev: leptos::ev::MouseEvent| {
        #[cfg(feature = "hydrate")]
        {
            use leptos::wasm_bindgen::JsCast;
            if ev
                .target()
                .and_then(|t| t.dyn_into::<web_sys::HtmlDialogElement>().ok())
                .is_some()
            {
                cerrar();
            }
        }
        #[cfg(not(feature = "hydrate"))]
        let _ = ev;
    };

    view! {
        <dialog
            class="dialogo ficha"
            node_ref=dialogo
            aria-labelledby="ficha-titulo"
            on:close=move |_| cerrar()
            on:click=fuera
        >
            {move || {
                actual()
                    .map(|t| {
                        let aviso = avisos
                            .with_value(|v| v.iter().find(|a| a.hash == t.hash).cloned());
                        view! { <ContenidoFicha t aviso j /> }
                    })
            }}
        </dialog>
    }
    .into_any()
}

/// Lleva el foco al nombre de la maceta `hash` (el botón que abre su
/// ficha).
fn devolver_foco(hash: &str) {
    #[cfg(feature = "hydrate")]
    {
        use leptos::wasm_bindgen::JsCast;
        if let Some(b) = document()
            .get_element_by_id(&format!("t-{hash}"))
            .and_then(|li| li.query_selector(".abrir-ficha").ok().flatten())
            .and_then(|b| b.dyn_into::<web_sys::HtmlElement>().ok())
        {
            let _ = b.focus();
        }
    }
    #[cfg(not(feature = "hydrate"))]
    let _ = hash;
}

/// Un dato de la etiqueta del pliego.
fn dato(nombre: &'static str, valor: String) -> impl IntoView {
    view! {
        <div class="ficha-dato">
            <dt>{nombre}</dt>
            <dd>{valor}</dd>
        </div>
    }
}

/// El pliego de herbario de una maceta.
#[component]
fn ContenidoFicha(t: FichaTorrent, aviso: Option<FichaAviso>, j: Jardin) -> impl IntoView {
    let acciones = Acciones::del_contexto();
    let abierta = expect_context::<Abierta>().0;
    let nombre = nombre(&t);
    let parado = t.actividad.parado();
    let ahora = if parado {
        "En pausa".to_owned()
    } else {
        t.actividad.texto().to_owned()
    };
    let siembra = if t.siembra_s < 60 {
        "Aún no ha sembrado".to_owned()
    } else {
        duracion(t.siembra_s)
    };
    let ratio = if t.ratio.is_finite() {
        decimal(t.ratio, 2)
    } else {
        "sin datos".to_owned()
    };
    let donde = format!("{}, {}", t.categoria, bytes(t.tamano));
    let aviso_dato = aviso.as_ref().map(|a| {
        view! {
            <div class="ficha-dato ficha-aviso">
                <dt>"Aviso"</dt>
                <dd>{texto_aviso(j, a)}</dd>
            </div>
        }
    });

    let pausa = {
        let hash = t.hash.clone();
        move |_| acciones.lanzar(pausar(hash.clone(), !parado))
    };
    let no_recoger = aviso.as_ref().map(|a| {
        let (hash, regla) = (a.hash.clone(), a.regla);
        view! {
            <button
                type="button"
                class="boton boton-texto"
                on:click=move |_| acciones.lanzar(no_borrar(hash.clone(), regla))
            >
                "No recoger"
            </button>
        }
    });
    let recoger = recogible(t.estado).then(|| {
        let (hash, nombre) = (t.hash.clone(), nombre.clone());
        let (hash2, nombre2) = (hash.clone(), nombre.clone());
        view! {
            <button
                type="button"
                class="boton boton-texto boton-peligro"
                on:click=move |_| {
                    abierta.set(None);
                    pedir_confirmacion(peticion_recoger(&hash, &nombre));
                }
            >
                "Recoger"
            </button>
            <button
                type="button"
                class="boton boton-texto"
                on:click=move |_| {
                    abierta.set(None);
                    pedir_confirmacion(peticion_quitar_sin_ficheros(&hash2, &nombre2));
                }
            >
                "Quitar sin borrar los ficheros"
            </button>
        }
    });
    let exenciones = t
        .exenciones
        .iter()
        .map(|r| {
            let (hash, regla) = (t.hash.clone(), *r);
            view! {
                <li>
                    {format!("Con «No recoger»: la regla «{}» no la recoge. ", nombre_regla(regla))}
                    <button
                        type="button"
                        class="boton boton-texto"
                        on:click=move |_| acciones.lanzar(quitar_exencion(hash.clone(), regla))
                    >
                        "Volver a contar"
                    </button>
                </li>
            }
        })
        .collect_view();
    let hay_exenciones = !t.exenciones.is_empty();
    view! {
        <div class="pliego">
            <button
                type="button"
                class="boton boton-plano ficha-cerrar"
                on:click=move |_| abierta.set(None)
            >
                "Cerrar"
            </button>
            // La planta, sujeta al papel con dos tiras de cinta.
            <div class="pliego-planta">
                <span class="cinta cinta-izquierda" aria-hidden="true"></span>
                <span class="cinta cinta-derecha" aria-hidden="true"></span>
                <Maceta t=t.clone() normas=j.normas />
            </div>
            <div class="pliego-etiqueta">
                <h2 id="ficha-titulo" class="ficha-nombre">
                    {nombre}
                </h2>
                <dl class="ficha-datos">
                    {dato("Estado", estado_planta(&t, &j.normas))}
                    {dato("Siembra", siembra)}
                    {dato("Ratio", ratio)}
                    {dato("Ahora", ahora)}
                    {dato("Dónde", donde)}
                    {aviso_dato}
                </dl>
                {hay_exenciones.then(|| view! { <ul class="ficha-exenciones">{exenciones}</ul> })}
            </div>
            <p class="acciones ficha-acciones">
                <button type="button" class="boton boton-texto" on:click=pausa>
                    {if parado { "Reanudar" } else { "Parar" }}
                </button>
                {no_recoger}
                {recoger}
            </p>
        </div>
    }
    .into_any()
}

/// La ficha de la maceta `hash` como HTML, tal y como se ve al abrirla
/// (para las pruebas y la revisión visual). `None` si no está.
#[cfg(feature = "ssr")]
pub fn ficha_en_html(p: &DatosPortada, hash: &str) -> Option<String> {
    let t = p.torrents.iter().find(|t| t.hash == hash)?.clone();
    let aviso = p.avisos.iter().find(|a| a.hash == hash).cloned();
    let j = Jardin::de(p);
    let owner = Owner::new();
    Some(owner.with(|| {
        super::dar_contextos();
        provide_context(Abierta(RwSignal::new(Some(t.hash.clone()))));
        view! { <ContenidoFicha t aviso j /> }.to_html()
    }))
}
