//! Aplicación Leptos: documento HTML y rutas. Las páginas están en
//! [`crate::ui`].

use leptos::prelude::*;
use leptos_meta::{HashedStylesheet, MetaTags, Title, provide_meta_context};
use leptos_router::components::{Route, Router, Routes};
use leptos_router::{SsrMode, StaticSegment};

use crate::ui::anadir::PaginaAnadir;
use crate::ui::historial::PaginaHistorial;
use crate::ui::login::PaginaLogin;
use crate::ui::portada::PaginaPortada;
use crate::ui::reglas::PaginaReglas;
use crate::ui::{Cabecera, Seto};

/// Documento HTML que envuelve la aplicación en el servidor. Fija la CSP de
/// la página con el nonce de los scripts en línea de Leptos y, con sesión,
/// deja el token CSRF en `<meta name="csrf-token">` para las peticiones que
/// cambian algo.
pub fn shell(options: LeptosOptions) -> impl IntoView {
    #[cfg(feature = "ssr")]
    csp_con_nonce();
    #[cfg(feature = "ssr")]
    let csrf = token_de_la_pagina();
    #[cfg(not(feature = "ssr"))]
    let csrf: Option<String> = None;
    #[cfg(feature = "ssr")]
    let tema = tema_de_la_pagina();
    #[cfg(not(feature = "ssr"))]
    let tema: Option<&'static str> = None;
    view! {
        <!DOCTYPE html>
        <html lang="es" data-tema=tema>
            <head>
                <meta charset="utf-8" />
                <meta name="viewport" content="width=device-width, initial-scale=1" />
                <meta name="color-scheme" content="light dark" />
                <link
                    rel="preload"
                    href="/fuentes/alegreya-sans-latin-400-normal.woff2"
                    r#as="font"
                    r#type="font/woff2"
                    crossorigin=""
                />
                <link
                    rel="preload"
                    href="/fuentes/alegreya-latin-700-normal.woff2"
                    r#as="font"
                    r#type="font/woff2"
                    crossorigin=""
                />
                {csrf.map(|t| view! { <meta name="csrf-token" content=t /> })}
                <AutoReload options=options.clone() />
                <HashedStylesheet options=options.clone() id="leptos" />
                <HydrationScripts options />
                <MetaTags />
            </head>
            <body>
                <App />
            </body>
        </html>
    }
}

#[cfg(feature = "ssr")]
fn csp_con_nonce() {
    use axum::http::{HeaderValue, header};
    use leptos::nonce::use_nonce;
    use leptos_axum::ResponseOptions;

    let (Some(nonce), Some(resp)) = (use_nonce(), use_context::<ResponseOptions>()) else {
        return;
    };
    if let Ok(valor) = HeaderValue::from_str(&crate::seguridad::csp(Some(&nonce))) {
        resp.insert_header(header::CONTENT_SECURITY_POLICY, valor);
    }
}

#[cfg(feature = "ssr")]
fn token_de_la_pagina() -> Option<String> {
    use_context::<axum::http::request::Parts>()
        .and_then(|p| p.extensions.get::<crate::auth::Sesion>().cloned())
        .map(|s| s.csrf)
}

/// Tema elegido en la cookie `sm_tema` (ver [`crate::ui`]); sin ella, el
/// del sistema.
#[cfg(feature = "ssr")]
fn tema_de_la_pagina() -> Option<&'static str> {
    let partes = use_context::<axum::http::request::Parts>()?;
    partes
        .headers
        .get_all(axum::http::header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|par| par.trim().split_once('='))
        .filter(|(nombre, _)| *nombre == "sm_tema")
        .find_map(|(_, valor)| crate::ui::TEMAS.into_iter().find(|t| *t == valor))
}

/// Raíz de la interfaz.
#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();
    crate::ui::dar_contextos();
    view! {
        <Title text="Semillero" />
        <Router>
            <Routes fallback=NoEncontrada>
                <Route path=StaticSegment("login") view=PaginaLogin />
                <Route path=StaticSegment("") view=PaginaPortada ssr=SsrMode::Async />
                <Route path=StaticSegment("anadir") view=PaginaAnadir ssr=SsrMode::Async />
                <Route path=StaticSegment("reglas") view=PaginaReglas ssr=SsrMode::Async />
                <Route
                    path=StaticSegment("historial")
                    view=PaginaHistorial
                    ssr=SsrMode::Async
                />
            </Routes>
        </Router>
    }
}

#[component]
fn NoEncontrada() -> impl IntoView {
    view! {
        <Cabecera />
        <Seto />
        <main class="contenido acceso">
            <div class="acceso-caja">
                <h1>"Esta página no existe"</h1>
                <p>
                    <a href="/">"Volver al jardín"</a>
                </p>
            </div>
        </main>
    }
}
