//! qBittorrent simulado con torrents de muestra, para ver semillero en el
//! navegador sin tocar el de verdad. Usuario `admin`, contraseña `secreta`.
//!
//! ```sh
//! cargo run -p qbt-sim --example demo
//! ```
//!
//! Escribe la URL en la salida; pásala a semillero en `SEMILLERO_QBT`.

use qbt_sim::{Conf, arrancar};
use serde_json::{Value, json};

const DIA: i64 = 24 * 3600;
const GB: i64 = 1 << 30;

fn torrent(
    hash: char,
    nombre: &str,
    categoria: &str,
    estado: &str,
    descargado_pct: i64,
    dias: i64,
    ratio: f64,
) -> Value {
    let tamano = 4 * GB;
    json!({
        "hash": hash.to_string().repeat(40), "name": nombre, "category": categoria,
        "state": estado, "downloaded": tamano * descargado_pct / 100, "total_size": tamano,
        "uploaded": (tamano as f64 * ratio) as i64, "seeding_time": dias * DIA,
        "ratio": ratio, "added_on": 1_760_000_000 - dias * DIA,
        "save_path": format!("/media/{categoria}"), "upspeed": 0, "dlspeed": 0,
        "progress": descargado_pct as f64 / 100.0
    })
}

#[tokio::main]
async fn main() {
    let sim = arrancar(Conf {
        api_abierta: true,
        ..Conf::default()
    })
    .await;
    sim.control.fijar_torrents(vec![
        torrent(
            '1',
            "Película recién bajada",
            "peliculas",
            "uploading",
            100,
            2,
            0.4,
        ),
        torrent('2', "Serie T1 completa", "series", "stalledUP", 100, 5, 1.1),
        torrent(
            '3',
            "Documental lento",
            "peliculas",
            "stalledUP",
            100,
            12,
            0.6,
        ),
        torrent(
            '4',
            "Película ya cumplida",
            "peliculas",
            "stalledUP",
            100,
            9,
            1.8,
        ),
        torrent(
            '5',
            "Clásico de hace meses",
            "peliculas",
            "stoppedUP",
            100,
            45,
            0.3,
        ),
        torrent('6', "Serie T2 a medias", "series", "downloading", 5, 0, 0.0),
    ]);
    println!("{}", sim.url);
    std::future::pending::<()>().await;
}
