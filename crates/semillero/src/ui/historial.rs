//! Historial, como el cuaderno del jardinero abierto sobre el suelo del
//! jardín: avisos, «No recoger», recogidas, pausas y cambios de modo, un
//! renglón cada uno y la fecha en el margen.

use leptos::prelude::*;

use super::formato::{hace, iso};
use super::{Marco, Recarga, texto_error};
use crate::api::{DatosHistorial, LineaHistorial, Suceso, historial, nombre_regla};

#[component]
pub fn PaginaHistorial() -> impl IntoView {
    let recarga = expect_context::<Recarga>().0;
    let datos = Resource::new(move || recarga.get(), |_| historial());
    view! {
        <Marco>
            <div class="pagina">
                <h1 class="titulo-pagina">"Historial"</h1>
                <Transition fallback=|| view! { <p class="cargando">"Cargando…"</p> }>
                    {move || Suspend::new(async move {
                        match datos.await {
                            Ok(h) => view! { <Lista h /> }.into_any(),
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

fn clase(s: Suceso) -> &'static str {
    match s {
        Suceso::Borrado | Suceso::Quitado => "linea linea-borrado",
        Suceso::Aviso | Suceso::SeHabriaBorrado => "linea linea-aviso",
        Suceso::ModoCambiado | Suceso::PresetAplicado => "linea linea-general",
        _ => "linea",
    }
}

#[component]
fn Lista(h: DatosHistorial) -> impl IntoView {
    if h.lineas.is_empty() {
        return view! {
            <div class="cuaderno cuaderno-vacio">
                <Plantones />
                <p class="vacio">"Aún no ha pasado nada."</p>
                <p class="nota">
                    "Aquí se apuntan los avisos, las recogidas, las pausas y los cambios de modo."
                </p>
            </div>
        }
        .into_any();
    }
    let ahora = h.ahora;
    let filas = h
        .lineas
        .into_iter()
        .map(|l: LineaHistorial| {
            let quien = if l.general {
                "Ajuste general".to_owned()
            } else if l.nombre.is_empty() {
                "(sin nombre)".to_owned()
            } else {
                l.nombre
            };
            let regla = l.regla.map(|r| format!("Regla «{}». ", nombre_regla(r)));
            view! {
                <li class=clase(l.suceso)>
                    <time class="linea-cuando" datetime=iso(l.ts)>
                        {hace(ahora, l.ts)}
                    </time>
                    <p class="linea-texto">
                        <strong class="linea-suceso">{l.suceso.texto()}</strong>
                        ", "
                        <span class=if l.general {
                            "linea-quien linea-quien-general"
                        } else {
                            "linea-quien"
                        }>{quien}</span>
                        "."
                    </p>
                    <p class="linea-detalle">{regla} {l.detalle}</p>
                </li>
            }
        })
        .collect_view();
    view! {
        <div class="cuaderno">
            <ol class="historial">{filas}</ol>
        </div>
    }
    .into_any()
}

/// Un dibujo pequeño para el cuaderno vacío: un lápiz junto a dos
/// plantones recién puestos.
#[component]
fn Plantones() -> impl IntoView {
    view! {
        <svg class="plantones" viewBox="0 0 120 56" aria-hidden="true" focusable="false">
            <path class="plantones-tierra" d="M6 50 Q60 43 114 50" />
            <path class="plantones-tallo" d="M34 48 V30 M64 47 V24" />
            <path class="plantones-hoja" d="M34 34 C26 34 23 28 24 22 C31 22 34 27 34 34 Z" />
            <path class="plantones-hoja" d="M34 31 C36 25 40 23 45 24 C45 30 40 32 34 31 Z" />
            <path class="plantones-hoja" d="M64 29 C55 29 52 22 53 15 C61 15 64 21 64 29 Z" />
            <path class="plantones-hoja" d="M64 26 C66 19 71 17 77 18 C77 25 71 27 64 26 Z" />
            <path class="plantones-lapiz" d="M86 46 L108 22 L113 27 L91 51 L85 52 Z" />
        </svg>
    }
}
