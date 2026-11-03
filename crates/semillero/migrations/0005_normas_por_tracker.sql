-- Normas propias de cada tracker, por host (el del anuncio, sin esquema,
-- puerto ni ruta; nunca el anuncio entero, que lleva el passkey). Las mismas
-- columnas y comprobaciones que `normas`, que sigue siendo el juego por
-- defecto: una base de datos de la 0.5 empieza sin ninguna fila aquí y todo
-- sigue igual.
CREATE TABLE normas_tracker (
    host             TEXT    PRIMARY KEY COLLATE NOCASE
        CHECK (length(host) BETWEEN 1 AND 253 AND (host COLLATE BINARY) = lower(host)
               AND host NOT GLOB '*[^a-z0-9.:-]*'),
    umbral_pct       REAL    NOT NULL CHECK (umbral_pct BETWEEN 0 AND 100),
    siembra_dias     INTEGER NOT NULL CHECK (siembra_dias BETWEEN 0 AND 365),
    ratio_min        REAL    NOT NULL CHECK (ratio_min BETWEEN 0 AND 100),
    plazo_largo_dias INTEGER NOT NULL
        CHECK (plazo_largo_dias BETWEEN 0 AND 365 AND plazo_largo_dias >= siembra_dias),
    ratio_pausa      REAL    NOT NULL
        CHECK (ratio_pausa BETWEEN 0 AND 100 AND ratio_pausa >= ratio_min),
    aviso_horas      INTEGER NOT NULL CHECK (aviso_horas BETWEEN 1 AND 720)
) STRICT;

-- El último host de tracker visto en cada torrent: si qBittorrent deja de
-- dar tracker (caído), el torrent sigue con las normas de este. `visto` es la
-- última pasada en que el torrent estaba en qBittorrent; los que llevan unos
-- días sin aparecer se olvidan, como sus muestras de subida.
CREATE TABLE trackers_vistos (
    hash  TEXT    PRIMARY KEY,
    host  TEXT    NOT NULL
        CHECK (length(host) BETWEEN 1 AND 253 AND (host COLLATE BINARY) = lower(host)
               AND host NOT GLOB '*[^a-z0-9.:-]*'),
    visto INTEGER NOT NULL
) STRICT;
