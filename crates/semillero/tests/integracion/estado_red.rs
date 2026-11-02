//! Lectura del fichero de estado de la VPN y del puerto (los mismos
//! criterios que la versión anterior).

use semillero::estado_red::{EstadoRed, leer};

const AHORA: f64 = 1_000_000.0;

fn con(texto: &str) -> EstadoRed {
    let dir = tempfile::tempdir().unwrap();
    let ruta = dir.path().join("estado");
    std::fs::write(&ruta, texto).unwrap();
    leer(Some(&ruta), AHORA)
}

#[test]
fn todo_bien() {
    let e = con("escrito_en=999990\nhandshake=999800\npuerto=51413\nfallos=2\n");
    assert_eq!(
        e,
        EstadoRed {
            vigilada: true,
            vpn: true,
            puerto: Some(51413)
        }
    );
    assert!(e.red_ok());
}

#[test]
fn fichero_viejo_es_todo_caido() {
    // Escrito hace 121 s: quien lo escribe no corre.
    let e = con("escrito_en=999879\nhandshake=999870\npuerto=51413\nfallos=0\n");
    assert_eq!(e, EstadoRed::default());
    assert!(!e.red_ok());
    // A los 120 s justos todavía vale.
    assert!(con("escrito_en=999880\nhandshake=999870\npuerto=51413\n").red_ok());
}

#[test]
fn saludo_viejo_es_vpn_caida() {
    let e = con("escrito_en=999990\nhandshake=999699\npuerto=51413\nfallos=0\n");
    assert!(!e.vpn);
    assert_eq!(e.puerto, Some(51413), "el puerto no depende de la VPN");
    assert!(!e.red_ok());
    assert!(con("escrito_en=999990\nhandshake=999700\npuerto=51413\n").vpn);
}

#[test]
fn tres_fallos_es_puerto_cerrado() {
    let e = con("escrito_en=999990\nhandshake=999990\npuerto=51413\nfallos=3\n");
    assert!(e.vpn);
    assert_eq!(e.puerto, None);
    assert!(!e.red_ok());
}

#[test]
fn sin_puerto_o_sin_fichero_o_raro() {
    assert!(!con("escrito_en=999990\nhandshake=999990\npuerto=\n").red_ok());
    assert_eq!(
        con("escrito_en=ayer\nhandshake=999990\npuerto=1\n"),
        EstadoRed::default()
    );
    assert_eq!(
        leer(Some(std::path::Path::new("/no/existe/estado")), AHORA),
        EstadoRed::default()
    );
}

#[test]
fn sin_fichero_configurado_no_se_vigila() {
    let e = leer(None, AHORA);
    assert_eq!(e, EstadoRed::SIN_VIGILAR);
    assert!(!e.vigilada);
    // Las cuentas atrás avanzan: nadie ha pedido vigilar la red.
    assert!(e.red_ok());
}

#[test]
fn fichero_demasiado_grande_es_todo_caido() {
    let bueno = "escrito_en=999990\nhandshake=999800\npuerto=51413\nfallos=0\n";
    let relleno = "#".repeat(semillero::estado_red::MAX_BYTES as usize);
    let e = con(&format!("{bueno}{relleno}\n"));
    assert_eq!(e, EstadoRed::default());
    assert!(!e.red_ok());
}
