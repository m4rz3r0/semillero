//! Interfaz «Vivero» (un jardín de macetas): marco común, diálogo de
//! confirmación y las páginas.
//!
//! Los datos llegan ya en palabras (ver [`crate::api`]); aquí no aparece
//! ningún nombre de la API de qBittorrent.

pub mod anadir;
pub mod formato;
pub mod historial;
pub mod login;
pub mod maceta;
pub mod portada;
pub mod reglas;
pub mod tiempo;

use std::future::Future;

use ::reglas::{NormasTracker, ReglasCategoria};
use leptos::prelude::*;
use leptos_router::components::A;

use crate::api::{
    ERROR_REAUTH, aplicar_preset, cambiar_modo, eliminar_preset, guardar_normas,
    guardar_normas_tracker, guardar_reglas, quitar, quitar_normas_tracker,
};
use crate::auth::{ERROR_SESION, Logout};

/// Cambia cada vez que algo se modifica: las páginas vuelven a pedir sus
/// datos.
#[derive(Clone, Copy)]
pub struct Recarga(pub RwSignal<u32>);

/// Último mensaje para la persona (un error de una acción).
#[derive(Clone, Copy)]
pub struct Mensaje(pub RwSignal<Option<String>>);

/// La maceta que se acaba de recoger (su hash): se va con un pequeño
/// gesto antes de recargar el jardín.
#[derive(Clone, Copy)]
pub struct Saliendo(pub RwSignal<Option<String>>);

/// Acción delicada pendiente de confirmar en el diálogo.
#[derive(Clone, Copy)]
pub struct Confirmar(pub RwSignal<Option<Peticion>>);

/// Lo que pide el diálogo de confirmación.
#[derive(Clone, Debug, PartialEq)]
pub struct Peticion {
    pub titulo: String,
    pub texto: String,
    pub boton: &'static str,
    pub que: Delicada,
}

/// Acciones que piden confirmar (y la contraseña, si hace más de 5
/// minutos que no se escribe).
#[derive(Clone, Debug, PartialEq)]
pub enum Delicada {
    Quitar {
        hash: String,
        ficheros: bool,
    },
    BorradoReal,
    /// Sus normas y reglas sustituyen a las de ahora.
    AplicarPreset {
        id: i64,
    },
    EliminarPreset {
        id: i64,
    },
    /// Con el borrado real activo; el diálogo pide la contraseña. Sin
    /// `tracker`, las de por defecto.
    GuardarNormas {
        normas: NormasTracker,
        tracker: Option<String>,
    },
    /// Con el borrado real activo; el diálogo pide la contraseña.
    QuitarNormasTracker {
        tracker: String,
    },
    /// Con el borrado real activo; el diálogo pide la contraseña.
    GuardarReglas {
        categoria: String,
        reglas: ReglasCategoria,
    },
}

impl Delicada {
    /// Solo se pide confirmar después de que el servidor pidiera la
    /// contraseña: el diálogo la pide ya al abrirse.
    fn pide_clave_al_abrir(&self) -> bool {
        matches!(
            self,
            Delicada::GuardarNormas { .. }
                | Delicada::GuardarReglas { .. }
                | Delicada::QuitarNormasTracker { .. }
        )
    }
}

/// Los contextos de la interfaz; [`crate::app::App`] los crea una vez.
pub fn dar_contextos() {
    provide_context(Recarga(RwSignal::new(0)));
    provide_context(Mensaje(RwSignal::new(None)));
    provide_context(Confirmar(RwSignal::new(None)));
    provide_context(Saliendo(RwSignal::new(None)));
}

/// Texto de un error de una server function. Si la sesión caducó, lleva al
/// formulario de acceso.
pub fn texto_error(e: &ServerFnError) -> String {
    let texto = match e {
        ServerFnError::ServerError(m) => m.clone(),
        otro => otro.to_string(),
    };
    if texto.contains(ERROR_SESION) {
        #[cfg(feature = "hydrate")]
        let _ = window().location().set_href("/login");
        return "La sesión ha caducado.".into();
    }
    if matches!(e, ServerFnError::ServerError(_)) {
        texto
    } else {
        "No se puede conectar con el servidor.".into()
    }
}

/// Lanza acciones sencillas: si salen bien, recarga los datos; si no,
/// enseña el error.
#[derive(Clone, Copy)]
pub struct Acciones {
    recarga: Recarga,
    mensaje: Mensaje,
}

impl Acciones {
    pub fn del_contexto() -> Self {
        Self {
            recarga: expect_context(),
            mensaje: expect_context(),
        }
    }

    pub fn lanzar(self, f: impl Future<Output = Result<(), ServerFnError>> + 'static) {
        self.mensaje.0.set(None);
        leptos::task::spawn_local(async move {
            match f.await {
                Ok(()) => self.recarga.0.update(|v| *v += 1),
                Err(e) => self.mensaje.0.set(Some(texto_error(&e))),
            }
        });
    }
}

/// Pide confirmar `p` en el diálogo.
pub fn pedir_confirmacion(p: Peticion) {
    expect_context::<Confirmar>().0.set(Some(p));
}

/// Marco de las páginas con sesión: la cabecera (la misma en todas), el
/// seto del jardín, los mensajes y el diálogo de confirmación. Debajo del
/// seto, el suelo del jardín llega hasta el final de la pantalla.
///
/// - `cabeza`: lo que va en la cabecera bajo la fila de la marca (en el
///   jardín, el estado en una frase).
/// - `escena`: la página es una escena que llena la pantalla y no se
///   desplaza entera (el jardín: solo se desplaza el bancal).
#[component]
pub fn Marco(
    children: Children,
    #[prop(optional, into)] cabeza: Option<ViewFn>,
    #[prop(optional)] escena: bool,
) -> impl IntoView {
    let mensaje = expect_context::<Mensaje>().0;
    let clase_main = if escena {
        "contenido contenido-escena"
    } else {
        "contenido"
    };
    view! {
        <a class="saltar" href="#contenido">
            "Saltar al contenido"
        </a>
        <Cabecera sesion=true cabeza />
        <Seto />
        <div class="mensaje" role="status" aria-live="polite">
            {move || {
                mensaje
                    .get()
                    .map(|m| {
                        view! {
                            <p class="aviso-error">
                                <span>{m}</span>
                                <button
                                    type="button"
                                    class="boton boton-plano"
                                    on:click=move |_| mensaje.set(None)
                                >
                                    "Cerrar"
                                </button>
                            </p>
                        }
                    })
            }}
        </div>
        <main id="contenido" class=clase_main>
            {children()}
        </main>
        <Confirmacion />
    }
}

/// La cabecera, igual en todas las páginas (también en la de acceso): el
/// mismo ancho, el mismo relleno y una sola fila, también en el móvil. Con
/// sesión lleva las secciones, el tema y «Salir».
#[component]
pub fn Cabecera(
    #[prop(optional)] sesion: bool,
    #[prop(default = None)] cabeza: Option<ViewFn>,
) -> impl IntoView {
    let resto = sesion.then(|| {
        view! {
            <nav class="navegacion" aria-label="Secciones">
                <A href="/" exact=true>
                    "Jardín"
                </A>
                <A href="/anadir">"Añadir"</A>
                <A href="/reglas">"Reglas"</A>
                <A href="/historial">"Historial"</A>
            </nav>
            <div class="cabecera-fin">
                <BotonTema />
                <Salir />
            </div>
        }
    });
    view! {
        <header class="cabecera">
            <div class="cabecera-fila">
                <a class="marca" href="/">
                    <Brote />
                    <span class="marca-nombre">"Semillero"</span>
                </a>
                {resto}
            </div>
            {cabeza.map(|c| view! { <div class="cabecera-estado">{c.run()}</div> })}
        </header>
    }
}

/// El seto del fondo del jardín, de borde a borde, bajo la cabecera: matas
/// redondeadas de alturas distintas (siempre las mismas). Debajo empieza
/// el suelo, el mismo en todas las páginas. (La luna, de noche, va en el
/// tiempo del jardín.)
#[component]
pub fn Seto() -> impl IntoView {
    const MATAS: [(f64, f64); 9] = [
        (34.0, 22.0),
        (52.0, 30.0),
        (40.0, 18.0),
        (60.0, 26.0),
        (46.0, 34.0),
        (38.0, 20.0),
        (56.0, 28.0),
        (44.0, 24.0),
        (50.0, 31.0),
    ];
    // Matas de izquierda a derecha hasta cubrir 1600 de ancho.
    let mut d = String::from("M0 64 L0 40");
    let (mut x, mut k) = (0.0_f64, 0usize);
    while x < 1600.0 {
        let (ancho, alto) = MATAS[k % MATAS.len()];
        let y = 46.0 - alto;
        d.push_str(&format!(
            " Q{:.0} {:.0} {:.0} {:.0} Q{:.0} {:.0} {:.0} 40",
            x + ancho * 0.1,
            y,
            x + ancho * 0.5,
            y,
            x + ancho * 0.9,
            y,
            x + ancho
        ));
        x += ancho;
        k += 1;
    }
    d.push_str(&format!(" L{x:.0} 64 Z"));
    view! {
        <svg
            class="seto"
            viewBox="0 0 1600 64"
            preserveAspectRatio="xMidYMax slice"
            aria-hidden="true"
            focusable="false"
        >
            <path class="seto-matas" d=d />
            <path class="seto-linde" d="M0 58 Q400 52 800 57 T1600 56 L1600 64 L0 64 Z" />
        </svg>
    }
}

/// El brote de la marca.
#[component]
pub fn Brote() -> impl IntoView {
    view! {
        <svg class="brote" viewBox="0 0 32 32" aria-hidden="true" focusable="false">
            <path class="brote-tallo" d="M16 29 V15" />
            <path class="brote-hoja" d="M16 17 C 8 17 5 11 6 5 C 13 5 16 10 16 17 Z" />
            <path class="brote-hoja2" d="M16 14 C 18 8 22 6 27 7 C 27 13 22 15 16 14 Z" />
            <path class="brote-tierra" d="M8 29 H24" />
        </svg>
    }
}

/// Cierra la sesión (con el token CSRF) y vuelve al formulario de acceso.
#[component]
fn Salir() -> impl IntoView {
    let accion = ServerAction::<Logout>::new();
    Effect::new(move |_| {
        if accion.value().get().is_some() {
            let _ = window().location().set_href("/login");
        }
    });
    view! {
        <button
            type="button"
            class="boton boton-plano boton-icono"
            title="Salir"
            aria-label="Salir"
            disabled=move || accion.pending().get()
            on:click=move |_| {
                accion.dispatch(Logout {});
            }
        >
            // Una puerta entreabierta y la flecha que sale por ella.
            <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
                <path class="salir-poste" d="M13 4 H5 V20 H13" />
                <path class="salir-hoja" d="M10 12 H21 M17.5 8.5 L21 12 L17.5 15.5" />
            </svg>
        </button>
    }
}

/// Alterna el tema: claro, oscuro y el del sistema. Se guarda en la cookie
/// `sm_tema`, que el servidor pone en `<html data-tema>`.
#[component]
fn BotonTema() -> impl IntoView {
    view! {
        <button
            type="button"
            class="boton boton-plano boton-icono"
            title="Tema: claro, oscuro o el del sistema"
            aria-label="Cambiar el tema (claro, oscuro o el del sistema)"
            on:click=move |_| cambiar_tema()
        >
            <svg viewBox="0 0 24 24" aria-hidden="true" focusable="false">
                <circle cx="12" cy="12" r="8" class="tema-borde" />
                <path d="M12 4 A 8 8 0 0 1 12 20 Z" class="tema-mitad" />
            </svg>
        </button>
    }
}

/// Valores de la cookie del tema.
pub const TEMAS: [&str; 2] = ["claro", "oscuro"];

fn cambiar_tema() {
    #[cfg(feature = "hydrate")]
    {
        use leptos::wasm_bindgen::JsCast;
        let doc = document();
        let Some(raiz) = doc.document_element() else {
            return;
        };
        let siguiente = match raiz.get_attribute("data-tema").as_deref() {
            Some("claro") => "oscuro",
            Some("oscuro") => "sistema",
            _ => "claro",
        };
        if TEMAS.contains(&siguiente) {
            let _ = raiz.set_attribute("data-tema", siguiente);
        } else {
            let _ = raiz.remove_attribute("data-tema");
        }
        let html: web_sys::HtmlDocument = doc.unchecked_into();
        let _ = html.set_cookie(&format!(
            "sm_tema={siguiente}; Path=/; Max-Age=31536000; SameSite=Strict"
        ));
    }
}

/// Diálogo para confirmar lo delicado. Si el servidor dice que hace falta
/// la contraseña ([`ERROR_REAUTH`]), la pide y repite.
#[component]
fn Confirmacion() -> impl IntoView {
    let peticion = expect_context::<Confirmar>().0;
    let recarga = expect_context::<Recarga>().0;
    let saliendo = expect_context::<Saliendo>().0;
    let dialogo = NodeRef::<leptos::html::Dialog>::new();
    let pide_clave = RwSignal::new(false);
    let clave = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let enviar = Action::new(move |(que, clave): &(Delicada, Option<String>)| {
        let (que, clave) = (que.clone(), clave.clone());
        async move {
            match que {
                Delicada::Quitar { hash, ficheros } => quitar(hash, ficheros, clave).await,
                Delicada::BorradoReal => cambiar_modo(true, clave).await,
                Delicada::AplicarPreset { id } => aplicar_preset(id, clave).await,
                Delicada::EliminarPreset { id } => eliminar_preset(id).await,
                Delicada::GuardarNormas {
                    normas,
                    tracker: None,
                } => guardar_normas(normas, clave).await,
                Delicada::GuardarNormas {
                    normas,
                    tracker: Some(h),
                } => guardar_normas_tracker(h, normas, clave).await,
                Delicada::QuitarNormasTracker { tracker } => {
                    quitar_normas_tracker(tracker, clave).await
                }
                Delicada::GuardarReglas { categoria, reglas } => {
                    guardar_reglas(categoria, reglas, clave).await
                }
            }
        }
    });

    // Abrir y cerrar el diálogo según la petición.
    Effect::new(move |_| {
        let abierta = peticion.with(Option::is_some);
        let Some(d) = dialogo.get() else { return };
        if abierta {
            pide_clave.set(
                peticion
                    .with_untracked(|p| p.as_ref().is_some_and(|p| p.que.pide_clave_al_abrir())),
            );
            clave.set(String::new());
            error.set(None);
            if !d.open() {
                let _ = d.show_modal();
            }
        } else if d.open() {
            d.close();
        }
    });
    // Resultado.
    Effect::new(move |_| match enviar.value().get() {
        Some(Ok(())) => {
            clave.set(String::new());
            let recogida = match peticion.get_untracked().map(|p| p.que) {
                Some(Delicada::Quitar { hash, .. }) => Some(hash),
                _ => None,
            };
            peticion.set(None);
            match recogida {
                // La maceta se va (levanta y se desvanece) y luego se
                // recarga el jardín.
                Some(hash) => {
                    saliendo.set(Some(hash));
                    #[cfg(feature = "hydrate")]
                    set_timeout(
                        move || recarga.update(|v| *v += 1),
                        std::time::Duration::from_millis(450),
                    );
                    #[cfg(not(feature = "hydrate"))]
                    recarga.update(|v| *v += 1);
                }
                None => recarga.update(|v| *v += 1),
            }
        }
        Some(Err(ServerFnError::ServerError(m))) if m == ERROR_REAUTH => {
            pide_clave.set(true);
            error.set(None);
        }
        Some(Err(e)) => error.set(Some(texto_error(&e))),
        None => {}
    });

    let cerrar = move || {
        if peticion.with_untracked(Option::is_some) {
            peticion.set(None);
        }
    };
    let confirmar = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let Some(p) = peticion.get_untracked() else {
            return;
        };
        let c = pide_clave.get_untracked().then(|| clave.get_untracked());
        enviar.dispatch((p.que, c));
    };
    let datos = move || peticion.get();

    view! {
        <dialog
            class="dialogo"
            node_ref=dialogo
            aria-labelledby="dialogo-titulo"
            on:close=move |_| cerrar()
        >
            <form on:submit=confirmar>
                <h2 id="dialogo-titulo">{move || datos().map(|p| p.titulo)}</h2>
                <p>{move || datos().map(|p| p.texto)}</p>
                <Show when=move || pide_clave.get()>
                    <div class="campo">
                        <label for="dialogo-clave">"Tu contraseña"</label>
                        <input
                            id="dialogo-clave"
                            type="password"
                            autocomplete="current-password"
                            required
                            autofocus
                            bind:value=clave
                        />
                        <p class="nota">
                            "La misma de qBittorrent. Durante 5 minutos no se vuelve a pedir."
                        </p>
                    </div>
                </Show>
                <p class="error" role="alert">
                    {move || error.get()}
                </p>
                <div class="botones">
                    <button type="button" class="boton" on:click=move |_| cerrar()>
                        "Cancelar"
                    </button>
                    <button
                        type="submit"
                        class="boton boton-peligro"
                        disabled=move || enviar.pending().get()
                    >
                        {move || datos().map(|p| p.boton)}
                    </button>
                </div>
            </form>
        </dialog>
    }
}
