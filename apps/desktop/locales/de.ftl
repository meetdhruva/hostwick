# Hostwick desktop: Deutsch.
#
# Übersetzung von en.ftl (Quelle). Platzhalter ({ $name }) und Schlüssel
# bleiben unverändert; Produkt- und Protokollnamen (SSH, SFTP, SCP, sudo,
# Kerberos, PuTTY, WinSCP, X11, SOCKS) werden nicht übersetzt.

## General

error-internal = Interner Fehler: { $error }

## Main window

sidebar-search-placeholder = Hosts durchsuchen
split-only-terminals = Nur Terminal-Tabs können geteilt werden.
files-follow-no-folder = Das Terminal hat seinen Ordner noch nicht gemeldet. Aktivieren Sie „Shell-Integration“ unter Host bearbeiten → Terminal, damit der Dateibrowser ihm folgen kann (bash und zsh).
host-copy-name = { $name } (Kopie)
size-b = B
size-kib = KiB
size-mib = MiB
size-gib = GiB
size-tib = TiB
protocol-ssh = SSH
protocol-telnet = Telnet
protocol-serial = Seriell
sidebar-vault-unlocked-tooltip = Gespeicherte Passwörter sind entsperrt – zum Sperren klicken
sidebar-settings-tooltip = Einstellungen ({ $keys })
sidebar-open-terminal = Terminal
sidebar-open-files = Dateien
sidebar-edit-host = Host bearbeiten
sidebar-delete-host = Host löschen
sidebar-new-host = Neuer Host …
tab-files-title = Dateien: { $name }
broadcast = Rundsenden
broadcast-on = Rundsenden: AN
broadcast-tooltip = Tastatureingaben an alle offenen Terminals senden ({ $keys })
shortcut-new-host = neuer Host
shortcut-duplicate-tab = Tab duplizieren
shortcut-close-pane = Bereich/Tab schließen
shortcut-reconnect = neu verbinden
shortcut-next-tab = nächster Tab
shortcut-split-right = rechts teilen
shortcut-split-down = unten teilen
shortcut-move-between-panes = zwischen Bereichen wechseln
shortcut-broadcast = Rundsenden
shortcut-find = Suchen
shortcut-settings = Einstellungen
empty-title = Keine offenen Sitzungen
empty-hint-connect = Doppelklicken Sie auf einen Host oder geben Sie user@host in das Schnellverbindungsfeld ein.
empty-hint-menus = Weitere Aktionen per Rechtsklick auf Hosts, Tabs, Dateien und Terminals.
empty-hint-import = Importieren Sie aus ~/.ssh/config, PuTTY, WinSCP oder Solar-PuTTY mit „Importieren …“ (unten links).
pane-connecting = Verbindung zu { $host } wird hergestellt …
pane-retry = Erneut versuchen
transfer-progress = { $done } von { $total } – { $rate }/s
transfer-done = fertig – { $size }
transfer-skipped = übersprungen
transfer-cancelled = abgebrochen
transfer-failed = fehlgeschlagen: { $error }
transfer-cancel = Abbrechen
transfers-title = { $count ->
    [one] Übertragungen – { $count } aktiv
   *[other] Übertragungen – { $count } aktiv
}
transfers-clear = Abgeschlossene entfernen

## Common buttons

button-cancel = Abbrechen
button-delete = Löschen
button-ok = OK
button-save = Speichern
button-close = Schließen
button-browse = Durchsuchen …

## Menus, import

host-delete-title = Host löschen
host-delete-confirm = Den gespeicherten Host „{ $name }“ löschen?
sidebar-import = Importieren …
sidebar-tools = Werkzeuge
import-no-home = kein Benutzerordner gefunden
import-pick-putty = PuTTY-.reg-Export
import-pick-solar = Solar-PuTTY-Export (.json)
import-pick-winscp = WinSCP.ini
import-failed = Import fehlgeschlagen: { $error }
import-summary = { $added } hinzugefügt, { $updated } aktualisiert, { $notes } mit Hinweisen
import-done = Importiert: { $summary }
import-report-title = Importbericht
import-report-attention = Prüfen ({ $count })
import-report-added = Hinzugefügt ({ $count })
import-report-updated = Aktualisiert ({ $count })
import-report-note = Hosts unter „Prüfen“ wurden trotzdem importiert, sofern der Hinweis nicht „übersprungen“ sagt. Rechtsklick auf einen Host → Bearbeiten …, um ihn anzupassen.
import-menu-ssh-config = OpenSSH ~/.ssh/config
import-menu-putty-registry = PuTTY (gespeicherte Sitzungen dieses PCs)
import-menu-winscp-registry = WinSCP (gespeicherte Sites dieses PCs)
import-menu-putty-file = PuTTY-.reg-Exportdatei …
import-menu-winscp-file = WinSCP.ini-Datei …
import-menu-solar-file = Solar-PuTTY-.json-Export …
menu-open-terminal = Terminal öffnen
menu-open-files = Dateien öffnen (SFTP)
menu-edit = Bearbeiten …
menu-duplicate = Duplizieren
menu-copy-address = Adresse kopieren
menu-delete = Löschen …
menu-reconnect = Neu verbinden
menu-split-right = Rechts teilen
menu-split-down = Unten teilen
menu-send-break = Break senden
menu-follow-terminal = Terminal-Ordner folgen
menu-close = Schließen
menu-close-others = Andere schließen
menu-close-all = Alle schließen
tools-generate-key = Schlüssel erzeugen …
tools-inspect-key = Schlüssel prüfen oder konvertieren …
tools-known-hosts = Bekannte Hosts …
tools-saved-passwords = Gespeicherte Passwörter …
tools-settings-sync = Einstellungssynchronisierung …
button-continue = Weiter

## Connection prompts

notice-connection-title = Verbindung
hostkey-changed-title = Host-Schlüssel geändert!
hostkey-unknown-title = Unbekannter Host
hostkey-changed-body = WARNUNG: Der Host-Schlüssel von { $host } hat sich GEÄNDERT.

    Das kann bedeuten, dass jemand die Verbindung abfängt (Man-in-the-Middle), oder dass der Server neu installiert wurde.

    Neuer { $algorithm }-Schlüssel:
    { $fingerprint }

    Fahren Sie nur fort, wenn Sie wissen, dass der Schlüssel absichtlich geändert wurde.
hostkey-unknown-body = Dies ist die erste Verbindung zu { $host }.

    Fingerabdruck des { $algorithm }-Schlüssels:
    { $fingerprint }

    Prüfen Sie ihn mit der Administration des Servers, bevor Sie ihm vertrauen.
hostkey-reject = Ablehnen
hostkey-trust-once = Einmal vertrauen
hostkey-replace = Gespeicherten Schlüssel ersetzen
hostkey-trust-remember = Vertrauen und merken
remember-hint-unlocked = Wird verschlüsselt in Ihrem Passworttresor gespeichert.
remember-hint-locked = Sie werden nach dem Master-Passwort Ihres Tresors gefragt.
remember-hint-create = Sie legen einen Passworttresor mit einem Master-Passwort an.
prompt-server-auth-title = Server-Anmeldung
prompt-password-title = Passwort für { $user }@{ $host }
prompt-password-label = Passwort
prompt-passphrase-title = Passphrase für { $path }
prompt-passphrase-label = Passphrase
prompt-sudo-title = sudo-Passwort für { $who }
prompt-sudo-wrong-title = Falsches Passwort. sudo-Passwort für { $who }
secret-rejected-password = Das gespeicherte Passwort wurde nicht akzeptiert. Geben Sie das aktuelle ein.
secret-rejected-proxy = Das gespeicherte Proxy-Passwort wurde nicht akzeptiert. Geben Sie das aktuelle ein.
secret-rejected-passphrase = Die gespeicherte Passphrase wurde nicht akzeptiert. Geben Sie die aktuelle ein.
secret-rejected-sudo = sudo hat das gespeicherte Passwort nicht akzeptiert.
secret-update-password = Gespeichertes Passwort aktualisieren
secret-update-proxy = Gespeichertes Proxy-Passwort aktualisieren
secret-update-passphrase = Gespeicherte Passphrase aktualisieren
secret-update-sudo = Gespeichertes sudo-Passwort aktualisieren
secret-remember-password = Passwort speichern
secret-remember-proxy = Proxy-Passwort speichern
secret-remember-passphrase = Passphrase speichern
secret-remember-sudo = sudo-Passwort speichern
vault-reason-host = Für { $name } sind Passwörter gespeichert. Entsperren Sie sie mit Ihrem Master-Passwort.
vault-reason-store = Entsperren Sie Ihre gespeicherten Passwörter, um dieses zu speichern.
vault-reason-create = Legen Sie einen Tresor für gespeicherte Passwörter an. Ein Master-Passwort schützt sie; es wird nie gespeichert.
notice-not-saved-locked = Nicht gespeichert: Der Passworttresor ist gesperrt.
notice-save-password-failed = Das Passwort konnte nicht gespeichert werden: { $error }
notice-update-host-failed = { $name } konnte nicht aktualisiert werden: { $error }
files-need-ssh = Dateien benötigen SSH: { $name } ist eine { $protocol }-Verbindung, die keine Dateien übertragen kann.
files-root-failed = Konnte nicht als root öffnen: { $error }

## Settings sync

sync-no-changes = keine Änderungen hier
sync-added = { $count } hinzugefügt
sync-updated = { $count } aktualisiert
sync-removed = { $count } entfernt
sync-settings-updated = Einstellungen aktualisiert
sync-mode-sync = Synchronisieren
sync-mode-push = Hochladen
sync-mode-pull = Herunterladen
sync-summary = { $mode }: { $count ->
    [one] { $count } Host synchron
   *[other] { $count } Hosts synchron
}; { $changes }
sync-file-updated = Sync-Datei aktualisiert
vault-reason-sync-startup = Entsperren Sie Ihre gespeicherten Passwörter, um Einstellungen zu synchronisieren (die Sync-Passphrase liegt dort).
vault-reason-sync = Entsperren Sie Ihre gespeicherten Passwörter, um Einstellungen zu synchronisieren.
sync-no-folder = Wählen Sie zuerst einen Sync-Ordner unter Einstellungen → Einstellungssynchronisierung.
sync-no-passphrase = keine Sync-Passphrase gespeichert – legen Sie sie unter Einstellungen → Einstellungssynchronisierung fest
sync-passphrase-title = Passphrase für die Einstellungssynchronisierung
sync-passphrase-label = Sync-Passphrase
sync-passphrase-wrong-title = Falsche Sync-Passphrase – geben Sie die Ihrer anderen Computer ein
sync-wrong-passphrase = falsche Sync-Passphrase
sync-last-failed = fehlgeschlagen: { $error }
sync-done = Einstellungssynchronisierung – { $summary }
sync-failed = Einstellungssynchronisierung fehlgeschlagen: { $error }

## Password vault

cred-sync-passphrase = Passphrase der Einstellungssynchronisierung
cred-login-password = Anmeldepasswort
cred-sudo-password = sudo-Passwort
cred-proxy-password = Proxy-Passwort
cred-key-passphrase = Passphrase für { $file }
cred-key-unused = Schlüssel-Passphrase – { $path } (von keinem Host verwendet)
cred-host-deleted = Unbenutzt (Host wurde gelöscht)
strength-too-short = Zu kurz – mindestens 8 Zeichen verwenden
strength-strong-passphrase = Starke Passphrase
strength-weak = Schwach – länger ist besser (einige Wörter eignen sich gut)
strength-fair = Mittel
strength-strong = Stark
vault-locked-error = Der Tresor ist gesperrt
vault-current-wrong = Das aktuelle Master-Passwort ist falsch.
vault-current-master = Aktuelles Master-Passwort
vault-master = Master-Passwort
vault-new-master = Neues Master-Passwort
vault-repeat = Wiederholen
vault-enter-password = Geben Sie ein Passwort ein.
vault-too-weak = Mindestens 8 Zeichen verwenden – einige unabhängige Wörter eignen sich gut.
vault-mismatch = Die beiden Passwörter stimmen nicht überein.
vault-wrong-master = Falsches Master-Passwort.
vault-strength-empty = Mindestens 8 Zeichen; einige unabhängige Wörter eignen sich gut.
vault-never-stored = Das Master-Passwort wird nie gespeichert. Wenn Sie es vergessen, lassen sich gespeicherte Passwörter nicht wiederherstellen.
vault-unlocking = Wird entsperrt …
vault-encrypting = Wird verschlüsselt …
vault-path = Tresor: { $path } (gemeinsam mit dem Befehl hostwick)
vault-title-unlock = Gespeicherte Passwörter entsperren
vault-title-create = Passworttresor anlegen
vault-title-change = Master-Passwort ändern
vault-not-now = Nicht jetzt
vault-unlock = Entsperren
vault-create = Tresor anlegen
vault-change = Ändern

## Host editor

form-forward-needs-spec = `{ $token }` benötigt eine Portangabe
form-jump-invalid = Jump-Host `{ $host }`: { $error }
form-port-invalid = { $field } muss eine Zahl von 1 bis 65535 sein
form-baud-invalid = Geschwindigkeit (Baud) muss eine Zahl wie 9600 oder 115200 sein
form-bits-invalid = Datenbits müssen 5–8 und Stoppbits 1 oder 2 sein
host-section-general = Allgemein
host-section-auth = Anmeldung
host-section-connection = Verbindung
host-section-terminal = Terminal
host-section-files = Dateien
host-section-advanced = Erweitert
host-folder-placeholder = prod/web (optional)
host-host-placeholder = 10.0.0.5 oder host.example.com
host-user-placeholder = deploy (leer = Ihr lokaler Benutzername)
host-keepalive-placeholder = Sekunden, 0 = aus
host-optional-placeholder = optional
host-x11-placeholder-windows = leer = localhost:0 (VcXsrv, Xming)
host-x11-placeholder = leer = $DISPLAY
host-script-example = Beispiel:
host-snippets-empty = Keine Textbausteine für diesen Host. Globale Textbausteine finden Sie unter Einstellungen → Textbausteine.
host-log-placeholder = leer = aus, oder z. B. &S-&Y&M&D-&T.log
host-sftp-server-placeholder = leer = automatisch erkennen (/usr/lib/openssh/sftp-server, …)
host-error-no-device = Wählen Sie die serielle Schnittstelle (Gerät)
host-error-no-host = Geben Sie den Hostnamen oder die Adresse ein
host-error-no-auth = Wählen Sie mindestens eine Anmeldemethode
host-error-forwards = Portweiterleitungen: { $error }
host-error-keepalive = Keepalive muss in Sekunden angegeben werden (0 bis 3600)
host-error-proxy-host = Geben Sie den Hostnamen des Proxys ein
host-error-x11 = X11-Display: { $error }
host-error-script = Anmeldeskript, { $error }
host-saved-forget-pending = Wird beim Speichern vergessen: { $what }
host-saved-locked = Im Tresor gespeichert (gesperrt): { $what }
host-saved = Im Tresor gespeichert: { $what }
host-saved-missing = Nicht mehr im Tresor, Sie werden gefragt: { $what }
host-saved-keep = Behalten
host-saved-forget = Vergessen
host-what-password = Passwort
host-what-key-passphrase = Schlüssel-Passphrase
host-what-proxy-password = Proxy-Passwort
host-what-sudo-password = sudo-Passwort
host-pick-key = Privater Schlüssel (OpenSSH oder .ppk)
host-name = Name
host-folder = Ordner
host-protocol = Protokoll
host-host = Host
host-port = Port
host-user = Benutzer
host-notes = Notizen
host-telnet-hint = Telnet überträgt alles unverschlüsselt; verwenden Sie es für Netzwerkgeräte in einem vertrauenswürdigen Netz. Der Benutzername wird gesendet, wenn der Server danach fragt.
host-ports-looking = Suche …
host-ports-none = Keine Schnittstellen gefunden
host-ports-count = Schnittstellen ({ $count })
host-ports-none-menu = Keine seriellen Schnittstellen gefunden – Gerätenamen eingeben
host-device = Schnittstelle (Gerät)
host-ports-refresh = Erneut nach seriellen Schnittstellen suchen
host-device-hint-windows = COM3, COM4 … Wählen Sie eine aus oder geben Sie den Namen ein.
host-device-hint = /dev/ttyUSB0, /dev/ttyACM0, /dev/serial/by-id/… Wählen Sie eine aus oder geben Sie den Pfad ein.
host-baud = Geschwindigkeit (Baud)
host-data-bits = Datenbits
host-parity = Parität
host-parity-none = Keine
host-parity-odd = Ungerade
host-parity-even = Gerade
host-stop-bits = Stoppbits
host-flow = Flusssteuerung
host-flow-none = Keine
host-serial-none-linux = Keine Schnittstellen gefunden. USB-Adapter erscheinen als /dev/ttyUSB*; eventuell müssen Sie Mitglied der Gruppe dialout sein.
host-serial-hint = Die meisten Konsolen verwenden 9600 8N1 oder 115200 8N1 ohne Flusssteuerung.
host-auth-not-ssh = Telnet- und serielle Verbindungen melden sich im Terminal an. Mit einem Anmeldeskript (Terminal) lässt sich das automatisieren.
host-key-files = Schlüsseldateien
host-keys-hint = OpenSSH- oder PuTTY-.ppk-Schlüssel, durch Kommas getrennt. Ein Zertifikat neben einem Schlüssel (id_ed25519-cert.pub) wird automatisch verwendet. Sicherheitsschlüssel (sk-…) funktionieren über den Agenten.
host-auth-agent = Schlüssel im SSH-Agenten (Pageant, ssh-agent, 1Password …)
host-auth-password = Passwort
host-auth-remember-hint = Setzen Sie bei der Abfrage den Haken „Passwort speichern“, um sich künftig automatisch anzumelden.
host-auth-kbd = Tastatur-interaktiv (Einmalcodes, MFA)
host-auth-kerberos = Kerberos (Single Sign-on)
host-auth-kerberos-forward = Mein Kerberos-Ticket an den Server weiterleiten
host-auth-kerberos-hint-windows = Verwendet Ihre Windows-Domänenanmeldung.
host-auth-kerberos-hint = Verwendet das Ticket von kinit (siehe klist). Ohne Ticket wird die nächste Methode versucht.
host-auth-order = Methoden werden in dieser Reihenfolge versucht: Kerberos, Agent, Schlüsseldateien, Passwort, Tastatur-interaktiv.
host-connection-serial = Serielle Schnittstellen sind lokal: kein Proxy, keine Jump-Hosts oder Weiterleitungen.
host-proxy = Proxy
host-proxy-none = Keiner
host-proxy-host-port = Proxy-Host / -Port
host-proxy-port = Proxy-Port
host-proxy-user = Proxy-Benutzer
host-proxy-password-hint = Das Proxy-Passwort wird beim Verbinden abgefragt (zum Speichern „Speichern“ ankreuzen).
host-jump-hosts = Jump-Hosts
host-jump-hint = Durch Kommas getrennt, äußerster zuerst (wie ssh -J).
host-forwards = Portweiterleitungen
host-forwards-hint = L = lokal → entfernt, R = entfernt → lokal, D = SOCKS-Proxy. Werden mit dem Terminal gestartet.
host-keepalive = Keepalive (s)
host-agent-forwarding = Meinen SSH-Agenten weiterleiten (git auf dem Server nutzt meine Schlüssel)
host-agent-forwarding-hint = Nur bei vertrauenswürdigen Servern: Deren Administration kann Ihre Schlüssel nutzen, solange Sie verbunden sind.
host-x11 = X11 weiterleiten (entfernte grafische Programme hier anzeigen)
host-x11-display = X-Display
host-x11-hint-windows = Benötigt einen laufenden X-Server wie VcXsrv oder Xming.
host-x11-hint-macos = Benötigt XQuartz.
host-x11-hint = Verwendet den X-Server Ihres Desktops (oder XWayland).
host-terminal-type = Terminaltyp
host-login-script = Anmeldeskript
host-login-script-hint = Ein Befehl pro Zeile, nach der Anmeldung eingegeben. @wait TEXT wartet auf Ausgabe (10 s), @wait 30000 TEXT länger, @sleep 500 pausiert; # leitet einen Kommentar ein.
host-shell-integration = Shell-Integration (bash, zsh)
host-shell-integration-hint = Damit kann der Dateibrowser dem Ordner des Terminals folgen (Rechtsklick → Dateien hier öffnen). Fügt beim Start der Shell unauffällig einen Prompt-Hook hinzu; auf dem Server wird nichts installiert.
host-snippets-title = Textbausteine für diesen Host
host-session-log = Sitzungsprotokoll
host-session-log-hint = Platzhalter: &S Sitzung, &H Host, &P Port, &U Benutzer, &Y&M&D Datum, &T Uhrzeit. Relative Pfade landen im Protokollordner.
host-raw-log = Rohausgabe protokollieren (mit Escape-Sequenzen)
host-files-not-ssh = Der Dateibrowser benötigt SSH.
host-file-protocol = Dateiprotokoll
host-file-protocol-auto = Automatisch
host-file-protocol-hint = „Automatisch“ verwendet SFTP und weicht auf Servern ohne SFTP (Router, ESXi, busybox) auf SCP aus.
host-sudo = Dateibrowser als root öffnen (sudo)
host-sudo-user = Als Benutzer
host-sudo-hint = Funktioniert mit sudo ohne Passwort oder mit sudo-Passwort (abgefragt oder gespeichert). „Als root neu öffnen“ im Browser wechselt jederzeit.
host-legacy = Veraltete Algorithmen erlauben (ssh-rsa/SHA-1, alter Schlüsselaustausch) für alte Geräte
host-legacy-hint = Nur für alte Switches, Router und Appliances, die sie benötigen.
host-id = Host-ID: { $id }
host-title-edit = Host bearbeiten
host-title-new = Neuer Host
loading = Wird geladen …

## Settings

settings-title = Einstellungen
settings-section-appearance = Darstellung
settings-section-terminal = Terminal
settings-section-snippets = Textbausteine
settings-section-connections = Verbindungen
settings-section-files = Dateien
settings-section-logging = Sitzungsprotokolle
settings-section-passwords = Gespeicherte Passwörter
settings-section-sync = Einstellungen synchronisieren
settings-section-diagnostics = Diagnose
settings-language = Sprache
settings-language-system = System ({ $language })
settings-language-hint = Gilt sofort nach dem Speichern. Bereits geöffnete Dialoge behalten ihre Sprache, bis sie erneut geöffnet werden.
settings-theme = Design
settings-theme-dark = Dunkel
settings-theme-light = Hell
settings-terminal-font = Terminal-Schriftart
settings-font-size = Schriftgröße
settings-font-placeholder = leer = System-Monospace
settings-font-hint = Schriftänderungen gelten beim Speichern auch für offene Terminals.
settings-editor-placeholder = leer = Standard-App des Systems (z. B. code -w { "{file}" })
settings-auto-lock-placeholder = 0 = entsperrt lassen, bis Hostwick beendet wird
settings-sync-folder-placeholder = OneDrive-, Dropbox-, iCloud-Drive- oder git-Ordner
settings-sync-pass-saved = Im Tresor gespeichert – zum Ändern eingeben
settings-sync-pass-placeholder = Gleiche Passphrase auf jedem Computer
settings-snippets-empty = Noch keine Textbausteine. Sie erscheinen als Schaltflächen über jedem Terminal und unter Rechtsklick → Textbausteine.
settings-remove-failed = Konnte nicht entfernt werden: { $error }
settings-error-font-size = Terminal → Schriftgröße muss eine Zahl von 6 bis 48 sein
settings-error-scrollback = Terminal → Verlauf muss eine ganze Zahl von 100 bis 1000000 sein
settings-error-auto-lock = Gespeicherte Passwörter → Sperren nach muss in Minuten angegeben werden (0 bis 1440)
settings-error-rules = Terminal → { $error }
settings-error-sync-folder = Einstellungssynchronisierung → Der Ordner { $folder } existiert nicht
settings-folder-missing = Der Ordner { $folder } existiert nicht.
settings-sync-pass-short = Verwenden Sie eine Sync-Passphrase mit mindestens 8 Zeichen.
settings-sync-pass-save-failed = Die Passphrase konnte nicht gespeichert werden: { $error }
vault-reason-sync-passphrase = Die Sync-Passphrase wird in Ihrem Passworttresor aufbewahrt.
settings-save-failed = Einstellungen können nicht gespeichert werden: { $error }
settings-scrollback = Verlaufszeilen
settings-right-click = Rechtsklick
settings-right-click-menu = Menü anzeigen
settings-right-click-paste = Einfügen (PuTTY)
settings-copy-on-select = Markierten Text automatisch kopieren
settings-mouse = Mausklicks und Scrollen an Programme senden (htop, mc, vim)
settings-mouse-hint = Halten Sie die Umschalttaste gedrückt, um Text zu markieren, während ein Programm die Maus verwendet.
settings-button-bar = Leiste mit Textbausteinen über Terminals anzeigen
settings-highlighting = Schlüsselwort-Hervorhebung
settings-highlighting-hint = Färbt passenden Text auf dem Bildschirm. Frühere Regeln haben bei Überschneidungen Vorrang.
settings-snippets-hint = In jedem Terminal verfügbar. Hosts können unter Host bearbeiten → Terminal eigene hinzufügen.
settings-auto-reconnect = Automatisch neu verbinden, wenn eine Verbindung abbricht
settings-auto-reconnect-hint = SSH und Telnet: nach 1, 2, 5, 10, dann alle 30 Sekunden; der Verlauf bleibt erhalten. Nicht nach „exit“, einem Anmelde- oder Host-Schlüssel-Fehler.
settings-restore-tabs = Meine Tabs beim Start von Hostwick wieder öffnen
settings-restore-tabs-hint = Einschließlich geteilter Bereiche und ihrer Größen. Tabs verbinden sich, wenn Sie zum ersten Mal zu ihnen wechseln.
settings-editor = Externer Editor
settings-editor-pick = Editor-Programm
settings-editor-hint = Wird von „Bearbeiten“ (F4) verwendet. { "{file}" } wird durch den Dateipfad ersetzt, sonst wird der Pfad angehängt. Nutzen Sie die Warte-Option des Editors (code -w, subl -w), falls vorhanden.
settings-log-all = Jede Sitzung in eine Datei protokollieren
settings-log-folder = Protokollordner
settings-log-content = Protokollinhalt
settings-log-printable = Druckbarer Text
settings-log-raw = Roh (mit Escape-Sequenzen)
settings-log-hint = Die eigene Protokolleinstellung eines Hosts (Host bearbeiten → Terminal) hat Vorrang.
settings-log-level = App-Protokollstufe
settings-open-log-folder = Protokollordner öffnen
settings-data-folder = Datenordner: { $path }
settings-vault-none = Noch kein Tresor. Setzen Sie den Haken „Speichern“, wenn Hostwick nach einem Passwort fragt, oder legen Sie jetzt einen an.
settings-vault-locked = Gesperrt. Gespeicherte Passwörter werden nach dem Entsperren verwendet.
settings-vault-unlocked = Für diese Sitzung entsperrt.
settings-vault-unlock = Entsperren …
settings-vault-create = Tresor anlegen …
settings-vault-lock = Jetzt sperren
settings-vault-change = Master-Passwort ändern …
settings-vault-empty = Noch nichts gespeichert.
settings-vault-remove = Entfernen
settings-auto-lock = Sperren nach (min)
settings-auto-lock-hint = Minuten ohne Aktivität, bis gespeicherte Passwörter wieder gesperrt werden; 0 lässt sie entsperrt, bis Hostwick beendet wird.
settings-vault-file = Mit Argon2id + XChaCha20-Poly1305 verschlüsselt in { $path } – derselbe Tresor wie `hostwick vault`. Wird nie synchronisiert.
settings-sync-never = Auf diesem Computer noch nie synchronisiert.
settings-sync-intro = Hosts und Einstellungen landen in einer verschlüsselten Datei in einem Ordner, den Ihr Cloud-Speicher oder git abgleicht. Gespeicherte Passwörter verlassen diesen Computer nie.
settings-sync-folder = Ordner
settings-sync-folder-pick = Sync-Ordner
settings-sync-passphrase = Passphrase
settings-sync-passphrase-hint = Wird in Ihrem Passworttresor aufbewahrt. Jeder Computer braucht dieselbe.
settings-sync-auto = Beim Start, nach Änderungen und alle 5 Minuten synchronisieren
settings-sync-now = Jetzt synchronisieren
settings-sync-push = Sync-Datei ersetzen
settings-sync-push-tooltip = Sync-Datei an diesen Computer angleichen (ändert auch ihre Passphrase)
settings-sync-pull = Diesen Computer ersetzen
settings-sync-pull-tooltip = Hosts und Einstellungen dieses Computers an die Sync-Datei angleichen
settings-sync-last = Letzte Synchronisierung

## Snippet and highlight-rule editors

snippet-label-placeholder = Beschriftung
snippet-text-placeholder = Zu sendender Text (mehrere Zeilen werden als ein Block eingefügt)
snippet-press-enter = Eingabetaste drücken
snippet-move-up = Nach oben
snippet-move-down = Nach unten
editor-remove = Entfernen
snippet-add = Textbaustein hinzufügen
rule-error-empty = leeres Muster
rule-error-color = Farbe muss wie #ff8800 aussehen, nicht `{ $color }`
rule-error-regex = ungültiger regulärer Ausdruck
rule-pattern-placeholder = Wort oder regulärer Ausdruck
rule-error = Hervorhebung `{ $pattern }`: { $error }
rule-enabled = Aktiviert
rule-regex-on = Regulärer Ausdruck (klicken für ein einfaches Wort)
rule-regex-off = Einfaches Wort, Groß-/Kleinschreibung egal (klicken für einen regulären Ausdruck)
rule-add = Regel hinzufügen
rule-defaults = Standard wiederherstellen

## Terminal

script-timeout = { $after } auf „{ $text }“ gewartet, es erschien nie
script-session-ended = die Sitzung endete vorher
search-too-complex = Muster zu komplex
term-exit-status = beendet mit Status { $code }
term-exit-signal = durch Signal { $signal } beendet
term-connection-lost = Verbindung verloren
term-connection-closed = Verbindung geschlossen
term-reconnected-marker = neu verbunden { $time }
term-reconnect-failed = Neuverbindung fehlgeschlagen: { $error }
term-break-failed = Break konnte nicht gesendet werden
term-no-break = { $protocol }-Sitzungen haben kein Break-Signal
term-script-not-run = Anmeldeskript nicht ausgeführt: { $error }
term-script-stopped = Skript angehalten: { $error }
search-placeholder = Im Verlauf suchen
search-no-results = Keine Treffer
search-position = { $index } von { $total }
search-matches = { $count ->
    [one] { $count } Treffer
   *[other] { $count } Treffer
}
search-match-case = Groß-/Kleinschreibung beachten
search-regex = Regulärer Ausdruck
search-older = Vorheriger (älterer) Treffer – Eingabe, F3
search-newer = Nächster (neuerer) Treffer – Umschalt+Eingabe, Umschalt+F3
search-close = Schließen (Esc)
term-reconnect-in = Verbindung verloren{ $reason }. Neuverbindung in { $seconds } s …
term-reconnect-attempt = Verbinde neu (Versuch { $attempt }) …
term-log-badge = PROT
term-reconnect-now = Jetzt
term-session-ended = Sitzung beendet – { $reason }. Eingabetaste drücken, um neu zu verbinden.
term-menu-copy = Kopieren
term-menu-paste = Einfügen
term-menu-select-all = Alles markieren
term-menu-find = Suchen …
term-menu-snippets = Textbausteine
term-menu-copy-folder = Aktuellen Ordner kopieren
term-menu-stop-script = Skript anhalten
term-menu-clear = Verlauf löschen
term-menu-stop-log = Sitzungsprotokoll beenden
term-menu-start-log = Sitzung in Datei protokollieren …
term-menu-files-here = Dateien hier öffnen
term-menu-duplicate = Sitzung duplizieren
term-menu-reconnect-ended = Neu verbinden (Sitzung beendet)

## File browser

files-move = Verschieben
files-upload = Hochladen
files-download = Herunterladen
files-move-confirm = { $count ->
    [one] „{ $name }“ nach { $dir } verschieben?
   *[other] { $count } Elemente nach { $dir } verschieben?
}
files-upload-confirm = { $count ->
    [one] „{ $name }“ nach { $dir } hochladen?
   *[other] { $count } Elemente nach { $dir } hochladen?
}
files-download-confirm = { $count ->
    [one] „{ $name }“ nach { $dir } herunterladen?
   *[other] { $count } Elemente nach { $dir } herunterladen?
}
files-same-place = { $name }: Quelle und Ziel sind identisch
files-skipped-unsafe = Nicht angezeigt, weil ihre Namen auf diesem Computer unsicher sind: { $names }
files-not-removed = { $name }: kopiert, aber nicht entfernt: { $error }
files-pick-download = Hierher herunterladen
files-exists = „{ $name }“ existiert bereits
files-delete-title = Löschen
files-delete-folder-confirm = Den Ordner „{ $name }“ samt Inhalt endgültig löschen?
files-delete-file-confirm = Die Datei „{ $name }“ endgültig löschen?
files-delete-many-confirm = { $count } Elemente endgültig löschen (Ordner samt Inhalt)?
files-name-invalid = Geben Sie einen Namen ohne „/“ ein
files-new-folder = Neuer Ordner
files-create = Erstellen
files-folder-name = Ordnername
files-rename = Umbenennen
files-new-name = Neuer Name
files-new-symlink = Neuer symbolischer Link
files-symlink-placeholder = Ziel -> Linkname
files-symlink-format = Schreiben Sie es als „Ziel -> Linkname“
files-symlink-empty = Das Ziel ist leer
edit-conflict-title = Auf dem Server geändert
edit-conflict-body = „{ $name }“ wurde auf dem Server geändert, seit Sie die Datei geöffnet haben. Was soll mit Ihrer gespeicherten Fassung geschehen?
edit-conflict-later = Später entscheiden
edit-conflict-theirs = Server-Fassung herunterladen
edit-conflict-copy = Als Kopie speichern
edit-conflict-overwrite = Überschreiben
edit-editor-failed = Der Editor „{ $program }“ konnte nicht gestartet werden: { $error }
keep-up-to-date = aktuell
keep-synced = { $count ->
    [one] { $count } Datei synchronisiert, { $size }
   *[other] { $count } Dateien synchronisiert, { $size }
}
keep-failed = { $count } fehlgeschlagen
keep-error = Fehler (wird wiederholt): { $error }
keep-starting = startet …
keep-status = { $local } → { $remote } wird aktuell gehalten: { $status }
edits = Bearbeitungen
edits-busy = Bearbeitungen ({ $count }): { $status }
edits-tooltip = In einem Editor geöffnete Dateien; Speichern lädt automatisch hoch
edits-stop = Nicht mehr überwachen
edits-stop-all = Keine mehr überwachen
files-hide-local = Lokal ausblenden
files-show-local = Lokal anzeigen
files-local-tooltip = Dateien des lokalen Computers ein- oder ausblenden
files-upload-pick = Hochladen …
files-synchronize = Synchronisieren …
files-synchronize-tooltip = Ordner vergleichen, Änderungen prüfen und dann ausführen
files-keep-stop = Aktualhalten beenden
files-keep = Aktuell halten …
files-keep-tooltip = Änderungen aus einem lokalen Ordner automatisch in einen entfernten Ordner hochladen
files-following = Folgt dem Terminal
files-follow = Terminal folgen
files-follow-tooltip = Den Ordner anzeigen, in dem sich das Terminal „{ $terminal }“ gerade befindet
files-root-switching = Wechsle …
files-root-on = Root: an
files-root-reopen = Als root neu öffnen
files-root-tooltip = Den Server als root (oder als sudo-Benutzer des Hosts) über sudo durchsuchen
files-path-placeholder = /pfad
files-this-computer = Dieser Computer
files-selected-of = { $selected } von { $count } ausgewählt ({ $size })
files-item-count = { $count ->
    [one] { $count } Element
   *[other] { $count } Elemente
}
files-selected = { $count ->
    [one] { $count } Element
   *[other] { $count } Elemente
}
files-root-badge = ROOT
files-up-tooltip = Nach oben (Rücktaste)
files-refresh-tooltip = Aktualisieren (Strg+R)
files-mkdir-tooltip = Neuer Ordner (F7)
files-col-name = Name
files-col-size = Größe
files-col-modified = Geändert
files-col-mode = Rechte
files-download-pick = Herunterladen …
files-menu-open = Öffnen
files-menu-edit = Bearbeiten
files-menu-move = Auf die andere Seite verschieben
files-menu-rename = Umbenennen …
files-menu-new-folder = Neuer Ordner …
files-menu-new-symlink = Neuer symbolischer Link …
files-menu-copy-path = Pfad kopieren
files-menu-refresh = Aktualisieren
files-menu-properties = Eigenschaften
edit-status-downloading = wird heruntergeladen …
edit-status-watching = wird überwacht
edit-status-uploading = wird hochgeladen …
edit-status-uploaded = hochgeladen { $time }
edit-status-uploaded-in-place = hochgeladen { $time } (direkt überschrieben)
edit-status-conflict = auf dem Server geändert – wartet auf Sie
edit-status-error = Fehler: { $error }
edit-gone = { $path }: existiert nicht mehr
edit-kept-folder = Bearbeitete Dateien, die nicht hochgeladen wurden, liegen in { $path }
edit-is-folder = { $path } ist ein Ordner
edit-missing-after-upload = { $path }: fehlt nach dem Hochladen
edit-size-mismatch = { $path }: Server hat { $server } Bytes nach dem Hochladen von { $uploaded }
edit-no-copy-name = kein freier Name für die Kopie

## File properties

props-title = Eigenschaften
props-applying = Wird angewendet …
props-apply = Anwenden
props-calculating = wird berechnet …
props-error = Fehler: { $error }
props-usage = { $size } ({ $bytes } Bytes) in { $files ->
    [one] { $files } Datei
   *[other] { $files } Dateien
}, { $folders ->
    [one] { $folders } Ordner
   *[other] { $folders } Ordnern
}
props-usage-partial = (vorzeitig beendet: zu viele Einträge)
props-bad-id = „{ $id }“ ist keine numerische ID
props-bad-octal = Rechte müssen oktal angegeben werden, z. B. 0644
props-kind-folder = Ordner
props-kind-file = Datei
props-kind-symlink = Symbolischer Link → { $target }
props-kind-special = Spezialdatei
props-unknown-name = (Name unbekannt)
props-not-reported = vom Server nicht gemeldet
props-read = Lesen
props-write = Schreiben
props-execute = Ausführen
props-owner = Eigentümer
props-group = Gruppe
props-others = Andere
props-special = Spezial
props-octal = Oktal
props-calculate = Berechnen
props-size = { $size } ({ $bytes } Bytes)
props-location = Speicherort
props-type = Typ
props-checksum = Prüfsumme
props-unknown = unbekannt
props-permissions = Rechte
props-recursive = Auf den gesamten Inhalt anwenden (Dateien behalten ihre Ausführungsbits)

## Synchronize dialog

files-synchronize-title = Synchronisieren
sync-keep = Entfernt aktuell halten
sync-keep-tooltip = Den lokalen Ordner überwachen und jede Änderung hochladen (nur lokal → entfernt)
sync-comparing = Vergleiche …
sync-preview = Vorschau
sync-run-checked = Ausgewählte synchronisieren
sync-exclude-placeholder = z. B. .git, *.tmp, node_modules
sync-choose-folders = Wählen Sie beide Ordner
sync-use-local = Lokal verwenden
sync-use-remote = Entfernt verwenden
sync-total-uploads = { $count ->
    [one] { $count } Upload, { $size }
   *[other] { $count } Uploads, { $size }
}
sync-total-downloads = { $count ->
    [one] { $count } Download, { $size }
   *[other] { $count } Downloads, { $size }
}
sync-total-deletions = { $count ->
    [one] { $count } Löschung
   *[other] { $count } Löschungen
}
sync-total-conflicts = { $count ->
    [one] { $count } Konflikt – Seite wählen oder so lassen
   *[other] { $count } Konflikte – Seite wählen oder so lassen
}
sync-in-sync = Beide Ordner sind bereits synchron.
sync-checked = Ausgewählt: { $items }
sync-local-folder = Lokaler Ordner
sync-remote-folder = Entfernter Ordner
sync-direction = Richtung
sync-dir-up = Lokal → entfernt
sync-dir-down = Entfernt → lokal
sync-dir-both = Beide (neuere gewinnt)
sync-mirror = In der Quelle fehlende Dateien löschen (spiegeln)
sync-exclude = Ausschließen
sync-comparing-folders = Ordner werden verglichen …
sync-result = Synchronisieren: { $summary }
sync-running = Wird synchronisiert …
sync-run-failed = Synchronisieren fehlgeschlagen: { $error }
sync-kind-upload = Hochladen
sync-kind-download = Herunterladen
sync-kind-delete-local = Lokal löschen
sync-kind-delete-remote = Entfernt löschen
sync-kind-conflict = Konflikt
sync-kind-skip = Überspringen
sync-reason-new = neu
sync-reason-newer = neuer
sync-reason-size = Größe unterscheidet sich
sync-reason-differs = unterschiedlich
sync-reason-not-in-source = nicht in der Quelle
sync-reason-both-changed = auf beiden Seiten geändert (gleiche Zeit, andere Größe)
sync-reason-unknown = nicht vergleichbar (kein Zeitstempel)
sync-reason-destination-newer = Ziel ist neuer
sync-reason-new-folder = neuer Ordner
sync-reason-case-conflict = eine andere Datei unterscheidet sich nur in Groß-/Kleinschreibung
sync-review-deletions = Diese Synchronisierung löscht Dateien. Prüfen Sie die Liste unten und klicken Sie dann erneut auf Synchronisieren.
sync-report-uploaded = { $count } hochgeladen
sync-report-downloaded = { $count } heruntergeladen
sync-report-deleted = { $count } gelöscht
sync-report-folders = { $count ->
    [one] { $count } Ordner erstellt
   *[other] { $count } Ordner erstellt
}
sync-report-failed = { $count } fehlgeschlagen
sync-report-nothing = nichts zu tun

## Key tools

keyformat-openssh-hint = Für OpenSSH, Hostwick, Git und die meisten Werkzeuge (eine .pub-Datei wird ebenfalls geschrieben).
keyformat-ppk3-hint = Für PuTTY 0.75+ und WinSCP (Argon2-geschützt).
keyformat-ppk2-hint = Für ältere Versionen von PuTTY, WinSCP und FileZilla.
key-fingerprint = Fingerabdruck: { $fingerprint }
keygen-title = Schlüssel erzeugen
keygen-generate = Erzeugen
keygen-passphrase-placeholder = Passphrase (empfohlen; leer = keine)
keygen-repeat = Wiederholen
keygen-mismatch = Die Passphrasen stimmen nicht überein.
keygen-no-path = Wählen Sie, wo der Schlüssel gespeichert werden soll.
keygen-exists = { $path } existiert bereits. Wählen Sie einen anderen Namen oder setzen Sie den Haken „Überschreiben“.
keygen-used-in-host = { $name } meldet sich jetzt mit { $path } an. Fügen Sie den öffentlichen Schlüssel in ~/.ssh/authorized_keys auf dem Server ein.
keygen-ed25519-recommended = Ed25519 (empfohlen)
keygen-saved = { $file } (privater Schlüssel) und { $file }.pub in { $dir } gespeichert
keygen-public-key = Öffentlicher Schlüssel (für ~/.ssh/authorized_keys):
keygen-copy-public = Öffentlichen Schlüssel kopieren
keygen-use-in-host = In Host verwenden …
keygen-type = Typ
keygen-comment = Kommentar
keygen-format = Format
keygen-save-as = Speichern unter
keygen-overwrite = Vorhandene Datei überschreiben
keygen-generating-rsa = Wird erzeugt … RSA 4096 kann einige Sekunden dauern.
keygen-generating = Wird erzeugt …
keyconv-title = Schlüssel prüfen oder konvertieren
keyconv-path-placeholder = ~/.ssh/id_ed25519 oder C:\keys\work.ppk
keyconv-current-placeholder = Aktuelle Passphrase (falls vorhanden)
keyconv-new-placeholder = Neue Passphrase (leer = keine)
keyconv-repeat-placeholder = Neue Passphrase wiederholen
keyconv-output = Ausgabedatei
keyconv-mismatch = Die neuen Passphrasen stimmen nicht überein.
keyconv-pass-changed = Passphrase geändert ({ $format }).
keyconv-pass-removed = Passphrase entfernt ({ $format }).
keyconv-wrote = { $path } geschrieben ({ $format }).
keyconv-needs-passphrase = Dieser Schlüssel hat eine Passphrase: Geben Sie sie unter „Aktuelle Passphrase“ ein.
keyconv-wrong-passphrase = Falsche Passphrase.
keyconv-exists = { $path } existiert bereits; wählen Sie eine andere Ausgabedatei.
keyconv-key-file = Schlüsseldatei
keyconv-type-bits = { $kind } { $bits } Bit ({ $algorithm })
keyconv-protected = Geschützt
keyconv-public-only = nur öffentlicher Schlüssel
keyconv-encrypted = ja, mit Passphrase
keyconv-no-passphrase = keine Passphrase
keyconv-fingerprint = Fingerabdruck
keyconv-inspect = Prüfen
keyconv-current = Aktuell
keyconv-new = Neue Passphrase
keyconv-change = Passphrase ändern
keyconv-convert-to = Konvertieren in
keyconv-convert = Konvertieren
keyconv-keeps-passphrase = Behält die aktuelle Passphrase, sofern Sie keine neue eingeben.
keyconv-working = Wird bearbeitet …
kh-title = Bekannte Hosts
kh-search-placeholder = Host, Schlüsseltyp oder Fingerabdruck suchen
kh-forget-placeholder = Host oder Host:Port
kh-nothing-matched = Nichts gefunden; die Datei ist unverändert.
kh-removed = { $count ->
    [one] { $count } Schlüssel entfernt.
   *[other] { $count } Schlüssel entfernt.
}
kh-type-host = Geben Sie einen Hostnamen oder Host:Port ein.
kh-col-line = Zeile
kh-col-hosts = Host(s)
kh-empty = Noch keine bekannten Hosts. Schlüssel werden hinzugefügt, wenn Sie einem Server vertrauen.
kh-no-match = Nichts passt zur Suche.
kh-reload = Neu laden
kh-remove-selected = Ausgewählte entfernen ({ $count })
kh-forget-host = Host vergessen
kh-forget-button = Alle seine Schlüssel entfernen
kh-footer = { $path } – { $shown } von { $total } angezeigt. Gehashte Namen werden berücksichtigt, wenn Sie einen Host über seinen Namen vergessen.
files-opening-root = Dateien werden als root geöffnet
files-opening = Dateibrowser wird geöffnet

## Keyboard and screen readers

sidebar-vault-lock = Gespeicherte Passwörter sperren
sidebar-quick-connect = Schnellverbindung (user@host:port, Eingabe)
sidebar-hosts-label = Gespeicherte Hosts
sidebar-hosts-keys = Pfeiltasten wählen einen Host. Eingabe öffnet ein Terminal, Umschalt+Eingabe die Dateien, F2 bearbeitet, Entf löscht, Umschalt+F10 öffnet das Menü.
tabs-label = Offene Sitzungen
tabs-keys = Links und Rechts wechseln den Tab, Eingabe geht in den Tab, Entf schließt ihn, Umschalt+F10 öffnet sein Menü. Strg+F6 wechselt zwischen Hostliste, Tabs und Sitzung.
tab-state-not-connected = noch nicht verbunden
tab-state-connected = verbunden
tab-state-disconnected = getrennt
tab-spoken = { $title }, { $state }, Tab { $index } von { $count }
tab-close = { $title } schließen
pane-divider = Bereichsteiler

## Proxy test

vault-reason-proxy-test = Entsperren Sie Ihre gespeicherten Passwörter, um das gespeicherte Proxy-Passwort zu verwenden.
proxy-test-closed = Der Proxy hat verbunden, aber { $target } hat die Verbindung sofort geschlossen.
proxy-test-ok-ssh = Über den Proxy mit { $target } verbunden. Der Server meldet: { $server }
proxy-test-ok = Über den Proxy mit { $target } verbunden (der Server ist kein SSH-Server).
proxy-test-ok-quiet = Über den Proxy mit { $target } verbunden (der Server hat noch nichts gesendet).
proxy-test-read-failed = Über den Proxy mit { $target } verbunden, dann: { $error }
host-proxy-test = Verbindung testen
host-proxy-testing = Wird getestet …
host-proxy-test-tooltip = Über den Proxy mit dem Port dieses Hosts (bzw. des ersten Jump-Hosts) verbinden und das Ergebnis melden
host-baud-common = Übliche Geschwindigkeiten
dismiss = Schließen
term-no-output = Noch keine Ausgabe.
term-text-view-title = Terminalausgabe: { $title }
term-text-view-hint = { $lines ->
    [one] { $lines } Zeile. Mit den Pfeiltasten lesen; Esc kehrt zum Terminal zurück.
   *[other] { $lines } Zeilen. Mit den Pfeiltasten lesen; Esc kehrt zum Terminal zurück.
}
term-a11y-label = Terminal: { $title }
term-a11y-keys = Neue Ausgabe wird vorgelesen, sobald sie zur Ruhe kommt. { $read } liest die letzte Ausgabe erneut, { $view } zeigt die gesamte Ausgabe als Text. Strg+F6 verlässt das Terminal.
term-menu-read-output = Letzte Ausgabe vorlesen
term-menu-text-view = Ausgabe als Text anzeigen …
files-path-label = Ordner: { $side }
files-list-label = { $side }: { $dir }
files-list-keys = Pfeiltasten bewegen, Umschalt+Pfeile erweitern die Auswahl, Leertaste wählt eine Zeile aus oder ab, Eingabe öffnet, Rücktaste geht nach oben, F5 kopiert auf die andere Seite, F6 verschiebt, F2 benennt um, Entf löscht, Alt+Eingabe zeigt Eigenschaften, Tab wechselt die Seite, Umschalt+F10 öffnet das Menü.
files-kind-link = Link
a11y-sections = Bereiche
a11y-section-problem = { $section } (prüfen)
kh-row-label = Zeile { $line }: { $hosts }
rule-color = Farbe (#rrggbb)
toast-info = Information
toast-success = Erledigt
toast-warning = Warnung
toast-error = Fehler
toast-close = Benachrichtigung schließen
