//! Acceso con la cuenta de qBittorrent: la contraseña se comprueba contra su
//! API y aquí no se guarda nunca. Sesiones en SQLite (clave SHA-256 del id)
//! con la cookie `__Host-sm_session`, freno de intentos fallidos por IP real
//! del cliente (la que manda el proxy inverso, ver [`crate::config`]), canario de
//! arranque y reautenticación para lo delicado.
//!
//! Las server functions `login` y `logout`; el guardián de `/api` (en
//! [`crate::seguridad`]) exige sesión y token a todas salvo `login`.

use leptos::prelude::*;

#[cfg(feature = "ssr")]
mod servidor;
#[cfg(feature = "ssr")]
pub use servidor::*;

/// Código de error (estado 401) sin sesión vigente.
pub const ERROR_SESION: &str = "sesion-caducada";

/// Inicia sesión con el usuario y la contraseña de qBittorrent. Devuelve la
/// cookie de sesión en `Set-Cookie`.
///
/// Sin JavaScript (el formulario enviado antes de hidratar, que pide
/// `text/html`), redirige: si entra, a la portada; si no, de vuelta al
/// formulario.
#[server(endpoint = "login")]
pub async fn login(usuario: String, contrasena: String) -> Result<(), ServerFnError> {
    servidor::login_endpoint(usuario, contrasena).await
}

/// Cierra la sesión de la petición (con el token CSRF, como todo lo que
/// cambia algo).
#[server(endpoint = "logout", client = crate::cliente::ClienteCsrf)]
pub async fn logout() -> Result<(), ServerFnError> {
    servidor::logout_endpoint().await
}
