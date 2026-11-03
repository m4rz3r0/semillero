//! Reglas de recogida: «Solo aviso / Borrado real», las normas del tracker
//! (las de por defecto y las propias de cada tracker que se ve en los
//! torrents), un apartado por categoría (con su tiesto) con sus cuatro reglas
//! (interruptor y valor), los presets y los «No recoger» vigentes.

use leptos::prelude::*;
use reglas::{NormasTracker, ReglasCategoria, TipoRegla};

use crate::api::ERROR_REAUTH;

use super::formato::decimal;
use super::maceta::{Tiesto, TiestoVacio};
use super::{Acciones, Delicada, Marco, Peticion, Recarga, pedir_confirmacion, texto_error};
use crate::api::{
    DatosReglas, FichaExencion, FichaPreset, FichaTracker, RUTA_DESCARGA, TarjetaReglas,
    cambiar_modo, cambiar_preset, guardar_normas, guardar_normas_tracker, guardar_preset,
    guardar_reglas, nombre_categoria, nombre_regla, pagina_reglas, quitar_exencion,
    quitar_normas_tracker, subir_preset,
};

/// Texto del diálogo al guardar normas o reglas con el borrado real activo.
const CON_BORRADO_REAL: &str = "Con el borrado real activo, cambiar lo que se recoge pide tu contraseña. Se guardan los valores que acabas de escribir.";

/// Valores que se proponen al encender una regla.
const RATIO: f64 = 2.0;
const SIEMBRA_DIAS: u32 = 30;
const INACTIVIDAD_DIAS: u32 = 14;
const DISCO_PCT: f64 = 10.0;

#[component]
pub fn PaginaReglas() -> impl IntoView {
    let recarga = expect_context::<Recarga>().0;
    let datos = Resource::new(move || recarga.get(), |_| pagina_reglas());
    // Fuera del contenido: sobrevive a la recarga tras guardar un preset.
    let estado_presets = RwSignal::new(None::<(bool, String)>);
    view! {
        <Marco>
            <div class="pagina">
                <h1 class="titulo-pagina">"Reglas de recogida"</h1>
                <Transition fallback=|| view! { <p class="cargando">"Cargando…"</p> }>
                    {move || Suspend::new(async move {
                        match datos.await {
                            Ok(p) => view! { <Contenido p estado_presets /> }.into_any(),
                            Err(e) => {
                                view! { <p class="aviso-error">{texto_error(&e)}</p> }.into_any()
                            }
                        }
                    })}
                </Transition>
            </div>
        </Marco>
    }
}

#[component]
fn Contenido(p: DatosReglas, estado_presets: RwSignal<Option<(bool, String)>>) -> impl IntoView {
    let tarjetas = p
        .tarjetas
        .into_iter()
        .map(|t| view! { <Tarjeta t /> })
        .collect_view();
    // Las normas que usa ahora cada tracker sin las suyas (las de su padre o
    // las de por defecto): con ellas empieza el formulario al marcarlo.
    let propias: Vec<(String, NormasTracker)> = p
        .trackers
        .iter()
        .filter_map(|t| t.propias.map(|n| (t.host.clone(), n)))
        .collect();
    let con_propias = propias.len();
    let defecto = p.normas;
    let trackers = p
        .trackers
        .into_iter()
        .map(|f| {
            let usadas = f
                .usa
                .as_ref()
                .and_then(|h| propias.iter().find(|(p, _)| p == h))
                .map_or(defecto, |(_, n)| *n);
            view! { <UnTracker f usadas /> }
        })
        .collect_view();
    view! {
        <Modo real=p.modo_real />
        <p class="explica">
            {format!(
                "Las reglas solo recogen las macetas «Listos para recoger» y «Sin arraigar», como «Recoger»: el torrent, sus ficheros y sus datos de Jellyfin. Cuando una se cumple, la etiqueta de la maceta avisa {} horas antes, con «No recoger». Todas empiezan apagadas.",
                p.normas.aviso_horas,
            )}
        </p>
        <div class="categorias">
            <Normas n=p.normas />
            {tarjetas}
        </div>
        <section class="seccion exenciones-nota" aria-labelledby="titulo-trackers">
            <h2 id="titulo-trackers">"Normas por tracker"</h2>
            <p class="explica">
                "Cada torrent sigue las normas de su tracker. Los que no tienen normas propias usan las de arriba (o las de su dominio, si las tiene: ejemplo.example vale para tracker.ejemplo.example)."
            </p>
            {if trackers.is_empty() {
                view! { <p class="vacio">"Aún no se ve ningún tracker en los torrents."</p> }
                    .into_any()
            } else {
                view! { <div class="categorias">{trackers}</div> }.into_any()
            }}
        </section>
        <Presets lista=p.presets estado=estado_presets con_propias />
        <Exenciones lista=p.exenciones />
    }
}

/// El interruptor general: qué pasa cuando vence un aviso. Dos letreros de
/// madera clavados en el suelo; el elegido, pulsado.
#[component]
fn Modo(real: bool) -> impl IntoView {
    let acciones = Acciones::del_contexto();
    let a_solo_aviso = move |_| {
        if real {
            acciones.lanzar(cambiar_modo(false, None));
        }
    };
    let a_borrado_real = move |_| {
        if !real {
            pedir_confirmacion(Peticion {
                titulo: "¿Activar el borrado real?".into(),
                texto: "A partir de ahora, cuando venza un aviso se recogerá la maceta: se borran el torrent, sus ficheros y sus datos de Jellyfin. Puedes volver a «Solo aviso» cuando quieras.".into(),
                boton: "Activar el borrado real",
                que: Delicada::BorradoReal,
            });
        }
    };
    let clase = |activo: bool| {
        if activo {
            "letrero-modo letrero-elegido"
        } else {
            "letrero-modo"
        }
    };
    view! {
        <section class="modo" aria-labelledby="titulo-modo">
            <h2 id="titulo-modo">"Al vencer un aviso"</h2>
            <div class="letreros-modo" role="group" aria-labelledby="titulo-modo">
                <button
                    type="button"
                    class=clase(!real)
                    aria-pressed=if real { "false" } else { "true" }
                    on:click=a_solo_aviso
                >
                    <span class="letrero-titulo">"Solo aviso"</span>
                    <span class="letrero-texto">
                        "Se apunta «se habría recogido» en el historial y no se recoge nada."
                    </span>
                </button>
                <button
                    type="button"
                    class=clase(real)
                    aria-pressed=if real { "true" } else { "false" }
                    on:click=a_borrado_real
                >
                    <span class="letrero-titulo">"Borrado real"</span>
                    <span class="letrero-texto">
                        "Se recoge: se borran el torrent, sus ficheros y sus datos de Jellyfin. Pide tu contraseña para activarlo."
                    </span>
                </button>
            </div>
        </section>
    }
}

/// Una regla de la tarjeta: interruptor, frase y valor.
#[component]
fn Fila(
    id: String,
    encendida: RwSignal<bool>,
    valor: RwSignal<String>,
    antes: &'static str,
    despues: &'static str,
    paso: &'static str,
    titulo: &'static str,
) -> impl IntoView {
    let id_valor = format!("{id}-valor");
    view! {
        <div class="regla">
            <input
                id=id.clone()
                type="checkbox"
                role="switch"
                class="interruptor"
                bind:checked=encendida
            />
            <label for=id class="regla-nombre">
                {titulo}
            </label>
            <div class="regla-valor">
                <label for=id_valor.clone()>{antes}</label>
                <input
                    id=id_valor
                    type="number"
                    inputmode="decimal"
                    step=paso
                    min="0"
                    class="cifra"
                    // El valor en el HTML del servidor; luego, `bind:value`.
                    value=valor.get_untracked()
                    bind:value=valor
                    disabled=move || !encendida.get()
                />
                <span>{despues}</span>
            </div>
        </div>
    }
}

fn numero(s: &str) -> Option<f64> {
    s.trim().replace(',', ".").parse::<f64>().ok()
}

/// Días u horas: un número entero, sin decimales.
fn entero(s: &str) -> Option<u32> {
    numero(s)
        .filter(|v| v.fract() == 0.0 && *v >= 0.0 && *v <= f64::from(u32::MAX))
        .map(|v| v as u32)
}

/// Una cifra de las normas: frase, valor y unidad.
#[component]
fn Cifra(
    id: String,
    valor: RwSignal<String>,
    antes: &'static str,
    despues: &'static str,
    paso: &'static str,
) -> impl IntoView {
    view! {
        <div class="regla-valor norma">
            <label for=id.clone()>{antes}</label>
            <input
                id=id
                type="number"
                inputmode="decimal"
                step=paso
                min="0"
                class="cifra"
                value=valor.get_untracked()
                bind:value=valor
            />
            <span>{despues}</span>
        </div>
    }
}

/// Las normas del tracker: cuándo un torrent obliga a sembrar y cuánto. Lo
/// que no las cumple no se recoge nunca, ni a mano ni con las reglas. Estas
/// son las de por defecto.
#[component]
fn Normas(n: NormasTracker) -> impl IntoView {
    view! {
        <section class="categoria tablilla normas" aria-labelledby="normas-titulo">
            <h2 id="normas-titulo">
                <span>"Normas del tracker"</span>
            </h2>
            <p class="nota">
                "Lo que pide tu tracker para no dejar de sembrar antes de tiempo. Mientras un torrent no las cumple, no se recoge: ni a mano ni con las reglas."
            </p>
            <FormNormas n tracker=None />
        </section>
    }
}

/// Un trozo de `id` para el host de un tracker: letras, cifras y guiones.
fn id_de_host(host: &str) -> String {
    host.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Un tracker: cuántos torrents tiene y, con «Usar normas propias», sus
/// normas (el mismo formulario que las de por defecto). Al quitar la marca
/// vuelve a usar las de por defecto (o las de su dominio).
#[component]
fn UnTracker(f: FichaTracker, usadas: NormasTracker) -> impl IntoView {
    let recarga = expect_context::<Recarga>().0;
    let host = f.host.clone();
    let base = format!("tracker-{}", id_de_host(&host));
    let marcada = RwSignal::new(f.propias.is_some());
    let tenia = f.propias.is_some();
    let estado = RwSignal::new(None::<(bool, String)>);
    let quitar = Action::new(move |h: &String| {
        let h = h.clone();
        async move { (h.clone(), quitar_normas_tracker(h, None).await) }
    });
    let titulo = format!("¿Quitar las normas propias de {host}?");
    Effect::new(move |_| match quitar.value().get() {
        Some((_, Ok(()))) => recarga.update(|v| *v += 1),
        Some((tracker, Err(ServerFnError::ServerError(m)))) if m == ERROR_REAUTH => {
            marcada.set(true);
            pedir_confirmacion(Peticion {
                titulo: titulo.clone(),
                texto: CON_BORRADO_REAL.into(),
                boton: "Quitar",
                que: Delicada::QuitarNormasTracker { tracker },
            });
        }
        Some((_, Err(e))) => {
            marcada.set(true);
            estado.set(Some((false, texto_error(&e))));
        }
        None => {}
    });
    let h = host.clone();
    Effect::new(move |antes: Option<bool>| {
        let ahora = marcada.get();
        if antes == Some(true) && !ahora && tenia {
            estado.set(None);
            quitar.dispatch(h.clone());
        }
        ahora
    });
    let cuantos = match f.torrents {
        0 => "Ningún torrent ahora".to_owned(),
        1 => "1 torrent".to_owned(),
        n => format!("{n} torrents"),
    };
    let usa = match (&f.propias, &f.usa) {
        (Some(_), _) => "usa sus normas propias.".to_owned(),
        (None, Some(padre)) => format!("usa las normas de {padre}."),
        (None, None) => "usa las normas por defecto.".to_owned(),
    };
    let n = f.propias.unwrap_or(usadas);
    let (id_casilla, id_casilla2, id_titulo, id_h2) = (
        format!("{base}-propias"),
        format!("{base}-propias"),
        format!("{base}-titulo"),
        format!("{base}-titulo"),
    );
    let tracker = host.clone();
    view! {
        <section class="categoria tablilla normas" aria-labelledby=id_titulo>
            <h2 id=id_h2>
                <span>{host.clone()}</span>
            </h2>
            <p class="nota">{format!("{cuantos}; {usa}")}</p>
            <div class="regla">
                <input
                    id=id_casilla
                    type="checkbox"
                    role="switch"
                    class="interruptor"
                    bind:checked=marcada
                />
                <label for=id_casilla2 class="regla-nombre">
                    "Usar normas propias"
                </label>
            </div>
            <p
                class=move || match estado.get() {
                    Some((false, _)) => "estado-guardar error",
                    _ => "estado-guardar",
                }
                role="status"
            >
                {move || estado.get().map(|(_, m)| m)}
            </p>
            <Show when=move || marcada.get()>
                <FormNormas n tracker=Some(tracker.clone()) />
            </Show>
        </section>
    }
}

/// Los campos de unas normas y «Guardar»: las de por defecto (`tracker`
/// vacío) o las propias de un tracker.
#[component]
fn FormNormas(n: NormasTracker, tracker: Option<String>) -> impl IntoView {
    let recarga = expect_context::<Recarga>().0;
    let umbral = RwSignal::new(cifra_campo(n.umbral_pct));
    let siembra = RwSignal::new(n.siembra_dias.to_string());
    let ratio_min = RwSignal::new(cifra_campo(n.ratio_min));
    let plazo = RwSignal::new(n.plazo_largo_dias.to_string());
    let ratio_pausa = RwSignal::new(cifra_campo(n.ratio_pausa));
    let aviso = RwSignal::new(n.aviso_horas.to_string());
    let estado = RwSignal::new(None::<(bool, String)>);

    let base = match &tracker {
        None => "normas".to_owned(),
        Some(h) => format!("normas-{}", id_de_host(h)),
    };
    let id = |campo: &str| format!("{base}-{campo}");
    let titulo = match &tracker {
        None => "¿Guardar las normas del tracker?".to_owned(),
        Some(h) => format!("¿Guardar las normas de {h}?"),
    };
    let t = tracker.clone();
    let guardar = Action::new(move |n: &NormasTracker| {
        let (n, t) = (*n, t.clone());
        async move {
            let r = match t.clone() {
                None => guardar_normas(n, None).await,
                Some(h) => guardar_normas_tracker(h, n, None).await,
            };
            (n, t, r)
        }
    });
    Effect::new(move |_| match guardar.value().get() {
        Some((_, t, Ok(()))) => {
            estado.set(Some((true, "Guardado.".into())));
            // Las de un tracker: se recarga la página, como tras la
            // contraseña, para que su tarjeta sepa que ya las tiene (y quitar
            // la marca las borre) y el diálogo de aplicar un preset las cuente.
            if t.is_some() {
                recarga.update(|v| *v += 1);
            }
        }
        // Con el borrado real hace falta la contraseña: la pide el diálogo.
        Some((normas, tracker, Err(ServerFnError::ServerError(m)))) if m == ERROR_REAUTH => {
            pedir_confirmacion(Peticion {
                titulo: titulo.clone(),
                texto: CON_BORRADO_REAL.into(),
                boton: "Guardar",
                que: Delicada::GuardarNormas { normas, tracker },
            });
        }
        Some((_, _, Err(e))) => estado.set(Some((false, texto_error(&e)))),
        None => {}
    });

    let enviar = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let leer = || -> Result<NormasTracker, &'static str> {
            Ok(NormasTracker {
                umbral_pct: numero(&umbral.get_untracked())
                    .ok_or("Escribe el porcentaje descargado que obliga.")?,
                siembra_dias: entero(&siembra.get_untracked())
                    .ok_or("Escribe los días de siembra, sin decimales.")?,
                ratio_min: numero(&ratio_min.get_untracked()).ok_or("Escribe la ratio mínima.")?,
                plazo_largo_dias: entero(&plazo.get_untracked())
                    .ok_or("Escribe el plazo máximo en días, sin decimales.")?,
                ratio_pausa: numero(&ratio_pausa.get_untracked())
                    .ok_or("Escribe la ratio para parar.")?,
                aviso_horas: entero(&aviso.get_untracked())
                    .ok_or("Escribe las horas de aviso, sin decimales.")?,
            })
        };
        match leer().and_then(|n| n.validar().map(|()| n)) {
            Ok(n) => {
                estado.set(None);
                guardar.dispatch(n);
            }
            Err(m) => estado.set(Some((false, m.into()))),
        }
    };

    view! {
        <form on:submit=enviar>
                <Cifra
                    id=id("umbral")
                    valor=umbral
                    antes="Obliga a sembrar desde el"
                    despues="% descargado"
                    paso="1"
                />
                <Cifra
                    id=id("siembra")
                    valor=siembra
                    antes="Sembrar al menos"
                    despues="días"
                    paso="1"
                />
                <Cifra
                    id=id("ratio")
                    valor=ratio_min
                    antes="Después, listo con ratio"
                    despues=""
                    paso="0.1"
                />
                <Cifra
                    id=id("plazo")
                    valor=plazo
                    antes="o a los"
                    despues="días sembrando"
                    paso="1"
                />
                <Cifra
                    id=id("pausa")
                    valor=ratio_pausa
                    antes="Parar solo al llegar a ratio"
                    despues=""
                    paso="0.1"
                />
                <Cifra
                    id=id("aviso")
                    valor=aviso
                    antes="Avisar"
                    despues="horas antes de recoger"
                    paso="1"
                />
                <div class="categoria-pie">
                    <p
                        class=move || match estado.get() {
                            Some((false, _)) => "estado-guardar error",
                            _ => "estado-guardar",
                        }
                        role="status"
                    >
                        {move || estado.get().map(|(_, m)| m)}
                    </p>
                    <button
                        type="submit"
                        class="boton boton-acento"
                        disabled=move || guardar.pending().get()
                    >
                        "Guardar"
                    </button>
                </div>
        </form>
    }
}

/// Un número para un campo: con punto y al menos un decimal («1.0», «2.5»).
fn cifra_campo(v: f64) -> String {
    super::formato::cifra(v).replace(',', ".")
}

#[component]
fn Tarjeta(t: TarjetaReglas) -> impl IntoView {
    let r = t.reglas;
    let categoria = t.categoria;
    let base = format!(
        "reglas-{}",
        if categoria.is_empty() {
            "sin"
        } else {
            categoria.as_str()
        }
    );
    let ratio_on = RwSignal::new(r.ratio.is_some());
    let ratio = RwSignal::new(decimal(r.ratio.unwrap_or(RATIO), 1).replace(',', "."));
    let siembra_on = RwSignal::new(r.siembra_dias.is_some());
    let siembra = RwSignal::new(r.siembra_dias.unwrap_or(SIEMBRA_DIAS).to_string());
    let inact_on = RwSignal::new(r.inactividad_dias.is_some());
    let inact = RwSignal::new(r.inactividad_dias.unwrap_or(INACTIVIDAD_DIAS).to_string());
    let disco_on = RwSignal::new(r.disco_libre_pct.is_some());
    let disco = RwSignal::new(decimal(r.disco_libre_pct.unwrap_or(DISCO_PCT), 0));
    let estado = RwSignal::new(None::<(bool, String)>);

    let guardar = Action::new(move |(c, r): &(String, ReglasCategoria)| {
        let (c, r) = (c.clone(), *r);
        async move { (c.clone(), r, guardar_reglas(c, r, None).await) }
    });
    let nombre = nombre_categoria(&categoria);
    Effect::new(move |_| match guardar.value().get() {
        Some((_, _, Ok(()))) => estado.set(Some((true, "Guardado.".into()))),
        // Con el borrado real hace falta la contraseña: la pide el diálogo.
        Some((categoria, reglas, Err(ServerFnError::ServerError(m)))) if m == ERROR_REAUTH => {
            pedir_confirmacion(Peticion {
                titulo: format!("¿Guardar las reglas de {nombre}?"),
                texto: CON_BORRADO_REAL.into(),
                boton: "Guardar",
                que: Delicada::GuardarReglas { categoria, reglas },
            });
        }
        Some((_, _, Err(e))) => estado.set(Some((false, texto_error(&e)))),
        None => {}
    });

    let cat = categoria.clone();
    let enviar = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let dias = |s: &str| {
            numero(s)
                .filter(|v| v.fract() == 0.0 && *v >= 0.0)
                .map(|v| v as u32)
        };
        let leer = || -> Result<ReglasCategoria, &'static str> {
            Ok(ReglasCategoria {
                ratio: if ratio_on.get_untracked() {
                    Some(numero(&ratio.get_untracked()).ok_or("Escribe la ratio.")?)
                } else {
                    None
                },
                siembra_dias: if siembra_on.get_untracked() {
                    Some(
                        dias(&siembra.get_untracked())
                            .ok_or("Escribe los días sembrando, sin decimales.")?,
                    )
                } else {
                    None
                },
                inactividad_dias: if inact_on.get_untracked() {
                    Some(
                        dias(&inact.get_untracked())
                            .ok_or("Escribe los días sin subir, sin decimales.")?,
                    )
                } else {
                    None
                },
                disco_libre_pct: if disco_on.get_untracked() {
                    Some(
                        numero(&disco.get_untracked())
                            .ok_or("Escribe el porcentaje de espacio libre.")?,
                    )
                } else {
                    None
                },
            })
        };
        match leer() {
            Ok(r) => {
                estado.set(None);
                guardar.dispatch((cat.clone(), r));
            }
            Err(m) => estado.set(Some((false, m.into()))),
        }
    };

    let id_titulo = format!("{base}-titulo");
    let id_h2 = id_titulo.clone();
    view! {
        // Las reglas de una categoría, escritas en su sobre de semillas.
        <section class="categoria tablilla" aria-labelledby=id_titulo.clone()>
            <h2 id=id_h2>
                <TiestoVacio tiesto=Tiesto::de_nombre(&nombre_categoria(&categoria)) />
                <span>{nombre_categoria(&categoria)}</span>
            </h2>
            <form on:submit=enviar>
                <Fila
                    id=format!("{base}-ratio")
                    encendida=ratio_on
                    valor=ratio
                    titulo="Ratio"
                    antes="Recoger al llegar a"
                    despues=""
                    paso="0.1"
                />
                <Fila
                    id=format!("{base}-siembra")
                    encendida=siembra_on
                    valor=siembra
                    titulo="Tiempo sembrando"
                    antes="Recoger tras"
                    despues="días"
                    paso="1"
                />
                <Fila
                    id=format!("{base}-inactividad")
                    encendida=inact_on
                    valor=inact
                    titulo="Sin subir nada"
                    antes="Recoger tras"
                    despues="días sin subir"
                    paso="1"
                />
                <Fila
                    id=format!("{base}-disco")
                    encendida=disco_on
                    valor=disco
                    titulo="Espacio en disco"
                    antes="Si queda menos del"
                    despues="% libre, recoger de la más antigua a la más nueva"
                    paso="1"
                />
                <div class="categoria-pie">
                    <p
                        class=move || match estado.get() {
                            Some((false, _)) => "estado-guardar error",
                            _ => "estado-guardar",
                        }
                        role="status"
                    >
                        {move || estado.get().map(|(_, m)| m)}
                    </p>
                    <button
                        type="submit"
                        class="boton boton-acento"
                        disabled=move || guardar.pending().get()
                    >
                        "Guardar"
                    </button>
                </div>
            </form>
        </section>
    }
}

/// Qué formulario de presets está abierto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Abierto {
    Nuevo,
    Cambiar(i64),
}

/// Los presets: guardar la configuración de ahora, subir un archivo y, en
/// cada uno, aplicar, descargar, cambiar y eliminar.
#[component]
fn Presets(
    lista: Vec<FichaPreset>,
    estado: RwSignal<Option<(bool, String)>>,
    /// Trackers con normas propias ahora.
    con_propias: usize,
) -> impl IntoView {
    let recarga = expect_context::<Recarga>().0;
    let abierto = RwSignal::new(None::<Abierto>);
    let archivo = NodeRef::<leptos::html::Input>::new();

    // Lo que guarda o cambia un preset: si sale bien, cierra el formulario,
    // lo dice y recarga la página.
    let hecho = move |r: Result<(), ServerFnError>, ok: &str| match r {
        Ok(()) => {
            abierto.set(None);
            estado.set(Some((true, ok.to_owned())));
            recarga.update(|v| *v += 1);
        }
        Err(e) => estado.set(Some((false, texto_error(&e)))),
    };
    let subir = Action::new(|archivo: &String| subir_preset(archivo.clone()));
    Effect::new(move |_| {
        if let Some(r) = subir.value().get() {
            hecho(r, "Preset subido. «Aplicar» lo pone en marcha.");
        }
    });
    let al_elegir = move |_: leptos::ev::Event| {
        estado.set(None);
        #[cfg(feature = "hydrate")]
        {
            let Some(input) = archivo.get() else { return };
            let Some(f) = input.files().and_then(|l| l.get(0)) else {
                return;
            };
            // Para poder elegir otra vez el mismo archivo.
            input.set_value("");
            if f.size() > reglas::preset::MAX_BYTES as f64 {
                estado.set(Some((
                    false,
                    "El archivo pasa de 64 KiB: un preset es mucho más pequeño.".into(),
                )));
                return;
            }
            leptos::task::spawn_local(async move {
                match wasm_bindgen_futures::JsFuture::from(f.text()).await {
                    Ok(v) => {
                        subir.dispatch(v.as_string().unwrap_or_default());
                    }
                    Err(_) => estado.set(Some((false, "No se pudo leer el archivo.".into()))),
                }
            });
        }
        #[cfg(not(feature = "hydrate"))]
        let _ = archivo;
    };

    let filas = lista
        .into_iter()
        .map(|p| view! { <UnPreset p abierto estado con_propias /> })
        .collect_view();
    view! {
        <section class="seccion exenciones-nota presets" aria-labelledby="titulo-presets">
            <h2 id="titulo-presets">"Presets"</h2>
            <p class="explica">
                "Un preset guarda las normas del tracker (también las de cada tracker) y las reglas de las tres categorías. «Aplicar» cambia todo de una vez; «Descargar» da un archivo .toml para compartirlo, y «Subir preset» lo añade a la lista (64 KiB como mucho)."
            </p>
            <div class="preset-botones">
                <button
                    type="button"
                    class="boton boton-acento"
                    on:click=move |_| {
                        estado.set(None);
                        abierto.set(Some(Abierto::Nuevo));
                    }
                >
                    "Guardar como preset"
                </button>
                <input
                    id="preset-archivo"
                    type="file"
                    accept=".toml,text/plain"
                    class="solo-lectores archivo-preset"
                    node_ref=archivo
                    on:change=al_elegir
                    disabled=move || subir.pending().get()
                />
                <label for="preset-archivo" class="boton">
                    "Subir preset"
                </label>
            </div>
            <Show when=move || abierto.get() == Some(Abierto::Nuevo)>
                <FormPreset
                    nombre=String::new()
                    descripcion=String::new()
                    id=None
                    abierto
                    estado
                />
            </Show>
            <p
                class=move || match estado.get() {
                    Some((false, _)) => "estado-guardar error",
                    _ => "estado-guardar",
                }
                role="status"
            >
                {move || estado.get().map(|(_, m)| m)}
            </p>
            <ul class="presets-lista">{filas}</ul>
        </section>
    }
}

/// Un preset de la lista con sus acciones.
#[component]
fn UnPreset(
    p: FichaPreset,
    abierto: RwSignal<Option<Abierto>>,
    estado: RwSignal<Option<(bool, String)>>,
    con_propias: usize,
) -> impl IntoView {
    let id = p.id;
    let nombre = p.nombre.clone();
    let texto_aplicar = texto_aplicar(con_propias, p.trackers);
    let aplicar = move |_| {
        estado.set(None);
        pedir_confirmacion(Peticion {
            titulo: format!("¿Aplicar «{nombre}»?"),
            texto: texto_aplicar.clone(),
            boton: "Aplicar",
            que: Delicada::AplicarPreset { id },
        });
    };
    let nombre = p.nombre.clone();
    let eliminar = move |_| {
        estado.set(None);
        pedir_confirmacion(Peticion {
            titulo: format!("¿Eliminar «{nombre}»?"),
            texto: "Se borra el preset de la lista. Las normas y las reglas de ahora no cambian."
                .into(),
            boton: "Eliminar",
            que: Delicada::EliminarPreset { id },
        });
    };
    let propios = (!p.incluido).then(|| {
        view! {
            <button
                type="button"
                class="boton"
                on:click=move |_| {
                    estado.set(None);
                    abierto.set(Some(Abierto::Cambiar(id)));
                }
            >
                "Cambiar"
            </button>
            <button type="button" class="boton boton-peligro" on:click=eliminar>
                "Eliminar"
            </button>
        }
    });
    let (nombre, descripcion) = (p.nombre.clone(), p.descripcion.clone());
    view! {
        <li class="preset">
            <p>
                <span class="preset-nombre">{p.nombre}</span>
                {p.incluido.then(|| view! { <span class="preset-incluido">"Incluido"</span> })}
            </p>
            {(!p.descripcion.is_empty()).then(|| view! { <p class="nota">{p.descripcion}</p> })}
            <div class="preset-botones">
                <button type="button" class="boton" on:click=aplicar>
                    "Aplicar"
                </button>
                <a class="boton" href=format!("{RUTA_DESCARGA}?id={id}") download="">
                    "Descargar"
                </a>
                {propios}
            </div>
            <Show when=move || abierto.get() == Some(Abierto::Cambiar(id))>
                <FormPreset
                    nombre=nombre.clone()
                    descripcion=descripcion.clone()
                    id=Some(id)
                    abierto
                    estado
                />
            </Show>
        </li>
    }
}

/// Lo que dice el diálogo al aplicar un preset con `trae` trackers con
/// normas propias cuando ahora hay `hay`.
fn texto_aplicar(hay: usize, trae: usize) -> String {
    let mut t = "Sus normas del tracker y sus reglas sustituyen a las de ahora; las categorías que no traiga quedan con todas las reglas apagadas.".to_owned();
    if hay > 0 || trae > 0 {
        let trackers = |n: usize| match n {
            1 => "1 tracker".to_owned(),
            n => format!("{n} trackers"),
        };
        let quita = (hay > 0).then(|| format!("quita las normas propias de {}", trackers(hay)));
        let pone = (trae > 0).then(|| format!("pone las de {}", trackers(trae)));
        let frase = match (quita, pone) {
            (Some(q), Some(p)) => format!("{q} y {p}"),
            (Some(q), None) => q,
            (None, Some(p)) => p,
            (None, None) => unreachable!(),
        };
        let mut c = frase.chars();
        let frase: String = c
            .next()
            .map(|p| p.to_uppercase().chain(c).collect())
            .unwrap_or_default();
        t.push_str(&format!(" {frase}."));
    }
    t.push_str(" Si quieres conservar lo de ahora, guárdalo antes como preset.");
    t
}

/// Nombre y descripción de un preset: nuevo (`id` vacío, con la
/// configuración de ahora) o uno que se cambia (y, si se marca, se
/// sobrescribe con la configuración de ahora).
#[component]
fn FormPreset(
    nombre: String,
    descripcion: String,
    id: Option<i64>,
    abierto: RwSignal<Option<Abierto>>,
    estado: RwSignal<Option<(bool, String)>>,
) -> impl IntoView {
    let recarga = expect_context::<Recarga>().0;
    let nombre = RwSignal::new(nombre);
    let descripcion = RwSignal::new(descripcion);
    let sobrescribir = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let base = match id {
        Some(i) => format!("preset-{i}"),
        None => "preset-nuevo".to_owned(),
    };
    let enviar = Action::new(move |(n, d, s): &(String, String, bool)| {
        let (n, d, s) = (n.clone(), d.clone(), *s);
        async move {
            match id {
                Some(id) => cambiar_preset(id, n, d, s).await,
                None => guardar_preset(n, d).await,
            }
        }
    });
    Effect::new(move |_| match enviar.value().get() {
        Some(Ok(())) => {
            abierto.set(None);
            estado.set(Some((true, "Guardado.".into())));
            recarga.update(|v| *v += 1);
        }
        Some(Err(e)) => error.set(Some(texto_error(&e))),
        None => {}
    });
    let al_enviar = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        error.set(None);
        enviar.dispatch((
            nombre.get_untracked(),
            descripcion.get_untracked(),
            sobrescribir.get_untracked(),
        ));
    };
    let (id_nombre, id_desc, id_sobre) = (
        format!("{base}-nombre"),
        format!("{base}-descripcion"),
        format!("{base}-sobrescribir"),
    );
    let casilla = id.map(|_| {
        let id_sobre2 = id_sobre.clone();
        view! {
            <div class="regla">
                <input id=id_sobre type="checkbox" role="switch" class="interruptor" bind:checked=sobrescribir />
                <label for=id_sobre2 class="regla-nombre">
                    "Poner en él las normas y las reglas de ahora"
                </label>
            </div>
        }
    });
    view! {
        <form class="preset-form" on:submit=al_enviar>
            <div class="campo">
                <label for=id_nombre.clone()>"Nombre"</label>
                <input
                    id=id_nombre
                    type="text"
                    maxlength="60"
                    required
                    value=nombre.get_untracked()
                    bind:value=nombre
                />
            </div>
            <div class="campo">
                <label for=id_desc.clone()>"Descripción (si quieres)"</label>
                <input
                    id=id_desc
                    type="text"
                    maxlength="300"
                    value=descripcion.get_untracked()
                    bind:value=descripcion
                />
            </div>
            {match id {
                None => {
                    view! {
                        <p class="nota">
                            "Guarda las normas del tracker y las reglas que hay ahora."
                        </p>
                    }
                        .into_any()
                }
                Some(_) => casilla.into_any(),
            }}
            <p class="error" role="alert">
                {move || error.get()}
            </p>
            <div class="botones">
                <button type="button" class="boton" on:click=move |_| abierto.set(None)>
                    "Cancelar"
                </button>
                <button type="submit" class="boton boton-acento" disabled=move || enviar.pending().get()>
                    "Guardar"
                </button>
            </div>
        </form>
    }
}

#[component]
fn Exenciones(lista: Vec<FichaExencion>) -> impl IntoView {
    let acciones = Acciones::del_contexto();
    let cuerpo = if lista.is_empty() {
        view! { <p class="vacio">"Ninguna: todas las reglas cuentan para todas las macetas."</p> }
            .into_any()
    } else {
        let filas = lista
            .into_iter()
            .map(|e| {
                let (hash, regla): (String, TipoRegla) = (e.hash.clone(), e.regla);
                let nombre = if e.nombre.is_empty() {
                    "(sin nombre)".to_owned()
                } else {
                    e.nombre
                };
                view! {
                    <li class="exencion">
                        <p>
                            <span class="exencion-nombre">{nombre}</span>
                            {format!(", regla «{}».", nombre_regla(regla))}
                        </p>
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
        view! { <ul class="exenciones">{filas}</ul> }.into_any()
    };
    view! {
        <section class="seccion exenciones-nota" aria-labelledby="titulo-exenciones">
            <h2 id="titulo-exenciones">"Con «No recoger»"</h2>
            <p class="explica">"Esa regla ya no las recogerá. «Volver a contar» la hace contar otra vez."</p>
            {cuerpo}
        </section>
    }
}
