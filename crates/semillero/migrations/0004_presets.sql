-- Presets guardados: normas del tracker y reglas por categoría, con un
-- nombre y una descripción. `archivo` es el TOML del preset (formato 1),
-- tal y como se descarga; se vuelve a comprobar al leerlo. El preset
-- incluido no está aquí: sale del código y no se puede borrar.
CREATE TABLE presets (
    id          INTEGER PRIMARY KEY,
    nombre      TEXT    NOT NULL COLLATE NOCASE UNIQUE
        CHECK (length(trim(nombre)) BETWEEN 1 AND 60),
    descripcion TEXT    NOT NULL DEFAULT '' CHECK (length(descripcion) <= 300),
    archivo     TEXT    NOT NULL CHECK (length(CAST(archivo AS BLOB)) <= 65536),
    cambiado    INTEGER NOT NULL
) STRICT;
