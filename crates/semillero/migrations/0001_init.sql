-- Esquema inicial de semillero. Las marcas de tiempo son segundos Unix (UTC)
-- salvo donde el nombre dice `_ms`.

-- Sesiones: la clave es el SHA-256 del id que viaja en la cookie; aquí nunca
-- está el id ni ninguna contraseña.
CREATE TABLE sesiones (
    id                  TEXT    PRIMARY KEY,
    usuario             TEXT    NOT NULL,
    creada              INTEGER NOT NULL,
    vista               INTEGER NOT NULL,
    -- Hasta cuándo no hace falta volver a escribir la contraseña para borrar
    -- a mano o activar el borrado real. NULL: hay que escribirla.
    reautenticado_hasta INTEGER
) STRICT;

-- Freno de fuerza bruta: fallos seguidos por IP de cliente y por usuario.
CREATE TABLE freno (
    tipo            TEXT    NOT NULL CHECK (tipo IN ('ip', 'usuario')),
    clave           TEXT    NOT NULL,
    fallos          INTEGER NOT NULL,
    ultimo_fallo_ms INTEGER NOT NULL,
    PRIMARY KEY (tipo, clave)
) STRICT;

-- Ajustes generales (`modo`: 'solo_aviso' o 'borrado_real').
CREATE TABLE ajustes (
    clave TEXT PRIMARY KEY,
    valor TEXT NOT NULL
) STRICT;

-- Reglas de borrado por categoría ('' es «sin categoría»). NULL: desactivada.
CREATE TABLE reglas (
    categoria        TEXT PRIMARY KEY,
    ratio            REAL,
    siembra_dias     INTEGER CHECK (siembra_dias IS NULL OR siembra_dias >= 0),
    inactividad_dias INTEGER CHECK (inactividad_dias IS NULL OR inactividad_dias >= 0),
    disco_libre_pct  REAL
) STRICT;

-- Avisos pendientes: uno por torrent, con la regla que lo eligió.
CREATE TABLE avisos (
    hash      TEXT    PRIMARY KEY,
    regla     TEXT    NOT NULL CHECK (regla IN ('ratio', 'siembra', 'inactividad', 'disco')),
    creado_en INTEGER NOT NULL,
    vence_en  INTEGER NOT NULL
) STRICT;

-- «No borrar»: el torrent queda exento de esa regla.
CREATE TABLE exenciones (
    hash   TEXT    NOT NULL,
    regla  TEXT    NOT NULL CHECK (regla IN ('ratio', 'siembra', 'inactividad', 'disco')),
    creada INTEGER NOT NULL,
    PRIMARY KEY (hash, regla)
) STRICT;

-- Historial de avisos, cancelaciones, borrados y pausas.
CREATE TABLE historial (
    id      INTEGER PRIMARY KEY,
    ts      INTEGER NOT NULL,
    hash    TEXT    NOT NULL,
    nombre  TEXT    NOT NULL,
    evento  TEXT    NOT NULL,
    regla   TEXT    CHECK (regla IS NULL OR regla IN ('ratio', 'siembra', 'inactividad', 'disco')),
    detalle TEXT    NOT NULL DEFAULT ''
) STRICT;
CREATE INDEX historial_ts ON historial (ts);

-- Contador de subida de cada torrent, una muestra por hora: mide la
-- inactividad.
CREATE TABLE muestras_subida (
    hash   TEXT    NOT NULL,
    ts     INTEGER NOT NULL,
    subido INTEGER NOT NULL,
    PRIMARY KEY (hash, ts)
) STRICT;
