//! Cliente de las server functions en el navegador: el `fetch` de siempre
//! más la cabecera `x-csrf-token` con el token de la página (de
//! `<meta name="csrf-token">`), que el guardián de `/api` exige en todo lo que
//! cambia algo, `logout` incluido.
//!
//! En el servidor el tipo existe (las server functions lo nombran), pero no
//! se usa: allí se llaman directamente.

use leptos::server_fn::client::Client;
use leptos::server_fn::client::browser::BrowserClient;
use leptos::server_fn::error::FromServerFnError;
use leptos::server_fn::request::browser::BrowserRequest;
use leptos::server_fn::response::browser::BrowserResponse;

/// Nombre de la cabecera con el token (la misma que mira el guardián).
pub const CABECERA_CSRF: &str = "x-csrf-token";

/// `fetch` del navegador con el token CSRF de la página.
pub struct ClienteCsrf;

/// Token de `<meta name="csrf-token">`, si la página lo trae.
#[cfg(feature = "hydrate")]
fn token_de_la_pagina() -> Option<String> {
    leptos::prelude::document()
        .query_selector("meta[name=\"csrf-token\"]")
        .ok()
        .flatten()
        .and_then(|m| m.get_attribute("content"))
}

#[cfg(not(feature = "hydrate"))]
fn token_de_la_pagina() -> Option<String> {
    None
}

impl<E, EIn, EOut> Client<E, EIn, EOut> for ClienteCsrf
where
    E: FromServerFnError,
    EIn: FromServerFnError,
    EOut: FromServerFnError,
{
    type Request = BrowserRequest;
    type Response = BrowserResponse;

    fn send(req: Self::Request) -> impl Future<Output = Result<Self::Response, E>> + Send {
        if let Some(token) = token_de_la_pagina() {
            // Una petición recién creada admite cabeceras nuevas.
            req.headers().set(CABECERA_CSRF, &token);
        }
        <BrowserClient as Client<E, EIn, EOut>>::send(req)
    }

    #[allow(clippy::type_complexity)]
    fn open_websocket(
        path: &str,
    ) -> impl Future<
        Output = Result<
            (
                impl futures::Stream<Item = Result<leptos::server_fn::Bytes, leptos::server_fn::Bytes>>
                + Send
                + 'static,
                impl futures::Sink<leptos::server_fn::Bytes> + Send + 'static,
            ),
            E,
        >,
    > + Send {
        <BrowserClient as Client<E, EIn, EOut>>::open_websocket(path)
    }

    fn spawn(future: impl Future<Output = ()> + Send + 'static) {
        <BrowserClient as Client<E, EIn, EOut>>::spawn(future)
    }
}
