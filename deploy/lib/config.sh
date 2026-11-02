# shellcheck shell=sh
#
# Configuración del equipo de trabajo para deploy/cross/lanzar.sh
# (deploy/config.example, que se copia a deploy/config.local; git ignora
# esta). Se carga con «.» después de poner DEPLOY_DIR (el directorio deploy/).
#
# Los mensajes de error nombran la clave, nunca su valor: la configuración
# lleva equipos y usuarios de la red local.

CLAVES_CONFIG="PC_COMPILAR_SSH PRECOMPILADAS_BUZON_SSH REPO_URL"

# Carga la configuración $1 (por defecto $DEPLOY_DIR/config.local) y exporta
# sus claves. Antes borra las del entorno, para que una clave que falta en el
# fichero no se tome de fuera. El fichero puede empezar con
#   . "$CONFIG_EJEMPLO"
# para tomar los valores del ejemplo y cambiar solo los propios. Qué claves
# hacen falta lo decide quien la usa.
cargar_config()
{
	_config=${1:-$DEPLOY_DIR/config.local}
	if [ ! -r "$_config" ]; then
		echo "ERROR: no se puede leer la configuración ($(basename "$_config")); copia deploy/config.example a deploy/config.local y ajústala" >&2
		return 1
	fi
	for _k in $CLAVES_CONFIG; do
		unset "$_k"
	done
	# shellcheck disable=SC2034 # lo usan las configuraciones que se cargan después
	CONFIG_EJEMPLO=$DEPLOY_DIR/config.example
	# shellcheck source=/dev/null # la configuración de quien instala
	case $_config in
	*/*) . "$_config" ;;
	*) . "./$_config" ;;
	esac
	for _k in $CLAVES_CONFIG; do
		eval "$_k=\${$_k-}"
		# shellcheck disable=SC2163 # se exporta la variable que nombra $_k
		export "$_k"
	done
}
