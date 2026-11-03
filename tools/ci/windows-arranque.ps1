# Sigue docs/instalar/windows.md como lo haría alguien en Windows, sin
# qBittorrent: deja una carpeta como la del zip de la versión, responde a
# «semillero configurar» por la entrada estándar (modo lista blanca), mira lo
# que escribe, arranca semillero con --env y comprueba que /salud y la
# portada contestan en 127.0.0.1; luego lo para. En Windows lo arranca
# también con semillero.cmd, como dice la guía. Lo usa la CI
# (.github/workflows/ci.yml, trabajo windows); en Linux sirve con pwsh para
# probar el guion, sin la parte de semillero.cmd.
#
#   pwsh tools/ci/windows-arranque.ps1 -Binario BINARIO -Web SITE [-Puerto N]
#
# BINARIO  semillero.exe (en Linux, el semillero de Linux).
# SITE     la web (target/site de cargo-leptos).
# Sale con 0 si todo va bien; si no, dice qué falla, con la salida de
# semillero, y sale con 1.

param(
	[Parameter(Mandatory)] [string] $Binario,
	[Parameter(Mandatory)] [string] $Web,
	[int] $Puerto = 8090
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
# semillero escribe UTF-8; sin esto, en Windows se leería con la página de
# códigos de la consola.
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$OutputEncoding = [Text.UTF8Encoding]::new($false)

$raiz = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
# Nadie escucha en el puerto 9 (discard): qBittorrent «no responde».
$qbt = 'http://127.0.0.1:9'
$base = "http://127.0.0.1:$Puerto"
$procesos = [Collections.Generic.List[Diagnostics.Process]]::new()
$errores = [Collections.Generic.List[string]]::new()

# Dice qué falla, con la salida de semillero, una sola vez.
$informado = $false
function Informar([string] $mensaje) {
	if ($script:informado) { return }
	$script:informado = $true
	if ($env:GITHUB_ACTIONS) { Write-Host "::error::windows-arranque: $mensaje" }
	foreach ($f in $errores) {
		if (Test-Path $f) {
			Write-Host "---- $f"
			Get-Content -Raw $f | Write-Host
		}
	}
}

function Fallar([string] $mensaje) {
	Informar $mensaje
	throw "windows-arranque: $mensaje"
}

function Paso([string] $texto) { Write-Host "== $texto" }

# Lanza semillero (o semillero.cmd) en segundo plano, con su salida en
# ficheros de la carpeta de trabajo.
function Lanzar([string] $nombre, [string] $programa, [string] $argumentos) {
	$out = Join-Path $trabajo "$nombre.out"
	$err = Join-Path $trabajo "$nombre.err"
	$errores.Add($out)
	$errores.Add($err)
	$op = @{
		FilePath               = $programa
		WorkingDirectory       = $carpeta
		RedirectStandardOutput = $out
		RedirectStandardError  = $err
		PassThru               = $true
	}
	if ($argumentos) { $op.ArgumentList = $argumentos }
	if ($IsWindows) { $op.NoNewWindow = $true }
	$p = Start-Process @op
	$procesos.Add($p)
	return $p
}

# Espera hasta 60 s a que /salud conteste; falla si el proceso acaba antes.
function Esperar_salud([Diagnostics.Process] $p) {
	$hasta = (Get-Date).AddSeconds(60)
	while ((Get-Date) -lt $hasta) {
		if ($p.HasExited) { Fallar "semillero ha terminado (código $($p.ExitCode)) sin contestar en $base" }
		try {
			return Invoke-RestMethod "$base/salud" -TimeoutSec 5
		} catch {
			Start-Sleep -Milliseconds 500
		}
	}
	Fallar "$base/salud no contesta en 60 s"
}

# Espera hasta 30 s a que el fichero `fichero` diga `que`.
function Esperar_texto([Diagnostics.Process] $p, [string] $fichero, [string] $que) {
	$hasta = (Get-Date).AddSeconds(30)
	while ((Get-Date) -lt $hasta) {
		if ((Get-Content -Raw $fichero) -like "*$que*") { return }
		if ($p.HasExited) { break }
		Start-Sleep -Milliseconds 500
	}
	Fallar "la salida de semillero no dice «$que»"
}

# Para el proceso (y lo que haya lanzado: semillero.cmd lanza semillero.exe).
function Parar([Diagnostics.Process] $p) {
	if ($p.HasExited) { return }
	if ($IsWindows) {
		& taskkill.exe /PID $p.Id /T /F | Out-Null
	} else {
		Stop-Process -Id $p.Id -Force
	}
	$p.WaitForExit(30000) | Out-Null
}

# Ejecuta «semillero configurar» en la carpeta, como la guía (sin --env:
# semillero.env en la carpeta actual), con una respuesta por línea.
function Configurar([string[]] $respuestas) {
	Push-Location $carpeta
	try {
		$salida = $respuestas | & $exe configurar 2>&1 | Out-String
		return @{ Codigo = $LASTEXITCODE; Salida = $salida }
	} finally {
		Pop-Location
	}
}

function Debe_decir([string] $texto, [string] $que, [string] $donde) {
	if (-not $texto.Contains($que)) {
		Write-Host $texto
		Fallar "$donde no dice «$que»"
	}
}

$trabajo = Join-Path ([IO.Path]::GetTempPath()) ("semillero-arranque-" + [IO.Path]::GetRandomFileName())
$carpeta = Join-Path $trabajo 'semillero'
try {
	# ---------------------------------------------------------- 2. descomprimir
	# Lo que trae el zip (packaging/empaquetar.sh).
	Paso "carpeta como la del zip en $carpeta"
	if (-not (Test-Path -PathType Leaf $Binario)) { Fallar "no existe el binario $Binario" }
	if (-not (Test-Path -PathType Container $Web)) { Fallar "no existe la web $Web" }
	New-Item -ItemType Directory $carpeta | Out-Null
	$exe = Join-Path $carpeta ($IsWindows ? 'semillero.exe' : 'semillero')
	Copy-Item $Binario $exe
	Copy-Item -Recurse $Web (Join-Path $carpeta 'site')
	$win = Join-Path $raiz 'packaging/windows'
	Copy-Item (Join-Path $win 'semillero.cmd'), (Join-Path $win 'semillero.env.example') $carpeta
	Copy-Item (Join-Path $win 'LEEME.md') (Join-Path $carpeta 'LEEME-windows.md')
	if (-not (Test-Path (Join-Path $carpeta 'site/hash.txt'))) { Fallar "la web no trae hash.txt (cargo leptos build --release)" }

	$v = (& $exe --version | Out-String).Trim()
	$cargo = Get-Content (Join-Path $raiz 'crates/semillero/Cargo.toml') |
		Where-Object { $_ -match '^version = "(.+)"' } | Select-Object -First 1
	$version = [regex]::Match($cargo, '"(.+)"').Groups[1].Value
	if ($v -ne "semillero $version") { Fallar "semillero --version dice «$v», no «semillero $version»" }

	# ---------------------------------------------------------- 3. configurar
	# Orden de las preguntas (crates/semillero/src/configurar.rs): dónde
	# escucha, dirección en el navegador, qBittorrent, modo (2: lista
	# blanca), proxy, Jellyfin, fichero de estado, base de datos y, como
	# qBittorrent no responde, si se guarda de todos modos.
	$preguntas = @("127.0.0.1:$Puerto", '', $qbt, '2', '', '', '', '')
	$envf = Join-Path $carpeta 'semillero.env'

	Paso 'semillero configurar sin qBittorrent y sin guardar'
	$r = Configurar ($preguntas + 'N')
	Debe_decir $r.Salida "qBittorrent no responde en $qbt" 'semillero configurar'
	Debe_decir $r.Salida 'No se ha guardado nada' 'semillero configurar'
	if ($r.Codigo -ne 1) { Write-Host $r.Salida; Fallar "semillero configurar sin guardar sale con $($r.Codigo), no con 1" }
	if (Test-Path $envf) { Fallar 'semillero configurar ha escrito semillero.env sin guardar' }

	Paso 'semillero configurar sin qBittorrent, guardando de todos modos'
	$r = Configurar ($preguntas + 's')
	if ($r.Codigo -ne 0) { Write-Host $r.Salida; Fallar "semillero configurar sale con $($r.Codigo)" }
	Debe_decir $r.Salida "qBittorrent no responde en $qbt" 'semillero configurar'
	Debe_decir $r.Salida "guardada en $envf" 'semillero configurar'
	Write-Host $r.Salida

	Paso 'lo que ha escrito'
	if (-not (Test-Path -PathType Leaf $envf)) { Fallar "no está $envf" }
	$lineas = Get-Content $envf
	foreach ($l in @("SEMILLERO_LISTEN=127.0.0.1:$Puerto", "SEMILLERO_QBT=$qbt")) {
		if ($lineas -notcontains $l) { Get-Content -Raw $envf | Write-Host; Fallar "semillero.env no tiene $l" }
	}
	# Intro deja el valor por defecto, que no se escribe; en lista blanca, ni
	# usuario ni contraseña.
	foreach ($k in 'SEMILLERO_ORIGEN', 'SEMILLERO_QBT_USUARIO', 'SEMILLERO_QBT_CLAVE', 'SEMILLERO_DB', 'SEMILLERO_JELLYFIN') {
		if ($lineas | Where-Object { $_.StartsWith("$k=") }) { Get-Content -Raw $envf | Write-Host; Fallar "semillero.env tiene $k" }
	}
	foreach ($f in 'qbt.clave', 'jellyfin.clave') {
		if (Test-Path (Join-Path $carpeta $f)) { Fallar "ha escrito $f en modo lista blanca" }
	}

	# ---------------------------------------------------------- 4. arrancar
	$arranques = @(@{ Nombre = 'semillero-env'; Programa = $exe; Argumentos = "--env `"$envf`""; Db = 'semillero.db' })
	if ($IsWindows) {
		# Doble clic en semillero.cmd: lee semillero.env y la base va a datos\.
		$cmd = Join-Path $carpeta 'semillero.cmd'
		$arranques += @{ Nombre = 'semillero-cmd'; Programa = $env:ComSpec; Argumentos = "/d /c `"$cmd`""; Db = 'datos\semillero.db' }
	}
	foreach ($a in $arranques) {
		Paso "arranque: $($a.Programa) $($a.Argumentos)"
		$p = Lanzar $a.Nombre $a.Programa $a.Argumentos
		$salud = Esperar_salud $p
		# Desde este equipo, con la versión (la que lee el actualizador).
		$j = $salud | ConvertTo-Json -Compress
		if ($j -ne "{`"ok`":true,`"version`":`"$version`"}") { Fallar "/salud contesta $j, no ok con la versión $version" }
		# Sin sesión, la portada lleva a /login.
		$portada = Invoke-WebRequest "$base/" -TimeoutSec 10 -SkipHttpErrorCheck
		$final = $portada.BaseResponse.RequestMessage.RequestUri.AbsolutePath
		if ($portada.StatusCode -ne 200 -or $final -ne '/login') { Fallar "la portada acaba en $final con $($portada.StatusCode), no en /login con 200" }
		Debe_decir $portada.Content 'name="contrasena"' 'la página de entrar'
		# La web: el CSS y el JS con su hash, que salen de site\.
		foreach ($ext in 'css', 'js') {
			$m = [regex]::Match($portada.Content, "[`"'](/pkg/[^`"']+\.$ext)[`"']")
			if (-not $m.Success) { Write-Host $portada.Content; Fallar "la página de entrar no enlaza el .$ext de /pkg" }
			$f = Invoke-WebRequest ($base + $m.Groups[1].Value) -TimeoutSec 10 -SkipHttpErrorCheck
			if ($f.StatusCode -ne 200 -or $f.Content.Length -eq 0) { Fallar "$($m.Groups[1].Value) contesta $($f.StatusCode)" }
		}
		# El canario de acceso pregunta a qBittorrent al arrancar, en segundo
		# plano: hay que esperar a que diga que no ha podido.
		$err = Join-Path $trabajo "$($a.Nombre).err"
		Esperar_texto $p $err 'canario de acceso sin comprobar'
		Parar $p
		Debe_decir (Get-Content -Raw $err) "semillero escuchando en $base" 'la salida de semillero'
		if (-not (Test-Path -PathType Leaf (Join-Path $carpeta $a.Db))) { Fallar "no está la base de datos $($a.Db)" }
		Write-Host "bien: /salud, la portada y la web contestan en $base"
	}
	Paso 'todo bien'
} catch {
	# Lo que no viene de Fallar (una orden que falla por su cuenta).
	Informar "$_"
	throw
} finally {
	foreach ($p in $procesos) { try { Parar $p } catch { Write-Host "no se pudo parar $($p.Id): $_" } }
}
# Sin esto, el código de salida sería el de la última orden (taskkill).
exit 0
