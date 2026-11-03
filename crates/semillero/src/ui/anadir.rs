//! Añadir torrents: archivos `.torrent` o enlaces magnet, con una categoría
//! de qBittorrent, sin pasar por su web. El navegador lee los archivos (en
//! el wasm, sin scripts en línea) y los manda en base64.

use leptos::prelude::*;

use super::{Marco, Recarga, texto_error};
use crate::api::{
    ArchivoSubido, DatosAnadir, MAX_ARCHIVOS, MAX_BYTES_TORRENT, MAX_MAGNETS, anadir_torrents,
    pagina_anadir,
};

#[component]
pub fn PaginaAnadir() -> impl IntoView {
    let recarga = expect_context::<Recarga>().0;
    let datos = Resource::new(move || recarga.get(), |_| pagina_anadir());
    view! {
        <Marco>
            <div class="pagina">
                <h1 class="titulo-pagina">"Añadir torrents"</h1>
                <Transition fallback=|| view! { <p class="cargando">"Cargando…"</p> }>
                    {move || Suspend::new(async move {
                        match datos.await {
                            Ok(d) => view! { <Formulario d /> }.into_any(),
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

/// Lo que se manda: archivos, enlaces y categoría.
type Envio = (Vec<ArchivoSubido>, String, String);

#[component]
fn Formulario(d: DatosAnadir) -> impl IntoView {
    let input = NodeRef::<leptos::html::Input>::new();
    let elegidos = RwSignal::new(Vec::<ArchivoSubido>::new());
    let leyendo = RwSignal::new(false);
    let magnets = RwSignal::new(String::new());
    let categoria = RwSignal::new(String::new());
    // (bien, mensaje)
    let estado = RwSignal::new(None::<(bool, String)>);

    let anadir = Action::new(|(archivos, magnets, categoria): &Envio| {
        anadir_torrents(archivos.clone(), magnets.clone(), categoria.clone())
    });
    Effect::new(move |_| match anadir.value().get() {
        Some(Ok(r)) => {
            estado.set(Some((
                true,
                format!("Añadidos a qBittorrent: {}.", r.anadidos.join(", ")),
            )));
            elegidos.set(Vec::new());
            magnets.set(String::new());
            if let Some(i) = input.get_untracked() {
                i.set_value("");
            }
        }
        Some(Err(e)) => estado.set(Some((false, texto_error(&e)))),
        None => {}
    });

    let al_elegir = move |_: leptos::ev::Event| {
        estado.set(None);
        elegidos.set(Vec::new());
        #[cfg(feature = "hydrate")]
        leer_archivos(input, elegidos, leyendo, estado);
    };
    let al_enviar = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        estado.set(None);
        anadir.dispatch((
            elegidos.get_untracked(),
            magnets.get_untracked(),
            categoria.get_untracked(),
        ));
    };
    let ocupado = move || leyendo.get() || anadir.pending().get();

    let opciones = d
        .categorias
        .into_iter()
        .map(|c| {
            let valor = c.clone();
            view! { <option value=valor>{c}</option> }
        })
        .collect_view();
    view! {
        <section class="seccion exenciones-nota" aria-labelledby="titulo-anadir">
            <h2 id="titulo-anadir">"Nuevos torrents"</h2>
            <p class="explica">
                {format!(
                    "Sube archivos .torrent o pega enlaces magnet, uno por línea, y elige su categoría: van directos a qBittorrent. Como mucho {MAX_ARCHIVOS} archivos de {} MiB y {MAX_MAGNETS} enlaces de una vez. Las direcciones web (http o https) no valen. Si alguno no vale, no se añade ninguno.",
                    MAX_BYTES_TORRENT / (1024 * 1024),
                )}
            </p>
            <form class="preset-form" on:submit=al_enviar>
                <div class="campo">
                    <span>"Archivos .torrent"</span>
                    <div class="preset-botones">
                        <input
                            id="anadir-archivos"
                            type="file"
                            accept=".torrent,application/x-bittorrent"
                            multiple
                            class="solo-lectores archivo-preset"
                            node_ref=input
                            on:change=al_elegir
                            disabled=ocupado
                        />
                        <label for="anadir-archivos" class="boton">
                            "Elegir archivos"
                        </label>
                    </div>
                    <p class="nota">
                        {move || {
                            if leyendo.get() {
                                return "Leyendo los archivos…".to_owned();
                            }
                            let v = elegidos.get();
                            if v.is_empty() {
                                "Ningún archivo elegido.".to_owned()
                            } else {
                                let nombres: Vec<String> = v.into_iter().map(|a| a.nombre).collect();
                                format!("Elegidos: {}.", nombres.join(", "))
                            }
                        }}
                    </p>
                </div>
                <div class="campo">
                    <label for="anadir-magnets">"Enlaces magnet (uno por línea)"</label>
                    <textarea
                        id="anadir-magnets"
                        rows="5"
                        spellcheck="false"
                        autocomplete="off"
                        placeholder="magnet:?xt=urn:btih:…"
                        prop:value=move || magnets.get()
                        on:input=move |ev| magnets.set(event_target_value(&ev))
                    ></textarea>
                </div>
                <div class="campo">
                    <label for="anadir-categoria">"Categoría"</label>
                    <select
                        id="anadir-categoria"
                        prop:value=move || categoria.get()
                        on:change=move |ev| categoria.set(event_target_value(&ev))
                    >
                        <option value="">"Sin categoría"</option>
                        {opciones}
                    </select>
                </div>
                <div class="preset-botones">
                    <button type="submit" class="boton boton-acento" disabled=ocupado>
                        "Añadir"
                    </button>
                </div>
            </form>
            <p
                class=move || match estado.get() {
                    Some((false, _)) => "estado-guardar error",
                    _ => "estado-guardar",
                }
                role="status"
            >
                {move || estado.get().map(|(_, m)| m)}
            </p>
        </section>
    }
}

/// Lee los archivos elegidos y los deja en `elegidos`, en base64. Si son
/// demasiados o alguno pasa de [`MAX_BYTES_TORRENT`], lo dice en `estado`
/// y no lee ninguno (el servidor lo vuelve a comprobar).
#[cfg(feature = "hydrate")]
fn leer_archivos(
    input: NodeRef<leptos::html::Input>,
    elegidos: RwSignal<Vec<ArchivoSubido>>,
    leyendo: RwSignal<bool>,
    estado: RwSignal<Option<(bool, String)>>,
) {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use wasm_bindgen_futures::{JsFuture, js_sys};

    let Some(input) = input.get() else { return };
    let Some(lista) = input.files() else { return };
    let fallo = |m: String| {
        input.set_value("");
        estado.set(Some((false, m)));
    };
    if lista.length() as usize > MAX_ARCHIVOS {
        return fallo(format!(
            "Como mucho {MAX_ARCHIVOS} archivos .torrent de una vez."
        ));
    }
    let archivos: Vec<web_sys::File> = (0..lista.length()).filter_map(|i| lista.get(i)).collect();
    if let Some(f) = archivos
        .iter()
        .find(|f| f.size() > MAX_BYTES_TORRENT as f64)
    {
        return fallo(format!(
            "El archivo «{}» pasa de {} MiB.",
            f.name(),
            MAX_BYTES_TORRENT / (1024 * 1024)
        ));
    }
    leyendo.set(true);
    leptos::task::spawn_local(async move {
        let mut v = Vec::with_capacity(archivos.len());
        for f in archivos {
            match JsFuture::from(f.array_buffer()).await {
                Ok(b) => v.push(ArchivoSubido {
                    nombre: f.name(),
                    datos: STANDARD.encode(js_sys::Uint8Array::new(&b).to_vec()),
                }),
                Err(_) => {
                    leyendo.set(false);
                    estado.set(Some((
                        false,
                        format!("No se pudo leer el archivo «{}».", f.name()),
                    )));
                    return;
                }
            }
        }
        elegidos.set(v);
        leyendo.set(false);
    });
}
