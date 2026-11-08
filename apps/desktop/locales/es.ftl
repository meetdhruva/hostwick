# Hostwick desktop: español.
#
# Traducción de en.ftl (origen). Las claves y las variables ({ $name }) no
# cambian; los nombres de productos y protocolos (SSH, SFTP, SCP, sudo,
# Kerberos, PuTTY, WinSCP, X11, SOCKS) no se traducen.

## General

error-internal = error interno: { $error }

## Main window

sidebar-search-placeholder = Buscar hosts
split-only-terminals = Solo se pueden dividir las pestañas de terminal.
files-follow-no-folder = El terminal aún no ha indicado su carpeta. Active «Integración con el shell» en Editar host → Terminal para que el explorador de archivos pueda seguirlo (bash y zsh).
host-copy-name = { $name } (copia)
size-b = B
size-kib = KiB
size-mib = MiB
size-gib = GiB
size-tib = TiB
protocol-ssh = SSH
protocol-telnet = Telnet
protocol-serial = Serie
sidebar-vault-unlocked-tooltip = Las contraseñas guardadas están desbloqueadas; haga clic para bloquearlas
sidebar-settings-tooltip = Configuración ({ $keys })
sidebar-open-terminal = Terminal
sidebar-open-files = Archivos
sidebar-edit-host = Editar host
sidebar-delete-host = Eliminar host
sidebar-new-host = Nuevo host…
tab-files-title = Archivos: { $name }
broadcast = Difusión
broadcast-on = Difusión: ACTIVADA
broadcast-tooltip = Enviar las pulsaciones a todos los terminales abiertos ({ $keys })
shortcut-new-host = nuevo host
shortcut-duplicate-tab = duplicar pestaña
shortcut-close-pane = cerrar panel/pestaña
shortcut-reconnect = reconectar
shortcut-next-tab = pestaña siguiente
shortcut-split-right = dividir a la derecha
shortcut-split-down = dividir abajo
shortcut-move-between-panes = moverse entre paneles
shortcut-broadcast = difusión
shortcut-find = buscar
shortcut-settings = configuración
empty-title = No hay sesiones abiertas
empty-hint-connect = Haga doble clic en un host o escriba user@host en el cuadro de conexión rápida.
empty-hint-menus = Haga clic derecho en hosts, pestañas, archivos y terminales para ver más acciones.
empty-hint-import = Importe desde ~/.ssh/config, PuTTY, WinSCP o Solar-PuTTY con Importar… (abajo a la izquierda).
pane-connecting = Conectando con { $host }…
pane-retry = Reintentar
transfer-progress = { $done } de { $total } — { $rate }/s
transfer-done = completado — { $size }
transfer-skipped = omitido
transfer-cancelled = cancelado
transfer-failed = error: { $error }
transfer-cancel = Cancelar
transfers-title = { $count ->
    [one] Transferencias — { $count } activa
   *[other] Transferencias — { $count } activas
}
transfers-clear = Quitar las terminadas

## Common buttons

button-cancel = Cancelar
button-delete = Eliminar
button-ok = Aceptar
button-save = Guardar
button-close = Cerrar
button-browse = Examinar…

## Menus, import

host-delete-title = Eliminar host
host-delete-confirm = ¿Eliminar el host guardado «{ $name }»?
sidebar-import = Importar…
sidebar-tools = Herramientas
import-no-home = no hay carpeta personal
import-pick-putty = Exportación .reg de PuTTY
import-pick-solar = Exportación de Solar-PuTTY (.json)
import-pick-winscp = WinSCP.ini
import-failed = Error al importar: { $error }
import-summary = { $added } añadidos, { $updated } actualizados, { $notes } con notas
import-done = Importado: { $summary }
import-report-title = Informe de importación
import-report-attention = Requieren atención ({ $count })
import-report-added = Añadidos ({ $count })
import-report-updated = Actualizados ({ $count })
import-report-note = Los hosts que aparecen en «Requieren atención» se importaron igualmente, salvo que la nota diga que se omitieron. Haga clic derecho en un host → Editar… para ajustarlo.
import-menu-ssh-config = OpenSSH ~/.ssh/config
import-menu-putty-registry = PuTTY (sesiones guardadas en este equipo)
import-menu-winscp-registry = WinSCP (sitios guardados en este equipo)
import-menu-putty-file = Archivo de exportación .reg de PuTTY…
import-menu-winscp-file = Archivo WinSCP.ini…
import-menu-solar-file = Exportación .json de Solar-PuTTY…
menu-open-terminal = Abrir terminal
menu-open-files = Abrir archivos (SFTP)
menu-edit = Editar…
menu-duplicate = Duplicar
menu-copy-address = Copiar dirección
menu-delete = Eliminar…
menu-reconnect = Reconectar
menu-split-right = Dividir a la derecha
menu-split-down = Dividir abajo
menu-send-break = Enviar break
menu-follow-terminal = Seguir la carpeta del terminal
menu-close = Cerrar
menu-close-others = Cerrar las demás
menu-close-all = Cerrar todas
tools-generate-key = Generar clave…
tools-inspect-key = Examinar o convertir clave…
tools-known-hosts = Hosts conocidos…
tools-saved-passwords = Contraseñas guardadas…
tools-settings-sync = Sincronización de la configuración…
button-continue = Continuar

## Connection prompts

notice-connection-title = Conexión
hostkey-changed-title = ¡La clave del host ha cambiado!
hostkey-unknown-title = Host desconocido
hostkey-changed-body = ADVERTENCIA: la clave del host { $host } ha CAMBIADO.

    Puede significar que alguien está interceptando la conexión (ataque de intermediario) o que se reinstaló el servidor.

    Nueva clave { $algorithm }:
    { $fingerprint }

    Continúe solo si sabe que la clave se cambió a propósito.
hostkey-unknown-body = Es la primera conexión con { $host }.

    Huella de la clave { $algorithm }:
    { $fingerprint }

    Compruébela con la persona que administra el servidor antes de confiar en ella.
hostkey-reject = Rechazar
hostkey-trust-once = Confiar una vez
hostkey-replace = Reemplazar la clave guardada
hostkey-trust-remember = Confiar y recordar
remember-hint-unlocked = Se guarda cifrada en su almacén de contraseñas.
remember-hint-locked = Se le pedirá la contraseña maestra de su almacén.
remember-hint-create = Creará un almacén de contraseñas con una contraseña maestra.
prompt-server-auth-title = Autenticación del servidor
prompt-password-title = Contraseña de { $user }@{ $host }
prompt-password-label = Contraseña
prompt-passphrase-label = Frase de contraseña
prompt-sudo-title = Contraseña de sudo de { $who }
prompt-sudo-wrong-title = Contraseña incorrecta. Contraseña de sudo de { $who }
secret-rejected-password = La contraseña guardada no fue aceptada. Escriba la actual.
secret-rejected-proxy = La contraseña del proxy guardada no fue aceptada. Escriba la actual.
secret-rejected-passphrase = La frase de contraseña guardada no fue aceptada. Escriba la actual.
secret-rejected-sudo = sudo no aceptó la contraseña guardada.
secret-update-password = Actualizar la contraseña guardada
secret-update-proxy = Actualizar la contraseña del proxy guardada
secret-update-passphrase = Actualizar la frase de contraseña guardada
secret-update-sudo = Actualizar la contraseña de sudo guardada
secret-remember-password = Recordar contraseña
secret-remember-proxy = Recordar contraseña del proxy
secret-remember-passphrase = Recordar frase de contraseña
secret-remember-sudo = Recordar contraseña de sudo
vault-reason-host = { $name } tiene contraseñas guardadas. Desbloquéelas con su contraseña maestra.
vault-reason-store = Desbloquee sus contraseñas guardadas para guardar esta.
vault-reason-create = Cree un almacén para las contraseñas guardadas. Una sola contraseña maestra las protege; nunca se guarda.
notice-not-saved-locked = No se guardó: el almacén de contraseñas está bloqueado.
notice-save-password-failed = No se pudo guardar la contraseña: { $error }
notice-update-host-failed = No se pudo actualizar { $name }: { $error }
files-need-ssh = Los archivos necesitan SSH: { $name } es una conexión { $protocol }, que no puede transferir archivos.
files-root-failed = No se pudo abrir como root: { $error }

## Settings sync

sync-no-changes = sin cambios aquí
sync-added = { $count } añadidos
sync-updated = { $count } actualizados
sync-removed = { $count } eliminados
sync-settings-updated = configuración actualizada
sync-mode-sync = Sincronizar
sync-mode-push = Enviar
sync-mode-pull = Recibir
sync-summary = { $mode }: { $count ->
    [one] { $count } host sincronizado
   *[other] { $count } hosts sincronizados
}; { $changes }
sync-file-updated = archivo de sincronización actualizado
vault-reason-sync-startup = Desbloquee sus contraseñas guardadas para sincronizar la configuración (allí se guarda la frase de sincronización).
vault-reason-sync = Desbloquee sus contraseñas guardadas para sincronizar la configuración.
sync-no-folder = Elija primero una carpeta de sincronización en Configuración → Sincronización de la configuración.
sync-no-passphrase = no hay ninguna frase de sincronización guardada; configúrela en Configuración → Sincronización de la configuración
sync-passphrase-title = Frase de contraseña de sincronización
sync-passphrase-label = Frase de sincronización
sync-passphrase-wrong-title = Frase de sincronización incorrecta; escriba la que usan sus otros equipos
sync-wrong-passphrase = frase de sincronización incorrecta
sync-last-failed = error: { $error }
sync-done = Sincronización de la configuración — { $summary }
sync-failed = Error en la sincronización de la configuración: { $error }

## Password vault

cred-sync-passphrase = Frase de sincronización de la configuración
cred-login-password = contraseña de inicio de sesión
cred-sudo-password = contraseña de sudo
cred-proxy-password = contraseña del proxy
cred-key-passphrase = frase de contraseña de { $file }
cred-key-unused = Frase de contraseña de clave — { $path } (ningún host la usa)
cred-host-deleted = Sin uso (se eliminó su host)
strength-too-short = Demasiado corta: use al menos 8 caracteres
strength-strong-passphrase = Frase de contraseña segura
strength-weak = Débil: cuanto más larga, mejor (unas cuantas palabras funcionan bien)
strength-fair = Aceptable
strength-strong = Segura
vault-locked-error = el almacén está bloqueado
vault-current-wrong = La contraseña maestra actual no es correcta.
vault-current-master = Contraseña maestra actual
vault-master = Contraseña maestra
vault-new-master = Nueva contraseña maestra
vault-repeat = Repítala
vault-enter-password = Escriba una contraseña.
vault-too-weak = Use al menos 8 caracteres; unas cuantas palabras sin relación funcionan bien.
vault-mismatch = Las dos contraseñas no coinciden.
vault-wrong-master = Contraseña maestra incorrecta.
vault-strength-empty = Al menos 8 caracteres; unas cuantas palabras sin relación funcionan bien.
vault-never-stored = La contraseña maestra nunca se guarda. Si la olvida, no se podrán recuperar las contraseñas guardadas.
vault-unlocking = Desbloqueando…
vault-encrypting = Cifrando…
vault-path = Almacén: { $path } (compartido con el comando hostwick)
vault-title-unlock = Desbloquear contraseñas guardadas
vault-title-create = Crear un almacén de contraseñas
vault-title-change = Cambiar la contraseña maestra
vault-not-now = Ahora no
vault-unlock = Desbloquear
vault-create = Crear almacén
vault-change = Cambiar

## Host editor

form-forward-needs-spec = `{ $token }` necesita una especificación de puerto
form-jump-invalid = host de salto `{ $host }`: { $error }
form-port-invalid = { $field } debe ser un número del 1 al 65535
form-baud-invalid = La velocidad (baudios) debe ser un número como 9600 o 115200
form-bits-invalid = Los bits de datos deben ser de 5 a 8 y los bits de parada 1 o 2
host-section-general = General
host-section-auth = Autenticación
host-section-connection = Conexión
host-section-terminal = Terminal
host-section-files = Archivos
host-section-advanced = Avanzado
host-folder-placeholder = prod/web (opcional)
host-host-placeholder = 10.0.0.5 o host.example.com
host-user-placeholder = deploy (vacío = su nombre de usuario local)
host-keepalive-placeholder = segundos, 0 = desactivado
host-optional-placeholder = opcional
host-x11-placeholder-windows = vacío = localhost:0 (VcXsrv, Xming)
host-x11-placeholder = vacío = $DISPLAY
host-script-example = Ejemplo:
host-snippets-empty = No hay fragmentos para este host. Los fragmentos globales están en Configuración → Fragmentos.
host-log-placeholder = vacío = desactivado, o p. ej. &S-&Y&M&D-&T.log
host-sftp-server-placeholder = vacío = detectar (/usr/lib/openssh/sftp-server, …)
host-error-no-device = Elija el puerto serie (dispositivo)
host-error-no-host = Escriba el nombre o la dirección del host
host-error-no-auth = Elija al menos una forma de iniciar sesión
host-error-forwards = Reenvíos de puertos: { $error }
host-error-keepalive = El keepalive debe estar en segundos (de 0 a 3600)
host-error-proxy-host = Escriba el nombre de host del proxy
host-error-x11 = Pantalla X11: { $error }
host-error-script = Script de inicio de sesión, { $error }
host-saved-forget-pending = Se olvidará al guardar: { $what }
host-saved-locked = Guardado en su almacén (bloqueado): { $what }
host-saved = Guardado en su almacén: { $what }
host-saved-missing = Ya no está en su almacén, se le pedirá: { $what }
host-saved-keep = Conservar
host-saved-forget = Olvidar
host-what-password = contraseña
host-what-key-passphrase = frase de contraseña de la clave
host-what-proxy-password = contraseña del proxy
host-what-sudo-password = contraseña de sudo
host-pick-key = Clave privada (OpenSSH o .ppk)
host-name = Nombre
host-folder = Carpeta
host-protocol = Protocolo
host-host = Host
host-port = Puerto
host-user = Usuario
host-notes = Notas
host-telnet-hint = Telnet envía todo sin cifrar; úselo con equipos de red en una red de confianza. El nombre de usuario se envía si el servidor lo pide.
host-ports-looking = Buscando…
host-ports-none = No se encontraron puertos
host-ports-count = Puertos ({ $count })
host-ports-none-menu = No se encontraron puertos serie; escriba el nombre del dispositivo
host-device = Puerto (dispositivo)
host-ports-refresh = Volver a buscar puertos serie
host-device-hint-windows = COM3, COM4… Elija uno o escriba su nombre.
host-device-hint = /dev/ttyUSB0, /dev/ttyACM0, /dev/serial/by-id/… Elija uno o escriba su ruta.
host-baud = Velocidad (baudios)
host-data-bits = Bits de datos
host-parity = Paridad
host-parity-none = Ninguna
host-parity-odd = Impar
host-parity-even = Par
host-stop-bits = Bits de parada
host-flow = Control de flujo
host-flow-none = Ninguno
host-serial-none-linux = No se encontraron puertos. Los adaptadores USB aparecen como /dev/ttyUSB*; puede que tenga que pertenecer al grupo dialout.
host-serial-hint = La mayoría de las consolas usan 9600 8N1 o 115200 8N1 sin control de flujo.
host-auth-not-ssh = Las conexiones Telnet y serie inician sesión dentro del terminal. Use un script de inicio de sesión (Terminal) para automatizarlo.
host-key-files = Archivos de clave
host-keys-hint = Claves OpenSSH o PuTTY .ppk, separadas por comas. Un certificado junto a una clave (id_ed25519-cert.pub) se usa automáticamente. Las llaves de seguridad (sk-…) funcionan a través del agente.
host-auth-agent = Claves del agente SSH (Pageant, ssh-agent, 1Password…)
host-auth-password = Contraseña
host-auth-remember-hint = Marque «Recordar contraseña» cuando se le pida para iniciar sesión automáticamente la próxima vez.
host-auth-kbd = Interactiva por teclado (códigos de un solo uso, MFA)
host-auth-kerberos = Kerberos (inicio de sesión único)
host-auth-kerberos-forward = Reenviar mi ticket de Kerberos al servidor
host-auth-kerberos-hint-windows = Usa su inicio de sesión del dominio de Windows.
host-auth-kerberos-hint = Usa el ticket de kinit (vea klist). Si no hay ninguno, se prueba el siguiente método.
host-auth-order = Los métodos se prueban en este orden: Kerberos, agente, archivos de clave, contraseña, interactiva por teclado.
host-connection-serial = Los puertos serie son locales: sin proxy, hosts de salto ni reenvíos.
host-proxy = Proxy
host-proxy-none = Ninguno
host-proxy-host-port = Host / puerto del proxy
host-proxy-port = Puerto del proxy
host-proxy-user = Usuario del proxy
host-proxy-password-hint = La contraseña del proxy se pide al conectar (marque Recordar para guardarla).
host-jump-hosts = Hosts de salto
host-jump-hint = Separados por comas, el más externo primero (como ssh -J).
host-forwards = Reenvíos de puertos
host-forwards-hint = L = local → remoto, R = remoto → local, D = proxy SOCKS. Se inician con el terminal.
host-keepalive = Keepalive (s)
host-agent-forwarding = Reenviar mi agente SSH (git en el servidor usa mis claves)
host-agent-forwarding-hint = Solo en servidores de confianza: quien los administra puede usar sus claves mientras esté conectado.
host-x11 = Reenviar X11 (mostrar aquí programas gráficos remotos)
host-x11-display = Pantalla X
host-x11-hint-windows = Necesita un servidor X en ejecución, como VcXsrv o Xming.
host-x11-hint-macos = Necesita XQuartz.
host-x11-hint = Usa el servidor X de su escritorio (o XWayland).
host-terminal-type = Tipo de terminal
host-login-script = Script de inicio de sesión
host-login-script-hint = Un comando por línea, escrito tras iniciar sesión. @wait TEXTO espera esa salida (10 s), @wait 30000 TEXTO espera más, @sleep 500 hace una pausa; # inicia un comentario.
host-shell-integration = Integración con el shell (bash, zsh)
host-shell-integration-hint = Permite que el explorador de archivos siga la carpeta del terminal (clic derecho → Abrir archivos aquí). Añade discretamente un hook al prompt cuando se inicia el shell; no se instala nada en el servidor.
host-snippets-title = Fragmentos para este host
host-session-log = Registro de sesión
host-session-log-hint = Variables: &S sesión, &H host, &P puerto, &U usuario, &Y&M&D fecha, &T hora. Las rutas relativas van a la carpeta de registros.
host-raw-log = Registrar la salida sin procesar (con secuencias de escape)
host-files-not-ssh = El explorador de archivos necesita SSH.
host-file-protocol = Protocolo de archivos
host-file-protocol-auto = Automático
host-file-protocol-hint = Automático usa SFTP y recurre a SCP en servidores que no lo tienen (routers, ESXi, busybox).
host-sudo = Abrir el explorador de archivos como root (sudo)
host-sudo-user = Como usuario
host-sudo-hint = Funciona con sudo sin contraseña o con contraseña de sudo (solicitada o guardada). «Volver a abrir como root» en el explorador cambia en cualquier momento.
host-legacy = Permitir algoritmos antiguos (ssh-rsa/SHA-1, intercambio de claves antiguo) para equipos viejos
host-legacy-hint = Solo para switches, routers y dispositivos antiguos que los necesitan.
host-id = Id. del host: { $id }
host-title-edit = Editar host
host-title-new = Nuevo host
loading = Cargando…

## Settings

settings-title = Configuración
settings-section-appearance = Apariencia
settings-section-terminal = Terminal
settings-section-snippets = Fragmentos
settings-section-connections = Conexiones
settings-section-files = Archivos
settings-section-logging = Registro de sesiones
settings-section-passwords = Contraseñas guardadas
settings-section-sync = Sincronización de la configuración
settings-section-diagnostics = Diagnóstico
settings-language = Idioma
settings-language-system = Sistema ({ $language })
settings-language-hint = Se aplica en cuanto guarde. Los diálogos abiertos mantienen su idioma hasta que se vuelvan a abrir.
settings-theme = Tema
settings-theme-dark = Oscuro
settings-theme-light = Claro
settings-terminal-font = Fuente del terminal
settings-font-size = Tamaño de fuente
settings-font-placeholder = vacío = fuente monoespaciada del sistema
settings-font-hint = Los cambios de fuente se aplican a los terminales abiertos al guardar.
settings-editor-placeholder = vacío = aplicación predeterminada del sistema (p. ej. code -w { "{file}" })
settings-auto-lock-placeholder = 0 = mantener desbloqueado hasta cerrar Hostwick
settings-sync-folder-placeholder = Carpeta de OneDrive, Dropbox, iCloud Drive o git
settings-sync-pass-saved = Guardada en su almacén; escriba para cambiarla
settings-sync-pass-placeholder = La misma frase de contraseña en todos los equipos
settings-snippets-empty = Aún no hay fragmentos. Aparecen como botones encima de cada terminal y en clic derecho → Fragmentos.
settings-remove-failed = No se pudo quitar: { $error }
settings-error-font-size = Terminal → El tamaño de fuente debe ser un número de 6 a 48
settings-error-scrollback = Terminal → El historial debe ser un número entero de 100 a 1000000
settings-error-auto-lock = Contraseñas guardadas → Bloquear tras debe estar en minutos (de 0 a 1440)
settings-error-rules = Terminal → { $error }
settings-error-sync-folder = Sincronización de la configuración → la carpeta { $folder } no existe
settings-folder-missing = La carpeta { $folder } no existe.
settings-sync-pass-short = Use una frase de sincronización de al menos 8 caracteres.
settings-sync-pass-save-failed = No se pudo guardar la frase de contraseña: { $error }
vault-reason-sync-passphrase = La frase de sincronización se guarda en su almacén de contraseñas.
settings-save-failed = No se puede guardar la configuración: { $error }
settings-scrollback = Líneas de historial
settings-right-click = Clic derecho
settings-right-click-menu = Mostrar menú
settings-right-click-paste = Pegar (PuTTY)
settings-copy-on-select = Copiar automáticamente el texto seleccionado
settings-mouse = Enviar clics y desplazamiento del ratón a los programas (htop, mc, vim)
settings-mouse-hint = Mantenga pulsada Mayús para seleccionar texto mientras un programa usa el ratón.
settings-button-bar = Mostrar la barra de fragmentos encima de los terminales
settings-highlighting = Resaltado de palabras clave
settings-highlighting-hint = Colorea el texto coincidente en pantalla. Las reglas anteriores tienen prioridad cuando se solapan.
settings-snippets-hint = Disponibles en todos los terminales. Los hosts pueden añadir los suyos en Editar host → Terminal.
settings-auto-reconnect = Reconectar automáticamente si se corta la conexión
settings-auto-reconnect-hint = SSH y Telnet: tras 1, 2, 5, 10 y luego cada 30 segundos, conservando el historial. No después de escribir exit ni tras un fallo de inicio de sesión o de clave del host.
settings-restore-tabs = Volver a abrir mis pestañas al iniciar Hostwick
settings-restore-tabs-hint = Incluidos los paneles divididos y sus tamaños. Las pestañas se conectan cuando las visita por primera vez.
settings-editor = Editor externo
settings-editor-pick = Programa de edición
settings-editor-hint = Lo usa Editar (F4). { "{file}" } se sustituye por la ruta del archivo; si no, la ruta se añade al final. Use la opción de espera del editor (code -w, subl -w) si la tiene.
settings-log-all = Registrar cada sesión en un archivo
settings-log-folder = Carpeta de registros
settings-log-content = Contenido del registro
settings-log-printable = Texto imprimible
settings-log-raw = Sin procesar (con secuencias de escape)
settings-log-hint = La configuración de registro propia de un host (Editar host → Terminal) tiene prioridad.
settings-log-level = Nivel de registro de la aplicación
settings-open-log-folder = Abrir carpeta de registros
settings-data-folder = Carpeta de datos: { $path }
settings-vault-none = Aún no hay almacén. Marque «Recordar» cuando Hostwick pida una contraseña, o cree uno ahora.
settings-vault-locked = Bloqueado. Las contraseñas guardadas se usan después de desbloquearlas.
settings-vault-unlocked = Desbloqueado durante esta sesión.
settings-vault-unlock = Desbloquear…
settings-vault-create = Crear almacén…
settings-vault-lock = Bloquear ahora
settings-vault-change = Cambiar la contraseña maestra…
settings-vault-empty = Aún no hay nada guardado.
settings-vault-remove = Quitar
settings-auto-lock = Bloquear tras (min)
settings-auto-lock-hint = Minutos de inactividad antes de volver a bloquear las contraseñas guardadas; 0 las mantiene desbloqueadas hasta cerrar Hostwick.
settings-vault-file = Cifrado con Argon2id + XChaCha20-Poly1305 en { $path }: el mismo almacén que `hostwick vault`. Nunca se sincroniza.
settings-sync-never = Nunca se ha sincronizado en este equipo.
settings-sync-intro = Los hosts y la configuración se guardan en un archivo cifrado dentro de una carpeta que su nube o git mantiene al día. Las contraseñas guardadas nunca salen de este equipo.
settings-sync-folder = Carpeta
settings-sync-folder-pick = Carpeta de sincronización
settings-sync-passphrase = Frase de contraseña
settings-sync-passphrase-hint = Se guarda en su almacén de contraseñas. Todos los equipos necesitan la misma.
settings-sync-auto = Sincronizar al iniciar, tras cada cambio y cada 5 minutos
settings-sync-now = Sincronizar ahora
settings-sync-push = Reemplazar archivo
settings-sync-push-tooltip = Hacer que el archivo de sincronización coincida con este equipo (también cambia su frase de contraseña)
settings-sync-pull = Reemplazar este equipo
settings-sync-pull-tooltip = Hacer que los hosts y la configuración de este equipo coincidan con el archivo de sincronización
settings-sync-last = Última sincronización

## Snippet and highlight-rule editors

snippet-label-placeholder = Texto del botón
snippet-text-placeholder = Texto que se enviará (varias líneas se pegan como un solo bloque)
snippet-press-enter = Pulsar Intro
snippet-move-up = Subir
snippet-move-down = Bajar
editor-remove = Quitar
snippet-add = Añadir fragmento
rule-error-empty = patrón vacío
rule-error-color = el color debe tener el formato #ff8800, no `{ $color }`
rule-error-regex = expresión regular no válida
rule-pattern-placeholder = palabra o expresión regular
rule-error = Resaltado `{ $pattern }`: { $error }
rule-enabled = Activada
rule-regex-on = Expresión regular (haga clic para usar una palabra simple)
rule-regex-off = Palabra simple, sin distinguir mayúsculas (haga clic para usar una expresión regular)
rule-add = Añadir regla
rule-defaults = Restaurar valores predeterminados

## Terminal

script-timeout = se esperó { $after } a «{ $text }», que nunca apareció
script-session-ended = la sesión terminó antes
search-too-complex = patrón demasiado complejo
term-exit-status = terminó con el estado { $code }
term-exit-signal = terminado por la señal { $signal }
term-connection-lost = conexión perdida
term-connection-closed = conexión cerrada
term-reconnected-marker = reconectado a las { $time }
term-reconnect-failed = error al reconectar: { $error }
term-break-failed = No se pudo enviar el break
term-no-break = Las sesiones { $protocol } no tienen señal de break
term-script-not-run = No se ejecutó el script de inicio de sesión: { $error }
term-script-stopped = Script detenido: { $error }
search-placeholder = Buscar en el historial
search-no-results = Sin resultados
search-position = { $index } de { $total }
search-matches = { $count ->
    [one] { $count } coincidencia
   *[other] { $count } coincidencias
}
search-match-case = Distinguir mayúsculas
search-regex = Expresión regular
search-older = Coincidencia anterior (más antigua) — Intro, F3
search-newer = Coincidencia siguiente (más reciente) — Mayús+Intro, Mayús+F3
search-close = Cerrar (Esc)
term-reconnect-in = Conexión perdida{ $reason }. Reconectando en { $seconds } s…
term-reconnect-attempt = Reconectando (intento { $attempt })…
term-log-badge = REG
term-reconnect-now = Ahora
term-session-ended = Sesión terminada — { $reason }. Pulse Intro para reconectar.
term-menu-copy = Copiar
term-menu-paste = Pegar
term-menu-select-all = Seleccionar todo
term-menu-find = Buscar…
term-menu-snippets = Fragmentos
term-menu-copy-folder = Copiar la carpeta actual
term-menu-stop-script = Detener script
term-menu-clear = Borrar historial
term-menu-stop-log = Detener el registro de sesión
term-menu-start-log = Registrar la sesión en un archivo…
term-menu-files-here = Abrir archivos aquí
term-menu-duplicate = Duplicar sesión
term-menu-reconnect-ended = Reconectar (sesión terminada)

## File browser

files-move = Mover
files-upload = Subir
files-download = Descargar
files-move-confirm = { $count ->
    [one] ¿Mover «{ $name }» a { $dir }?
   *[other] ¿Mover { $count } elementos a { $dir }?
}
files-upload-confirm = { $count ->
    [one] ¿Subir «{ $name }» a { $dir }?
   *[other] ¿Subir { $count } elementos a { $dir }?
}
files-download-confirm = { $count ->
    [one] ¿Descargar «{ $name }» en { $dir }?
   *[other] ¿Descargar { $count } elementos en { $dir }?
}
files-same-place = { $name }: el origen y el destino son el mismo
files-skipped-unsafe = No se muestran porque sus nombres no son seguros en este equipo: { $names }
files-not-removed = { $name }: copiado, pero no eliminado: { $error }
files-pick-download = Descargar aquí
files-exists = «{ $name }» ya existe
files-delete-title = Eliminar
files-delete-folder-confirm = ¿Eliminar definitivamente la carpeta «{ $name }» y todo su contenido?
files-delete-file-confirm = ¿Eliminar definitivamente el archivo «{ $name }»?
files-delete-many-confirm = ¿Eliminar definitivamente { $count } elementos (las carpetas con todo su contenido)?
files-name-invalid = Escriba un nombre sin «/»
files-new-folder = Nueva carpeta
files-create = Crear
files-folder-name = Nombre de la carpeta
files-rename = Cambiar nombre
files-new-name = Nuevo nombre
files-new-symlink = Nuevo enlace simbólico
files-symlink-placeholder = destino -> nombre del enlace
files-symlink-format = Escríbalo como «destino -> nombre del enlace»
files-symlink-empty = El destino está vacío
edit-conflict-title = Cambiado en el servidor
edit-conflict-body = «{ $name }» cambió en el servidor desde que lo abrió. ¿Qué debe pasar con su versión guardada?
edit-conflict-later = Decidir más tarde
edit-conflict-theirs = Descargar la del servidor
edit-conflict-copy = Guardar como copia
edit-conflict-overwrite = Sobrescribir
edit-editor-failed = no se pudo iniciar el editor «{ $program }»: { $error }
keep-up-to-date = al día
keep-synced = { $count ->
    [one] { $count } archivo sincronizado, { $size }
   *[other] { $count } archivos sincronizados, { $size }
}
keep-failed = { $count } con error
keep-error = error (reintentando): { $error }
keep-starting = iniciando…
keep-status = Manteniendo al día { $local } → { $remote }: { $status }
edits = Ediciones
edits-busy = Ediciones ({ $count }): { $status }
edits-tooltip = Archivos abiertos en un editor; al guardar se suben automáticamente
edits-stop = Dejar de vigilar
edits-stop-all = Dejar de vigilar todos
files-hide-local = Ocultar local
files-show-local = Mostrar local
files-local-tooltip = Mostrar u ocultar los archivos del equipo local
files-upload-pick = Subir…
files-synchronize = Sincronizar…
files-synchronize-tooltip = Comparar carpetas, revisar los cambios y luego aplicarlos
files-keep-stop = Dejar de mantener al día
files-keep = Mantener al día…
files-keep-tooltip = Subir automáticamente los cambios de una carpeta local a una carpeta remota
files-following = Siguiendo al terminal
files-follow = Seguir al terminal
files-follow-tooltip = Mostrar la carpeta en la que está el terminal «{ $terminal }» a medida que cambia
files-root-switching = Cambiando…
files-root-on = Root: activado
files-root-reopen = Volver a abrir como root
files-root-tooltip = Explorar el servidor como root (o como el usuario sudo del host) mediante sudo
files-path-placeholder = /ruta
files-this-computer = Este equipo
files-selected-of = { $selected } de { $count } seleccionados ({ $size })
files-item-count = { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
}
files-selected = { $count ->
    [one] { $count } elemento
   *[other] { $count } elementos
}
files-root-badge = ROOT
files-up-tooltip = Subir un nivel (Retroceso)
files-refresh-tooltip = Actualizar (Ctrl+R)
files-mkdir-tooltip = Nueva carpeta (F7)
files-col-name = Nombre
files-col-size = Tamaño
files-col-modified = Modificado
files-col-mode = Permisos
files-download-pick = Descargar…
files-menu-open = Abrir
files-menu-edit = Editar
files-menu-move = Mover al otro lado
files-menu-rename = Cambiar nombre…
files-menu-new-folder = Nueva carpeta…
files-menu-new-symlink = Nuevo enlace simbólico…
files-menu-copy-path = Copiar ruta
files-menu-refresh = Actualizar
files-menu-properties = Propiedades
edit-status-downloading = descargando…
edit-status-watching = vigilando
edit-status-uploading = subiendo…
edit-status-uploaded = subido a las { $time }
edit-status-uploaded-in-place = subido a las { $time } (reescrito en su sitio)
edit-status-conflict = cambiado en el servidor; esperando su decisión
edit-status-error = error: { $error }
edit-gone = { $path }: ya no existe
edit-kept-folder = Los archivos editados que no se subieron se guardaron en { $path }
edit-is-folder = { $path } es una carpeta
edit-missing-after-upload = { $path }: no está tras la subida
edit-size-mismatch = { $path }: el servidor tiene { $server } bytes tras subir { $uploaded }
edit-no-copy-name = no hay ningún nombre libre para la copia

## File properties

props-title = Propiedades
props-applying = Aplicando…
props-apply = Aplicar
props-calculating = calculando…
props-error = error: { $error }
props-usage = { $size } ({ $bytes } bytes) en { $files ->
    [one] { $files } archivo
   *[other] { $files } archivos
}, { $folders ->
    [one] { $folders } carpeta
   *[other] { $folders } carpetas
}
props-usage-partial = (detenido antes: demasiadas entradas)
props-bad-id = «{ $id }» no es un id. numérico
props-bad-octal = Los permisos deben estar en octal, p. ej. 0644
props-kind-folder = Carpeta
props-kind-file = Archivo
props-kind-symlink = Enlace simbólico → { $target }
props-kind-special = Archivo especial
props-unknown-name = (nombre desconocido)
props-not-reported = el servidor no lo indica
props-read = Lectura
props-write = Escritura
props-execute = Ejecución
props-owner = Propietario
props-group = Grupo
props-others = Otros
props-special = Especiales
props-octal = Octal
props-calculate = Calcular
props-size = { $size } ({ $bytes } bytes)
props-location = Ubicación
props-type = Tipo
props-checksum = Suma de comprobación
props-unknown = desconocido
props-permissions = Permisos
props-recursive = Aplicar a todo el contenido (los archivos conservan sus bits de ejecución)

## Synchronize dialog

files-synchronize-title = Sincronizar
sync-keep = Mantener al día el remoto
sync-keep-tooltip = Vigilar la carpeta local y subir cada cambio (solo local → remoto)
sync-comparing = Comparando…
sync-preview = Vista previa
sync-run-checked = Sincronizar lo marcado
sync-exclude-placeholder = p. ej. .git, *.tmp, node_modules
sync-choose-folders = Elija ambas carpetas
sync-use-local = Usar el local
sync-use-remote = Usar el remoto
sync-total-uploads = { $count ->
    [one] { $count } subida, { $size }
   *[other] { $count } subidas, { $size }
}
sync-total-downloads = { $count ->
    [one] { $count } descarga, { $size }
   *[other] { $count } descargas, { $size }
}
sync-total-deletions = { $count ->
    [one] { $count } eliminación
   *[other] { $count } eliminaciones
}
sync-total-conflicts = { $count ->
    [one] { $count } conflicto: elija un lado o déjelo
   *[other] { $count } conflictos: elija un lado o déjelos
}
sync-in-sync = Las dos carpetas ya están sincronizadas.
sync-checked = Marcado: { $items }
sync-local-folder = Carpeta local
sync-remote-folder = Carpeta remota
sync-direction = Dirección
sync-dir-up = Local → remoto
sync-dir-down = Remoto → local
sync-dir-both = Ambos (gana el más reciente)
sync-mirror = Eliminar archivos que no están en el origen (espejo)
sync-exclude = Excluir
sync-comparing-folders = Comparando carpetas…
sync-result = Sincronizar: { $summary }
sync-running = Sincronizando…
sync-run-failed = Error al sincronizar: { $error }
sync-kind-upload = Subir
sync-kind-download = Descargar
sync-kind-delete-local = Eliminar local
sync-kind-delete-remote = Eliminar remoto
sync-kind-conflict = Conflicto
sync-kind-skip = Omitir
sync-reason-new = nuevo
sync-reason-newer = más reciente
sync-reason-size = tamaño distinto
sync-reason-differs = distinto
sync-reason-not-in-source = no está en el origen
sync-reason-both-changed = cambiado en ambos lados (misma hora, distinto tamaño)
sync-reason-unknown = no se puede comparar (sin marca de tiempo)
sync-reason-destination-newer = el destino es más reciente
sync-reason-new-folder = carpeta nueva
sync-reason-case-conflict = otro archivo solo difiere en mayúsculas y minúsculas
sync-review-deletions = Esta sincronización elimina archivos. Revise la lista de abajo y vuelva a pulsar Sincronizar.
sync-report-uploaded = { $count } subidos
sync-report-downloaded = { $count } descargados
sync-report-deleted = { $count } eliminados
sync-report-folders = { $count ->
    [one] { $count } carpeta creada
   *[other] { $count } carpetas creadas
}
sync-report-failed = { $count } con error
sync-report-nothing = nada que hacer

## Key tools

keyformat-openssh-hint = Para OpenSSH, Hostwick, Git y la mayoría de herramientas (también se escribe un archivo .pub).
keyformat-ppk3-hint = Para PuTTY 0.75+ y WinSCP (protegido con Argon2).
keyformat-ppk2-hint = Para versiones antiguas de PuTTY, WinSCP y FileZilla.
key-fingerprint = Huella: { $fingerprint }
keygen-title = Generar clave
keygen-generate = Generar
keygen-passphrase-placeholder = Frase de contraseña (recomendada; vacío = ninguna)
keygen-repeat = Repetir
keygen-mismatch = Las frases de contraseña no coinciden.
keygen-no-path = Elija dónde guardar la clave.
keygen-exists = { $path } ya existe. Elija otro nombre o marque «Sobrescribir».
keygen-used-in-host = { $name } ahora inicia sesión con { $path }. Añada la clave pública al archivo ~/.ssh/authorized_keys del servidor.
keygen-ed25519-recommended = Ed25519 (recomendada)
keygen-saved = Se guardó { $file } (clave privada) y { $file }.pub en { $dir }
keygen-public-key = Clave pública (para ~/.ssh/authorized_keys):
keygen-copy-public = Copiar clave pública
keygen-use-in-host = Usar en un host…
keygen-type = Tipo
keygen-comment = Comentario
keygen-format = Formato
keygen-save-as = Guardar como
keygen-overwrite = Sobrescribir un archivo existente
keygen-generating-rsa = Generando… RSA 4096 puede tardar unos segundos.
keygen-generating = Generando…
keyconv-title = Examinar o convertir clave
keyconv-path-placeholder = ~/.ssh/id_ed25519 o C:\keys\work.ppk
keyconv-current-placeholder = Frase de contraseña actual (si tiene)
keyconv-new-placeholder = Nueva frase de contraseña (vacío = ninguna)
keyconv-repeat-placeholder = Repetir la nueva frase de contraseña
keyconv-output = Archivo de salida
keyconv-mismatch = Las nuevas frases de contraseña no coinciden.
keyconv-pass-changed = Frase de contraseña cambiada ({ $format }).
keyconv-pass-removed = Frase de contraseña eliminada ({ $format }).
keyconv-wrote = Se escribió { $path } ({ $format }).
keyconv-needs-passphrase = Esta clave tiene frase de contraseña: escríbala en «Frase de contraseña actual».
keyconv-wrong-passphrase = Frase de contraseña incorrecta.
keyconv-exists = { $path } ya existe; elija otro archivo de salida.
keyconv-key-file = Archivo de clave
keyconv-type-bits = { $kind } de { $bits } bits ({ $algorithm })
keyconv-protected = Protegida
keyconv-public-only = solo clave pública
keyconv-encrypted = sí, con frase de contraseña
keyconv-no-passphrase = sin frase de contraseña
keyconv-fingerprint = Huella
keyconv-inspect = Examinar
keyconv-current = Actual
keyconv-new = Nueva frase de contraseña
keyconv-change = Cambiar frase de contraseña
keyconv-convert-to = Convertir a
keyconv-convert = Convertir
keyconv-keeps-passphrase = Conserva la frase de contraseña actual salvo que escriba una nueva.
keyconv-working = Procesando…
kh-title = Hosts conocidos
kh-search-placeholder = Buscar host, tipo de clave o huella
kh-forget-placeholder = host o host:puerto
kh-nothing-matched = No hubo coincidencias; el archivo no ha cambiado.
kh-removed = { $count ->
    [one] Se quitó { $count } clave.
   *[other] Se quitaron { $count } claves.
}
kh-type-host = Escriba un nombre de host o host:puerto.
kh-col-line = Línea
kh-col-hosts = Host(s)
kh-empty = Aún no hay hosts conocidos. Las claves se añaden cuando confía en un servidor.
kh-no-match = Nada coincide con la búsqueda.
kh-reload = Recargar
kh-remove-selected = Quitar lo seleccionado ({ $count })
kh-forget-host = Olvidar host
kh-forget-button = Quitar todas sus claves
kh-footer = { $path }: se muestran { $shown } de { $total }. Los nombres con hash también coinciden cuando olvida un host por su nombre.
files-opening-root = abriendo archivos como root
files-opening = abriendo el explorador de archivos

## Keyboard and screen readers

sidebar-vault-lock = Bloquear las contraseñas guardadas
sidebar-quick-connect = Conexión rápida (user@host:port, Intro)
sidebar-hosts-label = Hosts guardados
sidebar-hosts-keys = Las flechas mueven (Mayús+flechas selecciona varios), Izquierda y Derecha pliegan carpetas. Intro abre un terminal, Ctrl+Intro los archivos, Mayús+Intro una nueva división, Alt+Intro edita, F2 cambia el nombre, Supr elimina, Ctrl+A selecciona todo, Mayús+F10 abre el menú.
tabs-label = Sesiones abiertas
tabs-keys = Izquierda y Derecha cambian de pestaña, Intro entra en la pestaña, Supr la cierra, Mayús+F10 abre su menú. Ctrl+F6 pasa por la lista de hosts, las pestañas y la sesión. Ctrl+Mayús+Izquierda y Derecha mueven la pestaña.
tab-state-not-connected = aún sin conectar
tab-state-connected = conectada
tab-state-disconnected = desconectada
tab-spoken = { $title }, { $state }, pestaña { $index } de { $count }
tab-close = Cerrar { $title }
pane-divider = Divisor de paneles

## Proxy test

vault-reason-proxy-test = Desbloquee sus contraseñas guardadas para usar la contraseña del proxy guardada.
proxy-test-closed = El proxy conectó, pero { $target } cerró la conexión de inmediato.
proxy-test-ok-ssh = Conectado con { $target } a través del proxy. El servidor dice: { $server }
proxy-test-ok = Conectado con { $target } a través del proxy (el servidor no es un servidor SSH).
proxy-test-ok-quiet = Conectado con { $target } a través del proxy (el servidor aún no ha enviado nada).
proxy-test-read-failed = Conectado con { $target } a través del proxy, y después: { $error }
host-proxy-test = Probar conexión
host-proxy-testing = Probando…
host-proxy-test-tooltip = Conectar a través del proxy con el puerto de este host (o del primer host de salto) e informar del resultado
host-baud-common = Velocidades habituales
dismiss = Cerrar
term-no-output = Aún no hay salida.
term-text-view-title = Salida del terminal: { $title }
term-text-view-hint = { $lines ->
    [one] { $lines } línea. Léala con las flechas; Esc vuelve al terminal.
   *[other] { $lines } líneas. Léalas con las flechas; Esc vuelve al terminal.
}
term-a11y-label = Terminal: { $title }
term-a11y-keys = La salida nueva se lee cuando se estabiliza. { $read } vuelve a leer la última salida, { $view } muestra toda la salida como texto. Ctrl+F6 sale del terminal.
term-menu-read-output = Leer la última salida
term-menu-text-view = Mostrar la salida como texto…
files-path-label = Carpeta: { $side }
files-list-label = { $side }: { $dir }
files-list-keys = Las flechas mueven, Mayús+flechas amplía la selección, Ctrl+Espacio o Insert añade o quita una fila, Espacio muestra la vista previa, escribir filtra la lista, Intro abre, Retroceso sube un nivel, Ctrl+L escribe una ruta, Ctrl+D marca la carpeta, F5 copia al otro lado, F6 mueve, F2 cambia el nombre, Supr elimina, Alt+Intro muestra las propiedades, Tab cambia de lado, Mayús+F10 abre el menú.
files-kind-link = enlace
a11y-sections = Secciones
a11y-section-problem = { $section } (requiere atención)
kh-row-label = Línea { $line }: { $hosts }
rule-color = Color (#rrggbb)
toast-info = Información
toast-success = Hecho
toast-warning = Advertencia
toast-error = Error
toast-close = Cerrar notificación
menu-close-pane = Cerrar panel
pane-files-title = Archivos: { $host }

## Command palette, sidebar, bulk edit, welcome (0.4)

prompt-passphrase-title-host = Frase de contraseña de { $path } ({ $host })
prompt-server-auth-title-host = Autenticación del servidor — { $host }
act-broadcast = Activar o desactivar la difusión
act-new-folder = Nueva carpeta
act-quick-connect = Conexión rápida…
act-settings-section = Configuración: { $section }
act-toggle-theme = Cambiar entre tema oscuro y claro
bulk-applies = { $count ->
    [one] Se aplica a { $count } host
   *[other] Se aplica a { $count } hosts
}
bulk-apply = Aplicar
bulk-changed = cambiará
bulk-done = { $count ->
    [one] { $count } host actualizado
   *[other] { $count } hosts actualizados
}
bulk-error-proxy = Proxy: { $error }
bulk-intro = { $count ->
    [one] Cambia la configuración de { $count } host.
   *[other] Cambia la configuración de { $count } hosts a la vez.
} Cada campo muestra el valor que comparten; un campo vacío significa que son distintos. Solo se guardan los campos que cambies.
bulk-keep = Mantener
bulk-mixed = distinto — déjalo vacío para mantenerlo
bulk-only-ssh = Solo se cambian los hosts SSH ({ $ssh } de { $total }).
bulk-proxy-hint = socks5://[usuario@]host:1080, socks4://… o http://…; «none» quita el proxy.
bulk-serial-skipped = { $count ->
    [one] { $count } host serie lo conserva todo salvo su carpeta.
   *[other] { $count } hosts serie lo conservan todo salvo su carpeta.
}
bulk-title = { $count ->
    [one] Editar { $count } host
   *[other] Editar { $count } hosts
}
folder-delete-body = { $count ->
    [one] ¿Eliminar la carpeta «{ $name }»? Su { $count } host y las subcarpetas pasan a «{ $target }».
   *[other] ¿Eliminar la carpeta «{ $name }»? Sus { $count } hosts y las subcarpetas pasan a «{ $target }».
}
folder-delete-ok = Eliminar carpeta
folder-delete-title = Eliminar carpeta
folder-new-name = Nueva carpeta
group-broadcast-on = La difusión está activada: lo que escribas va a todos los terminales. { $keys } la desactiva.
group-confirm-body = { $count ->
    [one] Esto abre { $count } conexión.
   *[other] Esto abre { $count } conexiones a la vez.
} Las preguntas, como las contraseñas, llegan de una en una y cada una indica su host.
group-confirm-open = Abrir todos
group-confirm-title = { $count ->
    [one] ¿Abrir { $count } host?
   *[other] ¿Abrir { $count } hosts?
}
hosts-delete-body = Se eliminarán estos hosts guardados: { $names }.
hosts-delete-body-more = Se eliminarán estos hosts guardados: { $names } y { $more } más.
hosts-delete-title = { $count ->
    [one] ¿Eliminar { $count } host?
   *[other] ¿Eliminar { $count } hosts?
}
hosts-save-failed = No se pudo guardar: { $error }
import-menu-scan = Buscar en este equipo…
keys-palette = Paleta de comandos
keys-shortcuts = Atajos de teclado
keys-new-host = Nuevo host
keys-settings = Configuración
keys-broadcast = Difundir lo escrito a todos los terminales
keys-cycle-focus = Mover el foco: lista de hosts, pestañas, sesión
keys-context-menu = Menú contextual del elemento enfocado
keys-duplicate-tab = Duplicar pestaña
keys-close-pane = Cerrar el panel y luego la pestaña
keys-reconnect = Reconectar
keys-next-tab = Pestaña siguiente
keys-prev-tab = Pestaña anterior
keys-split-right = Dividir a la derecha
keys-split-down = Dividir abajo
keys-move-pane = Moverse entre paneles divididos
keys-resize-pane = Cambiar el tamaño del panel enfocado
keys-host-open = Abrir un terminal
keys-host-files = Abrir los archivos
keys-host-split = Abrir en una nueva división
keys-host-edit = Editar (varios hosts: editar juntos)
keys-host-rename = Cambiar el nombre de un host o carpeta
keys-host-delete = Eliminar
keys-host-select-all = Seleccionar todos los hosts
keys-host-multi = Seleccionar varios hosts
keys-host-range = Seleccionar un rango de hosts
keys-copy = Copiar
keys-paste = Pegar
keys-find = Buscar en el historial
keys-find-next = Coincidencia anterior / siguiente
keys-scroll-back = Desplazarse hacia atrás
keys-select-in-app = Seleccionar texto mientras un programa usa el ratón
keys-send-break = Enviar break (telnet, serie)
keys-read-output = Volver a leer la última salida (lectores de pantalla)
keys-text-view = Salida como texto sin formato
keys-files-open = Abrir
keys-files-up = Carpeta superior
keys-files-edit = Editar en tu editor
keys-files-copy = Subir / descargar
keys-files-move = Mover
keys-files-rename = Cambiar nombre
keys-files-new-folder = Nueva carpeta
keys-files-delete = Eliminar
keys-files-refresh = Actualizar
keys-files-switch = Otro lado
keys-files-properties = Propiedades
keys-files-select-all = Seleccionar todo
keys-files-toggle-row = Marcar la fila
keys-group-app = Aplicación
keys-group-tabs = Pestañas y paneles
keys-group-hosts = Lista de hosts
keys-group-terminal = Terminal
keys-group-files = Archivos
keys-search = Buscar atajos
keys-note = Las teclas Ctrl+letra simples siempre van a la shell. Dentro de un terminal, F1 y Ctrl+/ son del programa (htop, mc); Ctrl+Mayús+/ abre esta lista desde cualquier sitio. En macOS ⌘ sustituye a Ctrl+Mayús.
menu-bulk-edit = { $count ->
    [one] Editar { $count } host…
   *[other] Editar { $count } hosts…
}
menu-clear-recent = Borrar conexiones recientes
menu-delete-folder = Eliminar carpeta…
menu-delete-hosts = { $count ->
    [one] Eliminar { $count } host…
   *[other] Eliminar { $count } hosts…
}
menu-favourite = Añadir a favoritos
menu-unfavourite = Quitar de favoritos
menu-new-subfolder = Nueva subcarpeta
menu-open-all-files = Abrir todos en el explorador de archivos
menu-open-all-grid = Abrir todos en una cuadrícula
menu-open-all-grid-broadcast = Abrir todos en una cuadrícula con difusión
menu-open-all-tabs = Abrir todos en pestañas
menu-open-all-tabs-count = Abrir todos en pestañas ({ $count })
menu-open-split = Abrir en una nueva división
menu-rename = Cambiar nombre
palette-connect-to = Conectar a { $target }
palette-save-as-host = Guardar { $target } como host…
palette-group-actions = Acciones
palette-group-connect = Conectar
palette-group-snippets = Fragmentos
palette-group-ssh-config = De ~/.ssh/config
palette-group-tabs = Pestañas abiertas
palette-hint-open = abrir
palette-hint-files = archivos
palette-hint-split = dividir
palette-hint-edit = editar
palette-hint-close = cerrar
palette-no-terminal = Los fragmentos se escriben en un terminal; abre uno primero.
palette-nothing = No se encontró nada
palette-placeholder = Busca hosts y acciones, o escribe usuario@host:puerto
palette-quick-detail = conexión rápida
palette-results = Resultados
palette-title = Paleta de comandos
quick-hint = ↵ conectar · { $files } archivos · ↑↓ elegir
quick-suggestions = Sugerencias
rename-no-slash = Los nombres no pueden contener / ni \.
shortcut-palette = paleta de comandos
shortcut-all-shortcuts = todos los atajos
sidebar-drag-hosts = { $count ->
    [one] { $count } host
   *[other] { $count } hosts
}
sidebar-favourite-toggle = Favorito: { $name }
sidebar-folder-spoken = { $count ->
    [one] Carpeta { $name }, { $count } host
   *[other] Carpeta { $name }, { $count } hosts
}
sidebar-help-tooltip = Atajos de teclado ({ $keys })
sidebar-palette-tooltip = Buscar hosts y acciones ({ $keys })
sidebar-quick-spoken = Conexión rápida reciente { $target }
sidebar-section-favourites = Favoritos
sidebar-section-recent = Recientes
sidebar-section-hosts = Hosts
sidebar-selected-count = { $count ->
    [one] { $count } seleccionado
   *[other] { $count } seleccionados
}
welcome-title = Te damos la bienvenida a Hostwick
welcome-intro = Hostwick puede traer los hosts que otras herramientas SSH guardaron en este equipo. Marca lo que quieras importar; los otros programas no se modifican.
welcome-scanning = Buscando sesiones guardadas…
welcome-found = Encontrado en este equipo
welcome-src-ssh = { $count ->
    [one] Configuración de OpenSSH ({ $path }): { $count } host
   *[other] Configuración de OpenSSH ({ $path }): { $count } hosts
}
welcome-src-putty = { $count ->
    [one] Sesiones de PuTTY: { $count } host
   *[other] Sesiones de PuTTY: { $count } hosts
}
welcome-src-winscp-registry = { $count ->
    [one] Sitios de WinSCP: { $count } host
   *[other] Sitios de WinSCP: { $count } hosts
}
welcome-src-winscp-ini = { $count ->
    [one] Sitios de WinSCP ({ $path }): { $count } host
   *[other] Sitios de WinSCP ({ $path }): { $count } hosts
}
welcome-src-solar = { $count ->
    [one] Exportación de Solar-PuTTY ({ $path }): { $count } host
   *[other] Exportación de Solar-PuTTY ({ $path }): { $count } hosts
}
welcome-keys = { $count ->
    [one] Usar mi archivo de clave de ~/.ssh para conexiones rápidas y hosts nuevos
   *[other] Usar mis { $count } archivos de clave de ~/.ssh para conexiones rápidas y hosts nuevos
}
welcome-agent = { $count ->
    [one] { $count } clave está lista en tu agente SSH (Pageant, ssh-agent…); los hosts la usan automáticamente.
   *[other] { $count } claves están listas en tu agente SSH (Pageant, ssh-agent…); los hosts las usan automáticamente.
}
welcome-agent-asking = Preguntando al agente SSH por sus claves…
welcome-agent-none = No hay ningún agente SSH con claves en ejecución.
welcome-nothing = No se encontraron sesiones guardadas por otras herramientas SSH. Añade un host o escribe usuario@host en la conexión rápida.
welcome-note = Puedes repetirlo cuando quieras: Importar… → Buscar en este equipo.
welcome-import = Importar lo seleccionado
welcome-skip = Omitir
welcome-continue = Continuar

# --- v0.4 tabs/files ---
files-bookmark-add = Añadir esta carpeta a marcadores
files-bookmark-remove = Quitar esta carpeta de marcadores
files-bookmark-remove-menu = Quitar marcador
files-bookmarks = Marcadores
files-bookmarks-empty = Aún no hay marcadores. Pulsa la estrella para añadir esta carpeta.
files-crumb-go = Ir a { $folder }
files-crumbs-more = Carpetas superiores
files-filter-label = Filtrar archivos en { $side }
files-filter-placeholder = Filtrar…
files-filtered-count = { $count } de { $total } mostrados
files-path-click-to-edit = Haz clic para escribir una ruta (Ctrl+L)
keys-files-bookmark = Marcar esta carpeta
keys-files-filter = Filtrar la lista (basta con escribir)
keys-files-path = Escribir una ruta
keys-files-preview = Vista previa rápida
keys-reopen-tab = Reabrir la pestaña cerrada
keys-tab-number = Ir a la pestaña 1–8 · última pestaña (9)
keys-zoom-in = Acercar (texto del terminal)
keys-zoom-out = Alejar (texto del terminal)
keys-zoom-reset = Restablecer el zoom
menu-move-new-window = Mover a una ventana nueva
menu-move-to-window = Mover a la ventana
preview-close = Cerrar la vista previa
preview-empty = Este archivo está vacío.
preview-error = No se puede leer este archivo: { $error }
preview-folder = Esto es una carpeta.
preview-hint = Espacio o Esc cierra · ↑ ↓ muestran el archivo siguiente · RePág / AvPág desplazan
preview-label = Vista previa de { $name }
preview-no-preview = No hay vista previa para este tipo de archivo.
preview-text-cut = { $encoding } · solo se muestran los primeros { $limit }
preview-text-info = { $encoding }
preview-too-large = Esta imagen supera { $limit }, así que no se muestra.
settings-ui-scale = Tamaño de la interfaz
settings-ui-scale-hint = Escala el texto, los botones y los diálogos en todas partes (90–150 %). Solo en este equipo.
shortcut-jump-tab = ir a la pestaña
shortcut-reopen-tab = reabrir pestaña cerrada
tab-nothing-to-reopen = No hay ninguna pestaña cerrada que reabrir.
tab-reopen-host-gone = El host de esa pestaña ya no existe, así que no se puede reabrir.
tabs-list = Todas las pestañas
transfer-pause = Pausar
transfer-paused = en pausa en { $done } de { $total }
transfer-resume = Reanudar
transfer-retry = Reintentar
transfer-show-in-folder = Mostrar en la carpeta
transfer-waiting = en espera…
transfers-batch-done = { $count ->
    [one] Se transfirió { $count } archivo ({ $size })
   *[other] Se transfirieron { $count } archivos ({ $size })
}
transfers-batch-failed = { $done } completadas, { $failed } con error
transfers-batch-title = Transferencias terminadas
transfers-cancel-all = Cancelar todas
transfers-collapse = Contraer la lista de transferencias
transfers-expand = Expandir la lista de transferencias
transfers-pause-all = Pausar todas
transfers-resume-all = Reanudar todas
transfers-retry-failed = Reintentar las fallidas
transfers-sum-done = { $count } terminadas
transfers-sum-eta = quedan { $eta }
transfers-sum-failed = { $count } con error
transfers-sum-paused = { $count } en pausa
transfers-sum-running = { $count } en curso
transfers-sum-speed = { $rate }/s
transfers-sum-waiting = { $count } en espera
window-gone = Esa ventana se ha cerrado.
window-name = Ventana { $n }
window-name-with-tab = Ventana { $n } — { $tab }
zoom-level = Zoom { $percent } %
