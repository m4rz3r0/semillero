-- Borrados pedidos a qBittorrent cuya respuesta no llegó clara (un error o un
-- plazo agotado no dicen si borró o no). Mientras el torrent no aparezca en
-- qBittorrent con un aviso pendiente, se mira en cada pasada: si ya no está,
-- se apunta «Borrado» y se avisa a la mediateca.
CREATE TABLE borrados_pedidos (
    hash      TEXT    PRIMARY KEY,
    regla     TEXT    NOT NULL CHECK (regla IN ('ratio', 'siembra', 'inactividad', 'disco')),
    pedido_en INTEGER NOT NULL,
    -- El torrent tal como estaba al pedir el borrado (JSON de `torrents/info`):
    -- la mediateca lo necesita cuando ya no está en qBittorrent.
    torrent   TEXT    NOT NULL
) STRICT;
