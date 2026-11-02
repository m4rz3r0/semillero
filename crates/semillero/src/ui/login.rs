//! Formulario de acceso, con la cuenta de qBittorrent.

use leptos::prelude::*;

use super::{Cabecera, Seto};
use crate::auth::Login;

#[component]
pub fn PaginaLogin() -> impl IntoView {
    let accion = ServerAction::<Login>::new();
    Effect::new(move |_| {
        if let Some(Ok(())) = accion.value().get() {
            // Recarga completa: la portada sale ya con su token.
            let _ = window().location().set_href("/");
        }
    });
    let error = move || match accion.value().get() {
        Some(Err(ServerFnError::ServerError(msg))) => Some(msg),
        Some(Err(_)) => Some("No se puede conectar con el servidor.".to_owned()),
        _ => None,
    };
    view! {
        <Cabecera />
        <Seto />
        <main class="contenido acceso">
            // Una etiqueta grande clavada en el suelo del jardín.
            <div class="acceso-caja">
                <h1>"Entrar"</h1>
                <ActionForm action=accion>
                    <div class="campo">
                        <label for="usuario">"Usuario"</label>
                        <input
                            id="usuario"
                            name="usuario"
                            type="text"
                            autocomplete="username"
                            autocapitalize="none"
                            spellcheck="false"
                            required
                            autofocus
                        />
                    </div>
                    <div class="campo">
                        <label for="contrasena">"Contraseña"</label>
                        <input
                            id="contrasena"
                            name="contrasena"
                            type="password"
                            autocomplete="current-password"
                            required
                        />
                    </div>
                    <p class="error" role="alert">
                        {error}
                    </p>
                    <button
                        type="submit"
                        class="boton boton-acento"
                        disabled=move || accion.pending().get()
                    >
                        "Entrar"
                    </button>
                </ActionForm>
                <p class="nota">"El mismo usuario y contraseña que qBittorrent."</p>
            </div>
            <span class="estaca acceso-estaca" aria-hidden="true"></span>
        </main>
    }
}
