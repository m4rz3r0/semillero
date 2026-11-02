//! Pruebas de integración de semillero (servidor, `ssr`) contra `qbt-sim`.
//!
//! Un solo binario con un módulo por área: se enlaza una vez.

mod comun;

mod acceso;
mod argumentos;
mod config;
mod configurar;
mod db;
mod estado_red;
mod freno;
mod interfaz;
mod normas;
#[cfg(unix)]
mod parada;
mod presets;
mod seguridad;
mod sondas;
mod sondas_t5;
mod sondas_t5v;
mod sondas_tarea;
mod tarea;
