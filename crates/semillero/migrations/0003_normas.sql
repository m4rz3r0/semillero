-- Normas del tracker: una sola fila (id = 1). Empieza con los valores por
-- defecto, los mismos que antes estaban fijos en el código, así que una base
-- de datos ya en uso sigue igual.
CREATE TABLE normas (
    id               INTEGER PRIMARY KEY CHECK (id = 1),
    -- Porcentaje descargado a partir del cual un torrent obliga a sembrar.
    umbral_pct       REAL    NOT NULL CHECK (umbral_pct BETWEEN 0 AND 100),
    siembra_dias     INTEGER NOT NULL CHECK (siembra_dias BETWEEN 0 AND 365),
    ratio_min        REAL    NOT NULL CHECK (ratio_min BETWEEN 0 AND 100),
    plazo_largo_dias INTEGER NOT NULL
        CHECK (plazo_largo_dias BETWEEN 0 AND 365 AND plazo_largo_dias >= siembra_dias),
    ratio_pausa      REAL    NOT NULL
        CHECK (ratio_pausa BETWEEN 0 AND 100 AND ratio_pausa >= ratio_min),
    aviso_horas      INTEGER NOT NULL CHECK (aviso_horas BETWEEN 1 AND 720)
) STRICT;

INSERT INTO normas (id, umbral_pct, siembra_dias, ratio_min, plazo_largo_dias, ratio_pausa, aviso_horas)
VALUES (1, 10.0, 7, 1.0, 30, 2.0, 24);
