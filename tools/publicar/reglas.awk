# Motor de reglas de escanear.sh (awk POSIX; corre con LC_ALL=C).
#
# Variables: indice (líneas «modo<TAB>fichero<TAB>etiqueta»), privado,
# excepciones y permitido (ficheros, pueden ir vacías). Modos: texto (todas
# las reglas), privado (solo la lista privada), rutas (cada línea es un
# nombre de fichero) y binarios (hallazgos ya hechos «etiqueta<TAB>regla<TAB>
# detalle»).
#
# Escribe un hallazgo por línea, «dónde: regla: muestra tapada», y un resumen
# por la salida de errores. Sale con 1 si hay alguno y con 2 si una lista
# está mal escrita.

function fallo(msj) {
	print "escanear: " msj > "/dev/stderr"
	error = 1
	exit 2
}

# Sustituye todas las apariciones literales de a por b en s.
function cambiar(s, a, b,    r, i) {
	r = ""
	while ((i = index(s, a)) > 0) {
		r = r substr(s, 1, i - 1) b
		s = substr(s, i + length(a))
	}
	return r s
}

# Expresión de Python (la de filter-repo) a ERE, en lo que se suele usar.
function a_ere(re) {
	re = cambiar(re, "(?:", "(")
	re = cambiar(re, "*?", "*")
	re = cambiar(re, "+?", "+")
	re = cambiar(re, "??", "?")
	re = cambiar(re, "\\d", "[0-9]")
	re = cambiar(re, "\\D", "[^0-9]")
	re = cambiar(re, "\\s", "[[:space:]]")
	re = cambiar(re, "\\S", "[^[:space:]]")
	re = cambiar(re, "\\w", "[A-Za-z0-9_]")
	re = cambiar(re, "\\W", "[^A-Za-z0-9_]")
	re = cambiar(re, "\\b", "")
	return re
}

function glob_a_ere(g,    r, i, c) {
	r = ""
	for (i = 1; i <= length(g); i++) {
		c = substr(g, i, 1)
		if (c == "*") r = r ".*"
		else if (c == "?") r = r "."
		else if (index(".+()|^${}\\", c)) r = r "\\" c
		else r = r c
	}
	return r
}

function leer_privado(    l, nl, p, i, ultimo) {
	nl = 0
	while ((getline l < privado) > 0) {
		nl++
		sub(/\r$/, "", l)
		if (l == "" || l ~ /^#/) continue
		# Como filter-repo: el reemplazo va tras el último «==>».
		ultimo = 0
		p = l
		while ((i = index(p, "==>")) > 0) { ultimo += i + 2; p = substr(p, i + 3) }
		if (ultimo) l = substr(l, 1, ultimo - 3)
		np++
		pnum[np] = nl
		pmay[np] = 0
		if (l ~ /^regex:/) {
			l = substr(l, 7)
			if (substr(l, 1, 4) == "(?i)") { l = substr(l, 5); pmay[np] = 1 }
			ptipo[np] = "re"; pval[np] = a_ere(l)
			if (pmay[np]) pval[np] = tolower(pval[np])
		} else if (l ~ /^glob:/) {
			ptipo[np] = "re"; pval[np] = glob_a_ere(substr(l, 6))
		} else {
			if (l ~ /^literal:/) l = substr(l, 9)
			if (l == "") { np--; continue }
			ptipo[np] = "lit"; pval[np] = l
		}
		if (ptipo[np] == "re" && pval[np] == "") np--
	}
	close(privado)
}

function leer_excepciones(    l) {
	while ((getline l < excepciones) > 0) {
		sub(/\r$/, "", l)
		if (l == "" || l ~ /^#/) continue
		nexc++
		exc[nexc] = l
	}
	close(excepciones)
}

function ipv4_num(s,    o) {
	split(s, o, ".")
	return ((o[1] * 256 + o[2]) * 256 + o[3]) * 256 + o[4]
}

function leer_permitido(    l, c, nl, k, red, bits) {
	nl = 0
	while ((getline l < permitido) > 0) {
		nl++
		sub(/\r$/, "", l)
		if (l ~ /^[ \t]*(#|$)/) continue
		k = split(l, c, /[ \t]+/)
		if (c[1] == "") { k--; c[1] = c[2]; c[2] = c[3]; c[3] = c[4] }
		if (k < 3)
			fallo(permitido ":" nl ": cada entrada lleva tipo, valor y motivo")
		if (c[1] == "ip") {
			if (index(c[2], ":")) { pip6[tolower(c[2])] = 1; continue }
			red = c[2]; bits = 32
			if (index(red, "/")) { bits = substr(red, index(red, "/") + 1) + 0; red = substr(red, 1, index(red, "/") - 1) }
			if (red !~ /^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$/ || bits < 0 || bits > 32)
				fallo(permitido ":" nl ": dirección no válida")
			nip4++
			ip4red[nip4] = int(ipv4_num(red) / 2 ^ (32 - bits))
			ip4bits[nip4] = bits
		} else if (c[1] == "dominio") pdom[tolower(c[2])] = 1
		else if (c[1] == "correo") pcorreo[tolower(c[2])] = 1
		else if (c[1] == "ruta") { nruta++; pruta[nruta] = c[2] }
		else if (c[1] == "binario") continue
		else fallo(permitido ":" nl ": tipo desconocido «" c[1] "»")
	}
	close(permitido)
}

# Muestra tapada: los dos primeros caracteres y la longitud.
function tapar(v) {
	return substr(v, 1, 2) "…(" length(v) ")"
}

# Una etiqueta sin valores de la lista privada.
function seguro(s,    i, r, t) {
	for (i = 1; i <= np; i++) {
		if (ptipo[i] == "lit") s = cambiar(s, pval[i], "‹privado#" pnum[i] "›")
		else {
			r = ""
			t = pmay[i] ? tolower(s) : s
			while (match(t, pval[i]) && RLENGTH > 0) {
				r = r substr(s, 1, RSTART - 1) "‹privado#" pnum[i] "›"
				s = substr(s, RSTART + RLENGTH); t = substr(t, RSTART + RLENGTH)
			}
			s = r s
		}
	}
	return s
}

function hallazgo(donde, regla, muestra) {
	print seguro(donde) ": " regla ": " muestra
	total++
	cuenta[regla]++
}

function ipv4_regla(s,    o, a, b, x, i) {
	split(s, o, ".")
	for (i = 1; i <= 4; i++) if (o[i] + 0 > 255) return ""
	a = o[1] + 0; b = o[2] + 0
	if (a == 192 && b == 0 && o[3] + 0 == 2) return "ok"
	if (a == 198 && b == 51 && o[3] + 0 == 100) return "ok"
	if (a == 203 && b == 0 && o[3] + 0 == 113) return "ok"
	x = ipv4_num(s)
	for (i = 1; i <= nip4; i++)
		if (int(x / 2 ^ (32 - ip4bits[i])) == ip4red[i]) return "ok"
	if (a == 10 || a == 127 || a == 0 || (a == 172 && b >= 16 && b <= 31) || \
	    (a == 192 && b == 168) || (a == 169 && b == 254) || (a == 100 && b >= 64 && b <= 127))
		return "ipv4-privada"
	return "ipv4-publica"
}

function revisar_ipv4(l, donde,    p, t, i, m, pre, post, sig, r) {
	p = 1
	while (p <= length(l)) {
		t = substr(l, p)
		if (!match(t, /[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+/)) return
		i = p + RSTART - 1; m = substr(l, i, RLENGTH); p = i + RLENGTH
		pre = substr(l, i - 1, 1); post = substr(l, p, 1); sig = substr(l, p + 1, 1)
		if (i > 1 && pre ~ /[A-Za-z0-9_.]/) continue
		if (post ~ /[A-Za-z0-9_]/ || (post == "." && sig ~ /[0-9]/)) continue
		r = ipv4_regla(m)
		if (r != "" && r != "ok") hallazgo(donde, r, tapar(m))
	}
}

function ipv6_valida(m,    g, k, i, doble) {
	if (m !~ /[0-9]/ || index(m, ":::")) return 0
	doble = index(m, "::")
	if (doble && index(substr(m, doble + 1), "::")) return 0
	if (!doble && (substr(m, 1, 1) == ":" || substr(m, length(m), 1) == ":")) return 0
	k = split(m, g, ":")
	for (i = 1; i <= k; i++) if (length(g[i]) > 4) return 0
	if (doble) return (k <= 9)
	return (k == 8)
}

function revisar_ipv6(l, donde,    p, t, i, m, pre, post, sig, g) {
	p = 1
	while (p <= length(l)) {
		t = substr(l, p)
		if (!match(t, /[0-9A-Fa-f:]*:[0-9A-Fa-f:]*:[0-9A-Fa-f:]*/)) return
		i = p + RSTART - 1; m = substr(l, i, RLENGTH); p = i + RLENGTH
		pre = substr(l, i - 1, 1); post = substr(l, p, 1); sig = substr(l, p + 1, 1)
		if (i > 1 && pre ~ /[A-Za-z0-9_.]/) continue
		if (post ~ /[A-Za-z0-9_]/ || (post == "." && sig ~ /[0-9]/)) continue
		if (!ipv6_valida(m)) continue
		m = tolower(m)
		split(m, g, ":")
		if (g[1] == "2001" && (g[2] == "db8" || g[2] == "0db8")) continue
		if (m in pip6) continue
		hallazgo(donde, "ipv6", tapar(m))
	}
}

function revisar_mac(l, donde,    p, t, i, m, pre, post, x) {
	p = 1
	while (p <= length(l)) {
		t = substr(l, p)
		if (!match(t, /[0-9A-Fa-f][0-9A-Fa-f]([:-][0-9A-Fa-f][0-9A-Fa-f])([:-][0-9A-Fa-f][0-9A-Fa-f])([:-][0-9A-Fa-f][0-9A-Fa-f])([:-][0-9A-Fa-f][0-9A-Fa-f])([:-][0-9A-Fa-f][0-9A-Fa-f])/)) return
		i = p + RSTART - 1; m = substr(l, i, RLENGTH); p = i + RLENGTH
		pre = substr(l, i - 1, 1); post = substr(l, p, 1)
		if (i > 1 && pre ~ /[A-Za-z0-9_:-]/) continue
		if (post ~ /[A-Za-z0-9_:-]/) continue
		x = tolower(m); gsub(/-/, ":", x)
		if (x ~ /^00:00:5e:00:53:/ || x == "00:00:00:00:00:00" || x == "ff:ff:ff:ff:ff:ff") continue
		hallazgo(donde, "mac", tapar(m))
	}
}

function dominio_permitido(d,    k) {
	if (d == "localhost" || d ~ /\.localhost$/) return 1
	if (d == "example" || d ~ /\.example$/) return 1
	if (d == "home.arpa" || d ~ /\.home\.arpa$/) return 1
	for (k in pdom) if (d == k || substr(d, length(d) - length(k)) == "." k) return 1
	return 0
}

function revisar_correo(l, donde,    p, t, i, m, d) {
	p = 1
	while (p <= length(l)) {
		t = substr(l, p)
		if (!match(t, /[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*\.[A-Za-z][A-Za-z]+/)) return
		i = p + RSTART - 1; m = substr(l, i, RLENGTH); p = i + RLENGTH
		m = tolower(m)
		if (m ~ /@users\.noreply\.github\.com$/) continue
		if (m in pcorreo) continue
		d = substr(m, index(m, "@") + 1)
		if (d == "example" || d ~ /\.example$/ || d == "home.arpa" || d ~ /\.home\.arpa$/) continue
		hallazgo(donde, "correo", tapar(m))
	}
}

function revisar_dominio(l, donde,    p, t, i, m, pre, post, n, e) {
	p = 1
	while (p <= length(l)) {
		t = substr(l, p)
		if (!match(t, /[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)+/)) return
		i = p + RSTART - 1; m = substr(l, i, RLENGTH); p = i + RLENGTH
		pre = substr(l, i - 1, 1); post = substr(l, p, 1)
		if (i > 1 && pre ~ /[A-Za-z0-9_@$&[]/) continue
		# Tras «/» solo cuenta en una URL («//»): si no, es una ruta.
		if (pre == "/" && substr(l, i - 2, 1) != "/") continue
		if (post ~ /[A-Za-z0-9_(]/) continue
		m = tolower(m)
		n = split(m, e, ".")
		if (!(e[n] in tld)) continue
		if (dominio_permitido(m)) continue
		hallazgo(donde, "dominio", tapar(m))
	}
}

function revisar_token(l, donde,    b, p, t, i, m, v, j, c, dist, k) {
	b = tolower(l)
	p = 1
	while (p <= length(b)) {
		t = substr(b, p)
		if (!match(t, "(token|clave|password|passwd|pwd|contrase(n|ñ)a|secret|secreto|api[_-]?key|apikey|bearer|basic)[a-z0-9_]*[\"']?[ \t]*(:=|=|:|[ \t])[ \t]*[\"']?[a-z0-9+/=-][a-z0-9+/=-]*")) return
		i = p + RSTART - 1; p = i + RLENGTH
		m = substr(l, i, RLENGTH)
		match(m, "[A-Za-z0-9+/=-]+$")
		v = substr(m, RSTART, RLENGTH)
		if (length(v) < 20 || substr(l, p, 1) ~ /[(_.]/) continue
		if (v !~ /[0-9]/ || v !~ /[A-Za-z]/) continue
		dist = 0
		split("", c)
		for (j = 1; j <= length(v); j++) { k = substr(v, j, 1); if (!(k in c)) { c[k] = 1; dist++ } }
		if (dist < 8) continue
		hallazgo(donde, "token", tapar(v))
	}
}

function revisar_ruta_personal(l, donde,    p, t, i, m, j, ok) {
	p = 1
	while (p <= length(l)) {
		t = substr(l, p)
		if (!match(t, /\/(home|Users)\/[A-Za-z0-9._-]+\//)) return
		i = p + RSTART - 1; m = substr(l, i, RLENGTH); p = i + RLENGTH
		ok = 0
		for (j = 1; j <= nruta; j++) if (m == pruta[j]) ok = 1
		if (!ok) hallazgo(donde, "ruta-personal", tapar(m))
	}
}

function revisar_genericas(l, donde) {
	if (l ~ /[0-9]\.[0-9]/) revisar_ipv4(l, donde)
	if (index(l, ":")) { revisar_ipv6(l, donde); revisar_mac(l, donde) }
	else if (index(l, "-")) revisar_mac(l, donde)
	if (index(l, "@")) revisar_correo(l, donde)
	if (index(l, ".")) revisar_dominio(l, donde)
	if (l ~ /-----BEGIN [A-Z0-9 ]*PRIVATE KEY/) hallazgo(donde, "clave-privada", "-----BEGIN…")
	if (l ~ /(ssh-(rsa|ed25519|dss)|ecdsa-sha2-nistp[0-9]+|sk-ssh-ed25519@openssh\.com) AAAA/) hallazgo(donde, "clave-ssh", "ssh-…")
	revisar_token(l, donde)
	if (index(l, "/home/") || index(l, "/Users/")) revisar_ruta_personal(l, donde)
}

function revisar_privado(l, donde,    i, t) {
	for (i = 1; i <= nexc; i++) l = cambiar(l, exc[i], " ")
	for (i = 1; i <= np; i++) {
		if (ptipo[i] == "lit") { if (index(l, pval[i])) hallazgo(donde, "privado", "entrada " pnum[i]) }
		else {
			t = pmay[i] ? tolower(l) : l
			if (match(t, pval[i])) hallazgo(donde, "privado", "entrada " pnum[i])
		}
	}
}

function revisar_unidad(modo, f, et,    l, nl, c) {
	nl = 0
	while ((getline l < f) > 0) {
		nl++
		sub(/\r$/, "", l)
		if (modo == "binarios") {
			split(l, c, "\t")
			hallazgo(c[1], c[2], c[3])
		} else if (modo == "rutas") {
			revisar_genericas(l, et " «" l "»")
			revisar_privado(l, et " «" l "»")
		} else {
			if (modo == "texto") revisar_genericas(l, et ":" nl)
			revisar_privado(l, et ":" nl)
		}
	}
	close(f)
}

BEGIN {
	FS = "\t"
	split("com net org io dev es eu de fr uk us co me info biz xyz cloud tech ai ch nl be pt it at se no fi dk pl cz ru cn jp br mx ar cl tv cc gg lan localdomain internal intranet home corp arpa onion", x, " ")
	for (k in x) tld[x[k]] = 1
	np = 0; nexc = 0; nip4 = 0; nruta = 0; total = 0
	if (privado != "") leer_privado()
	if (excepciones != "") leer_excepciones()
	if (permitido != "") leer_permitido()
	while ((getline linea < indice) > 0) {
		split(linea, campo, "\t")
		revisar_unidad(campo[1], campo[2], campo[3])
	}
	close(indice)
	if (total == 0) {
		print "escanear: cero hallazgos" > "/dev/stderr"
		exit 0
	}
	r = ""
	for (k in cuenta) r = r " " k "=" cuenta[k]
	print "escanear: " total " hallazgos:" r > "/dev/stderr"
	exit 1
}
