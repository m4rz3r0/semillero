//! Las macetas del jardín: un tiesto por categoría (barro para las
//! películas, vidriado para las series, piedra para lo demás) y una planta
//! que crece según el estado del torrent.
//!
//! - Sin arraigar: tierra y una semilla.
//! - Recién sembrados: un brote, más alto cuanto más cerca de los días de
//!   siembra de las normas del tracker.
//! - Creciendo: tallo con hojas y capullos cerrados; crece con
//!   max(ratio / 1, días / 30).
//! - Listos para recoger: la planta entera, en flor.
//!
//! Se dibuja igual en el servidor y en el navegador (la hidratación lo
//! necesita): todo sale de los datos, sin azar del sistema, sin reloj y sin
//! trigonometría; las pequeñas diferencias entre hojas salen del hash del
//! torrent, así dos macetas iguales no parecen clonadas.
//!
//! Sin estilos en línea (CSP `style-src 'self'`): solo atributos de SVG y
//! clases.

use leptos::prelude::*;
use reglas::{Estado, NormasTracker};

use crate::api::{Actividad, FichaTorrent, nombre_categoria};

const DIA: f64 = 86_400.0;

/// Pie de la planta: el centro de la tierra del tiesto.
const PIE_X: f64 = 50.0;
const PIE_Y: f64 = 87.0;

/// Pétalos de una flor (cinco, alrededor del corazón): coseno y seno de
/// 90°, 18°, -54°, -126° y 162°, ya calculados.
const PETALOS: [(f64, f64); 5] = [
    (0.0, -1.0),
    (0.951, -0.309),
    (0.588, 0.809),
    (-0.588, 0.809),
    (-0.951, -0.309),
];

/// El tiesto, según la categoría del torrent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tiesto {
    /// Películas: barro cocido.
    Barro,
    /// Series: cerámica vidriada verde azulada.
    Vidriado,
    /// Sin categoría (y cualquier otra): piedra.
    Piedra,
}

impl Tiesto {
    /// El tiesto de una categoría por su nombre en la interfaz
    /// («Películas», «Series»…).
    pub fn de_nombre(nombre: &str) -> Self {
        if nombre == nombre_categoria("peliculas") {
            Tiesto::Barro
        } else if nombre == nombre_categoria("series") {
            Tiesto::Vidriado
        } else {
            Tiesto::Piedra
        }
    }

    pub fn clase(self) -> &'static str {
        match self {
            Tiesto::Barro => "tiesto-barro",
            Tiesto::Vidriado => "tiesto-vidriado",
            Tiesto::Piedra => "tiesto-piedra",
        }
    }
}

/// Clase de la planta de un estado (la misma palabra que su bancal).
pub fn clase_planta(e: Estado) -> &'static str {
    match e {
        Estado::Cumplido => "planta-recoger",
        Estado::EnEspera => "planta-creciendo",
        Estado::Obligado => "planta-sembrado",
        Estado::Libre => "planta-arraigar",
    }
}

/// Un trazo del dibujo: su clase (el color sale de la hoja de estilos) y
/// su camino de SVG.
#[derive(Clone, Debug, PartialEq)]
pub struct Trazo {
    pub clase: &'static str,
    pub d: String,
}

fn trazo(clase: &'static str, d: String) -> Trazo {
    Trazo { clase, d }
}

/// Un número del dibujo, con un decimal.
fn n(v: f64) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    let s = format!("{v:.1}");
    // Sin «-0.0».
    if s == "-0.0" { "0.0".into() } else { s }
}

/// Semilla del dibujo: FNV-1a del hash.
pub fn semilla(hash: &str) -> u64 {
    hash.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Un valor entre 0 y 1 que sale siempre igual de la semilla y de `i`
/// (mezcla de splitmix64).
fn azar(s: u64, i: u64) -> f64 {
    let mut z = s.wrapping_add(i.wrapping_mul(0x9e37_79b9_7f4a_7c15));
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^= z >> 31;
    (z >> 11) as f64 / (1u64 << 53) as f64
}

/// Cuánto ha crecido la planta en su estado, de 0 a 1, según las normas
/// del tracker.
pub fn crecimiento(t: &FichaTorrent, n: &NormasTracker) -> f64 {
    let dias = t.siembra_s.max(0) as f64 / DIA;
    let ratio = if t.ratio.is_finite() {
        t.ratio.max(0.0)
    } else {
        0.0
    };
    let g = match t.estado {
        Estado::Obligado => parte(dias, f64::from(n.siembra_dias)),
        Estado::EnEspera => {
            parte(ratio, n.ratio_min).max(parte(dias, f64::from(n.plazo_largo_dias)))
        }
        Estado::Cumplido | Estado::Libre => 1.0,
    };
    g.clamp(0.0, 1.0)
}

/// Un punto.
#[derive(Clone, Copy)]
struct P(f64, f64);

/// El tallo: una curva desde el pie hasta `cima`, con `control`.
#[derive(Clone, Copy)]
struct Tallo {
    cima: P,
    control: P,
}

impl Tallo {
    fn nuevo(alto: f64, inclina: f64) -> Self {
        Tallo {
            cima: P(PIE_X + inclina, PIE_Y - alto),
            control: P(PIE_X - inclina * 0.6, PIE_Y - alto * 0.55),
        }
    }

    /// Punto del tallo en `t` (0 el pie, 1 la cima).
    fn en(self, t: f64) -> P {
        let u = 1.0 - t;
        P(
            u * u * PIE_X + 2.0 * u * t * self.control.0 + t * t * self.cima.0,
            u * u * PIE_Y + 2.0 * u * t * self.control.1 + t * t * self.cima.1,
        )
    }

    fn camino(self) -> String {
        format!(
            "M{} {} Q{} {} {} {}",
            n(PIE_X),
            n(PIE_Y),
            n(self.control.0),
            n(self.control.1),
            n(self.cima.0),
            n(self.cima.1)
        )
    }
}

/// Una hoja que sale de `p` hacia `lado` (-1 izquierda, 1 derecha), de
/// largo `largo`; `alza` dice cuánto sube la punta. Con su nervio.
fn hoja(p: P, lado: f64, largo: f64, alza: f64, clase: &'static str) -> [Trazo; 2] {
    let P(x, y) = p;
    let punta = P(x + lado * largo, y - largo * alza);
    let c1 = P(x + lado * largo * 0.3, y - largo * (alza + 0.42));
    let c2 = P(
        x + lado * largo * 0.62,
        y + largo * 0.18 - largo * alza * 0.3,
    );
    [
        trazo(
            clase,
            format!(
                "M{} {} Q{} {} {} {} Q{} {} {} {} Z",
                n(x),
                n(y),
                n(c1.0),
                n(c1.1),
                n(punta.0),
                n(punta.1),
                n(c2.0),
                n(c2.1),
                n(x),
                n(y)
            ),
        ),
        trazo(
            "planta-nervio",
            format!(
                "M{} {} L{} {}",
                n(x),
                n(y),
                n(x + lado * largo * 0.7),
                n(y - largo * alza * 0.75)
            ),
        ),
    ]
}

/// Un círculo como camino.
fn circulo(c: P, r: f64) -> String {
    format!(
        "M{} {} a{} {} 0 1 0 {} 0 a{} {} 0 1 0 -{} 0 Z",
        n(c.0 - r),
        n(c.1),
        n(r),
        n(r),
        n(2.0 * r),
        n(r),
        n(r),
        n(2.0 * r)
    )
}

/// Un capullo cerrado en `p`, apuntando hacia arriba.
fn capullo(p: P, tam: f64) -> Trazo {
    let P(x, y) = p;
    trazo(
        "planta-capullo",
        format!(
            "M{} {} Q{} {} {} {} Q{} {} {} {} Z",
            n(x),
            n(y + 1.0),
            n(x - tam * 0.42),
            n(y - tam * 0.45),
            n(x),
            n(y - tam),
            n(x + tam * 0.42),
            n(y - tam * 0.45),
            n(x),
            n(y + 1.0)
        ),
    )
}

/// Una flor en `p`: cinco pétalos y el corazón. `vuelta` la gira media
/// vuelta de pétalo.
fn flor(p: P, tam: f64, otra: bool, vuelta: bool) -> Vec<Trazo> {
    let clase = if otra {
        "planta-flor-otra"
    } else {
        "planta-flor"
    };
    let giro = if vuelta { -1.0 } else { 1.0 };
    let mut v: Vec<Trazo> = PETALOS
        .iter()
        .map(|(c, s)| {
            trazo(
                clase,
                circulo(
                    P(p.0 + c * tam * 1.1, p.1 + s * tam * 1.1 * giro),
                    tam * 0.85,
                ),
            )
        })
        .collect();
    v.push(trazo("planta-corazon", circulo(p, tam * 0.62)));
    v
}

/// `v` sobre `meta`; con la meta a cero, ya está.
fn parte(v: f64, meta: f64) -> f64 {
    if meta > 0.0 { v / meta } else { 1.0 }
}

/// Los trazos de la planta del torrent (el tiesto va aparte).
pub fn trazos(t: &FichaTorrent, normas: &NormasTracker) -> Vec<Trazo> {
    let s = semilla(&t.hash);
    let g = crecimiento(t, normas);
    let inclina = (azar(s, 1) - 0.5) * 7.0;
    let mut v = Vec::new();
    match t.estado {
        Estado::Libre => {
            // Una semilla sobre la tierra, algo ladeada, y la tierra
            // removida al lado.
            let x = 40.0 + 20.0 * azar(s, 0);
            let y = PIE_Y - 0.5;
            let l = if azar(s, 2) < 0.5 { -1.0 } else { 1.0 };
            v.push(trazo(
                "planta-semilla",
                format!(
                    "M{} {} Q{} {} {} {} Q{} {} {} {} Z",
                    n(x - 5.0 * l),
                    n(y + 1.0),
                    n(x - l),
                    n(y - 5.5),
                    n(x + 5.0 * l),
                    n(y - 1.2),
                    n(x + l),
                    n(y + 3.5),
                    n(x - 5.0 * l),
                    n(y + 1.0)
                ),
            ));
            v.push(trazo(
                "planta-surco",
                format!(
                    "M{} {} q4.5 1.4 {} 0",
                    n(x - 9.0),
                    n(y + 2.6),
                    n(9.0 + 9.0 * azar(s, 3))
                ),
            ));
        }
        Estado::Obligado => {
            let tallo = Tallo::nuevo(10.0 + 26.0 * g, inclina);
            v.push(trazo("planta-tallo", tallo.camino()));
            let largo = 7.0 + 7.0 * g + 3.0 * azar(s, 2);
            v.extend(hoja(
                tallo.cima,
                -1.0,
                largo * (0.85 + 0.3 * azar(s, 3)),
                0.35 + 0.25 * azar(s, 4),
                "planta-hoja",
            ));
            v.extend(hoja(
                tallo.cima,
                1.0,
                largo * (0.85 + 0.3 * azar(s, 5)),
                0.35 + 0.25 * azar(s, 6),
                "planta-hoja-clara",
            ));
        }
        Estado::EnEspera | Estado::Cumplido => {
            let en_flor = t.estado == Estado::Cumplido;
            let alto = if en_flor {
                64.0 + 8.0 * azar(s, 7)
            } else {
                32.0 + 30.0 * g
            };
            let tallo = Tallo::nuevo(alto, inclina);
            // Ramas: salen a dos tercios del tallo, hacia los lados.
            let nace = tallo.en(0.62);
            let cuantas = if en_flor {
                2
            } else {
                usize::from(g > 0.5) + usize::from(g > 0.85)
            };
            let mut ramas = Vec::new();
            for (i, lado) in [-1.0, 1.0].into_iter().enumerate().take(cuantas) {
                let i = i as u64;
                let fin = P(
                    nace.0 + lado * (13.0 + 5.0 * azar(s, 30 + i)),
                    nace.1 - 13.0 - 6.0 * azar(s, 32 + i),
                );
                v.push(trazo(
                    "planta-tallo",
                    format!(
                        "M{} {} Q{} {} {} {}",
                        n(nace.0),
                        n(nace.1),
                        n(nace.0 + lado * 2.0),
                        n(fin.1 + 2.0),
                        n(fin.0),
                        n(fin.1)
                    ),
                ));
                ramas.push(fin);
            }
            v.push(trazo("planta-tallo", tallo.camino()));
            // Hojas alternas a lo largo del tallo.
            let hojas = if en_flor {
                6
            } else {
                2 + (4.0 * g).round() as usize
            };
            let empieza = usize::from(azar(s, 8) >= 0.5);
            for i in 0..hojas {
                let p = tallo.en(0.18 + 0.62 * (i as f64 + 0.5) / hojas as f64);
                let lado = if (i + empieza) % 2 == 0 { -1.0 } else { 1.0 };
                let clase = if i % 3 == 1 {
                    "planta-hoja-clara"
                } else {
                    "planta-hoja"
                };
                let i = i as u64;
                v.extend(hoja(
                    p,
                    lado,
                    10.0 + 6.0 * azar(s, 10 + i),
                    0.2 + 0.3 * azar(s, 20 + i),
                    clase,
                ));
            }
            if en_flor {
                let otra = azar(s, 9) < 0.5;
                v.extend(flor(tallo.cima, 4.6, otra, azar(s, 40) < 0.5));
                for (i, r) in ramas.into_iter().enumerate() {
                    v.extend(flor(r, 3.6, otra == (i == 0), azar(s, 41 + i as u64) < 0.5));
                }
            } else {
                v.push(capullo(tallo.cima, 7.0 + 2.0 * g));
                for r in ramas {
                    v.push(capullo(r, 5.5));
                }
            }
        }
    }
    v
}

/// Los trazos del tiesto, con su tierra.
fn trazos_tiesto(tiesto: Tiesto, s: u64) -> Vec<Trazo> {
    let mut v = vec![
        trazo(
            "tiesto-cuerpo",
            "M18 95 L82 95 L75.5 127 Q50 130.5 24.5 127 Z".into(),
        ),
        trazo(
            "tiesto-borde",
            "M13 85.5 Q50 82.5 87 85.5 L86 96 Q50 98.5 14 96 Z".into(),
        ),
        trazo(
            "tiesto-tierra",
            "M17 87.6 Q50 83 83 87.6 Q50 91.6 17 87.6 Z".into(),
        ),
    ];
    match tiesto {
        Tiesto::Barro => v.push(trazo("tiesto-veta", "M20.5 103 Q50 105.5 79.5 103".into())),
        Tiesto::Vidriado => v.push(trazo("tiesto-brillo", "M27 100 Q24.5 112 29 123".into())),
        Tiesto::Piedra => {
            for i in 0..4 {
                let c = P(
                    27.0 + 46.0 * azar(s, 50 + i),
                    101.0 + 22.0 * azar(s, 60 + i),
                );
                v.push(trazo(
                    "tiesto-mota",
                    circulo(c, 0.9 + 0.6 * azar(s, 70 + i)),
                ));
            }
        }
    }
    v
}

fn pintar(v: Vec<Trazo>) -> impl IntoView {
    v.into_iter()
        .map(|t| view! { <path class=t.clase d=t.d /> })
        .collect_view()
}

/// La maceta de un torrent. `crecer`: la planta crece al aparecer (solo la
/// primera vez que se carga la página).
#[component]
pub fn Maceta(
    t: FichaTorrent,
    normas: NormasTracker,
    #[prop(optional)] crecer: bool,
) -> impl IntoView {
    let tiesto = Tiesto::de_nombre(&t.categoria);
    let clase = format!(
        "maceta-dibujo {} {}",
        tiesto.clase(),
        clase_planta(t.estado)
    );
    let planta = if crecer {
        "planta planta-brota"
    } else {
        "planta"
    };
    view! {
        // El tiesto, algo más pequeño que el dibujo (encogido hacia el pie
        // de la planta): la planta destaca sobre su maceta.
        <svg class=clase viewBox="0 0 100 124" aria-hidden="true" focusable="false">
            <g transform="translate(50 87) scale(0.8) translate(-50 -87)">
                {pintar(trazos_tiesto(tiesto, semilla(&t.hash)))}
            </g>
            <g class=planta>{pintar(trazos(&t, &normas))}</g>
        </svg>
    }
}

/// Un tiesto vacío, con su tierra: el dibujo de una categoría (en Reglas).
#[component]
pub fn TiestoVacio(tiesto: Tiesto) -> impl IntoView {
    let clase = format!("tiesto-icono {}", tiesto.clase());
    view! {
        <svg class=clase viewBox="10 80 80 52" aria-hidden="true" focusable="false">
            {pintar(trazos_tiesto(tiesto, semilla(tiesto.clase())))}
        </svg>
    }
}

/// La planta de un bancal, sin tiesto, para su pestaña: siempre la misma
/// (sale de unos datos fijos, como si fuera un torrent más).
#[component]
pub fn GlifoPlanta(estado: Estado) -> impl IntoView {
    const DIA_S: i64 = 86_400;
    let (siembra_s, ratio) = match estado {
        Estado::Obligado => (5 * DIA_S, 0.2),
        Estado::EnEspera => (12 * DIA_S, 0.55),
        Estado::Cumplido => (9 * DIA_S, 1.4),
        Estado::Libre => (0, 0.0),
    };
    let t = FichaTorrent {
        hash: format!("glifo-{}", clase_planta(estado)),
        nombre: String::new(),
        categoria: String::new(),
        estado,
        actividad: Actividad::Sembrando,
        ratio,
        siembra_s,
        falta_s: 0,
        tamano: 0,
        aviso: None,
        exenciones: Vec::new(),
    };
    // Encuadre de cada planta: la semilla es pequeña y va más cerca.
    let caja = match estado {
        Estado::Libre => "28 62 44 32",
        Estado::Obligado => "22 38 56 56",
        _ => "10 6 80 88",
    };
    let clase = format!("pestana-glifo {}", clase_planta(estado));
    view! {
        <svg class=clase viewBox=caja aria-hidden="true" focusable="false">
            <path class="glifo-tierra" d="M14 89 Q50 84.5 86 89 Q50 93 14 89 Z" />
            <g class="planta">{pintar(trazos(&t, &NormasTracker::default()))}</g>
        </svg>
    }
}
