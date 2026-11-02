@echo off
rem Arranca semillero desde su carpeta. Lee semillero.env (si existe) de
rem la misma carpeta: una variable por linea, VARIABLE=valor; las lineas
rem que empiezan por # no cuentan. La base de datos va a datos\.
setlocal
cd /d "%~dp0"
if exist "%~dp0semillero.env" (
	for /f "usebackq eol=# tokens=1,* delims==" %%A in ("%~dp0semillero.env") do set "%%A=%%B"
)
if not defined SEMILLERO_DB (
	if not exist "%~dp0datos" mkdir "%~dp0datos"
	set "SEMILLERO_DB=%~dp0datos\semillero.db"
)
if not defined LEPTOS_SITE_ROOT set "LEPTOS_SITE_ROOT=%~dp0site"
"%~dp0semillero.exe"
endlocal
