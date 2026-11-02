//! El tiempo del jardín: la VPN, el puerto y los discos, dichos como el
//! tiempo que hace sobre el huerto.
//!
//! - VPN y puerto bien: buen tiempo (sol de día, luna de noche).
//! - Puerto cerrado: nublado; llega poca gente a por las semillas.
//! - VPN caída: tormenta; el jardín está cerrado.
//! - Un disco con poco sitio: tierra seca, además del tiempo que haga.
//!
//! El dibujo del cielo es decoración; lo que se lee (y se anuncia) es el
//! letrero de madera. Cuando algo falla, debajo va la causa en palabras
//! llanas y que las cuentas atrás están paradas.

use leptos::prelude::*;

use crate::api::{FichaDisco, Franja};

/// El tiempo que hace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cielo {
    Bueno,
    Nublado,
    Tormenta,
}

impl Cielo {
    /// Sin VPN no hay nada que hacer (tormenta); con la VPN y sin puerto,
    /// nublado.
    pub fn de(f: &Franja) -> Self {
        if !f.vigilada {
            Cielo::Bueno
        } else if !f.vpn {
            Cielo::Tormenta
        } else if !f.puerto {
            Cielo::Nublado
        } else {
            Cielo::Bueno
        }
    }

    fn clase(self) -> &'static str {
        match self {
            Cielo::Bueno => "tiempo tiempo-bueno",
            Cielo::Nublado => "tiempo tiempo-nublado",
            Cielo::Tormenta => "tiempo tiempo-tormenta",
        }
    }

    /// La frase del letrero.
    pub fn frase(self) -> &'static str {
        match self {
            Cielo::Bueno => "Buen tiempo: el jardín riega y comparte.",
            Cielo::Nublado => "Nublado: llega poca gente a por las semillas.",
            Cielo::Tormenta => "Tormenta: el jardín está cerrado.",
        }
    }

    /// La causa, en palabras llanas (solo si algo falla).
    pub fn causa(self) -> Option<String> {
        const PARADAS: &str =
            "Las cuentas atrás están paradas y no se recoge nada automáticamente.";
        match self {
            Cielo::Bueno => None,
            Cielo::Nublado => Some(format!("El puerto de la VPN está cerrado. {PARADAS}")),
            Cielo::Tormenta => Some(format!("La VPN no responde. {PARADAS}")),
        }
    }
}

/// Porcentaje libre de un disco, redondeado hacia abajo (un 9,8 % no se
/// lee como un 10 %).
fn libre_pct(d: &FichaDisco) -> i64 {
    if d.total <= 0 {
        return 0;
    }
    let pct = d.libre.max(0) as f64 * 100.0 / d.total as f64;
    pct.floor() as i64
}

/// «Tierra seca: queda poco sitio en Películas (4 % libre) y en Series
/// (7 % libre).», con los discos bajos; `None` si no hay ninguno.
pub fn tierra_seca(discos: &[FichaDisco]) -> Option<String> {
    let bajos: Vec<String> = discos
        .iter()
        .filter(|d| d.bajo)
        .map(|d| format!("{} ({} % libre)", d.categoria, libre_pct(d)))
        .collect();
    let (ultimo, resto) = bajos.split_last()?;
    let lista = if resto.is_empty() {
        ultimo.clone()
    } else {
        format!("{} y en {ultimo}", resto.join(", en "))
    };
    Some(format!("Tierra seca: queda poco sitio en {lista}."))
}

/// El tiempo del jardín: el cielo dibujado y el letrero que lo dice.
#[component]
pub fn Tiempo(franja: Franja) -> impl IntoView {
    let cielo = Cielo::de(&franja);
    let causa = cielo
        .causa()
        .map(|c| view! { <p class="tiempo-causa">{c}</p> });
    let qbt = (!franja.qbt).then(|| {
        view! {
            <p class="tiempo-causa">"qBittorrent no responde: no se pueden ver las macetas ahora."</p>
        }
    });
    let sin_vigilar = (!franja.vigilada).then(|| {
        view! {
            <p class="tiempo-causa">
                "La VPN y el puerto no se vigilan (no hay fichero de estado configurado)."
            </p>
        }
    });
    let seca = tierra_seca(&franja.discos).map(|t| view! { <p class="tiempo-seco">{t}</p> });
    view! {
        <div class=cielo.clase()>
            <DibujoCielo cielo />
            <div class="tiempo-letrero" role="status">
                <p class="tiempo-frase">{cielo.frase()}</p>
                {causa}
                {sin_vigilar}
                {qbt}
                {seca}
            </div>
        </div>
    }
}

/// Los rayos del sol: ocho trazos cortos alrededor de (30, 26).
fn rayos() -> String {
    let mut d = String::new();
    for k in 0..8 {
        let a = f64::from(k) * std::f64::consts::FRAC_PI_4;
        let (s, c) = a.sin_cos();
        d.push_str(&format!(
            "M{:.1} {:.1} L{:.1} {:.1} ",
            30.0 + 15.5 * c,
            26.0 + 15.5 * s,
            30.0 + 20.5 * c,
            26.0 + 20.5 * s
        ));
    }
    d.trim_end().to_owned()
}

/// Una nube de algodón, hecha a mano.
const NUBE: &str = "M34 52 C25 52 23 41 31 38 C31 29 43 25 49 31 C53 22 68 22 71 33 \
                    C80 32 84 43 77 48 C76 51 73 52 70 52 Z";

/// El cielo: el sol (o la luna, de noche) y, si hace falta, nubes, lluvia
/// y un rayo. Siempre el mismo dibujo para el mismo tiempo.
#[component]
fn DibujoCielo(cielo: Cielo) -> impl IntoView {
    let astro = (cielo != Cielo::Tormenta).then(|| {
        view! {
            <g class="astro-sol">
                <circle cx="30" cy="26" r="11" />
                <path class="astro-rayos" d=rayos() />
            </g>
            <path class="astro-luna" d="M33 13 A13 13 0 1 0 43 33 A10.5 10.5 0 1 1 33 13 Z" />
        }
    });
    let nube = match cielo {
        Cielo::Bueno => None,
        Cielo::Nublado => Some(view! { <path class="nube" d=NUBE /> }.into_any()),
        Cielo::Tormenta => Some(
            view! {
                <path class="nube nube-detras" d="M10 36 C6 28 14 21 22 25 C26 16 40 17 41 27 C44 27 46 31 44 34 Z" />
                <path class="lluvia" d="M30 56 L27 62 M42 56 L39 62 M62 56 L59 62 M74 55 L71 61" />
                <path class="rayo" d="M52 49 L46 59 L52 59 L48 66 L58 55 L52 55 L56 49 Z" />
                <path class="nube nube-tormenta" d=NUBE />
            }
            .into_any(),
        ),
    };
    view! {
        <svg class="cielo-dibujo" viewBox="0 0 96 68" aria-hidden="true" focusable="false">
            {astro}
            {nube}
        </svg>
    }
}
