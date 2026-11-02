//! Espacio libre y total del sistema de ficheros de una ruta.

use std::path::Path;

use reglas::Disco;

/// Bytes libres (para usuarios sin privilegios) y totales del sistema de
/// ficheros donde está `ruta`; `None` si no se puede consultar.
#[cfg(unix)]
pub fn medir(ruta: &Path) -> Option<Disco> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c = CString::new(ruta.as_os_str().as_bytes()).ok()?;
    let mut s = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `c` es una cadena C válida y `s` tiene el tamaño de `statvfs`;
    // solo se lee `s` si la llamada devuelve 0 (y entonces la ha rellenado).
    let s = unsafe {
        if libc::statvfs(c.as_ptr(), s.as_mut_ptr()) != 0 {
            return None;
        }
        s.assume_init()
    };
    // Los tipos de los campos cambian entre sistemas (Linux, FreeBSD).
    #[allow(clippy::unnecessary_cast, clippy::useless_conversion)]
    let (bloque, libres, total) = (s.f_frsize as u64, s.f_bavail as u64, s.f_blocks as u64);
    let bytes = |n: u64| i64::try_from(n.saturating_mul(bloque)).unwrap_or(i64::MAX);
    Some(Disco {
        libre: bytes(libres),
        total: bytes(total),
    })
}

/// Bytes libres (para este usuario, con sus cuotas) y totales del volumen
/// donde está `ruta`; `None` si no se puede consultar.
#[cfg(windows)]
pub fn medir(ruta: &Path) -> Option<Disco> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let ancha: Vec<u16> = ruta.as_os_str().encode_wide().chain([0]).collect();
    if ancha[..ancha.len() - 1].contains(&0) {
        return None;
    }
    let (mut libres, mut total) = (0u64, 0u64);
    // SAFETY: `ancha` es una cadena UTF-16 terminada en 0 y sin otros ceros;
    // los punteros de salida apuntan a `u64` vivos y el último puede ser nulo.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            ancha.as_ptr(),
            &mut libres,
            &mut total,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return None;
    }
    let bytes = |n: u64| i64::try_from(n).unwrap_or(i64::MAX);
    Some(Disco {
        libre: bytes(libres),
        total: bytes(total),
    })
}

#[cfg(not(any(unix, windows)))]
pub fn medir(_ruta: &Path) -> Option<Disco> {
    None
}
