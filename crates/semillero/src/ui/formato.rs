//! Cifras y tiempos en palabras, iguales en el servidor y en el navegador
//! (la hidratación necesita el mismo texto): todo sale de los datos, nunca
//! del reloj del navegador.

const MIN: i64 = 60;
const HORA: i64 = 3600;
const DIA: i64 = 86_400;

/// Un número con coma decimal y `decimales` cifras.
pub fn decimal(v: f64, decimales: usize) -> String {
    format!("{v:.decimales$}").replace('.', ",")
}

/// Un número con coma decimal, con al menos un decimal y sin ceros de más:
/// «1,0», «2,5», «1,25».
pub fn cifra(v: f64) -> String {
    let mut s = format!("{v}");
    if v.is_finite() && !s.contains('.') {
        s.push_str(".0");
    }
    s.replace('.', ",")
}

/// Una duración: «3 días», «1 día y 5 h», «7 h», «12 min».
pub fn duracion(s: i64) -> String {
    let s = s.max(0);
    let (d, h, m) = (s / DIA, (s % DIA) / HORA, (s % HORA) / MIN);
    match (d, h) {
        (2.., _) => format!("{d} días"),
        (1, 0) => "1 día".into(),
        (1, h) => format!("1 día y {h} h"),
        (0, 1..) => format!("{h} h"),
        _ if m >= 1 => format!("{m} min"),
        _ => "menos de 1 min".into(),
    }
}

/// Lo que falta: «falta 1 día», «faltan 3 días», «faltan 5 h».
pub fn falta(s: i64) -> String {
    let d = duracion(s);
    let verbo = if d.starts_with("1 ") || d.starts_with("menos") {
        "falta"
    } else {
        "faltan"
    };
    format!("{verbo} {d}")
}

/// Hace cuánto: «hace 5 min», «hace 3 h», «hace 2 días».
pub fn hace(ahora: i64, ts: i64) -> String {
    let s = ahora - ts;
    if s < MIN {
        "hace un momento".into()
    } else {
        format!("hace {}", duracion(s))
    }
}

/// Cuánto queda para que venza un aviso.
pub fn cuenta_atras(ahora: i64, vence_en: i64) -> String {
    let s = vence_en - ahora;
    if s <= 0 {
        "en la próxima pasada".into()
    } else {
        format!("dentro de {}", duracion(s))
    }
}

/// Bytes en unidades legibles: «850 MB», «1,4 TB».
pub fn bytes(n: i64) -> String {
    const UNIDADES: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n.max(0) as f64;
    let mut u = 0;
    while v >= 1000.0 && u < UNIDADES.len() - 1 {
        v /= 1000.0;
        u += 1;
    }
    if u == 0 || v >= 100.0 {
        format!("{v:.0} {}", UNIDADES[u])
    } else {
        format!("{} {}", decimal(v, 1), UNIDADES[u])
    }
}

/// Fecha y hora UTC en ISO 8601, para `<time datetime>`.
pub fn iso(ts: i64) -> String {
    // Días desde 1970 a fecha civil (algoritmo de Howard Hinnant).
    let dias = ts.div_euclid(DIA);
    let resto = ts.rem_euclid(DIA);
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dia = doy - (153 * mp + 2) / 5 + 1;
    let mes = if mp < 10 { mp + 3 } else { mp - 9 };
    let ano = yoe + era * 400 + i64::from(mes <= 2);
    format!(
        "{ano:04}-{mes:02}-{dia:02}T{:02}:{:02}:{:02}Z",
        resto / HORA,
        (resto % HORA) / MIN,
        resto % MIN
    )
}
