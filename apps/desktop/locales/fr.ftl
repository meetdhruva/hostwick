# Hostwick desktop : français.
#
# Traduction de en.ftl (source). Les clés et les variables ({ $name }) ne
# changent pas ; les noms de produits et de protocoles (SSH, SFTP, SCP, sudo,
# Kerberos, PuTTY, WinSCP, X11, SOCKS) ne se traduisent pas.

## General

error-internal = erreur interne : { $error }

## Main window

sidebar-search-placeholder = Rechercher des hôtes
split-only-terminals = Seuls les onglets de terminal peuvent être divisés.
files-follow-no-folder = Le terminal n’a pas encore indiqué son dossier. Activez « Intégration du shell » dans Modifier l’hôte → Terminal pour que l’explorateur de fichiers puisse le suivre (bash et zsh).
host-copy-name = { $name } (copie)
size-b = o
size-kib = Kio
size-mib = Mio
size-gib = Gio
size-tib = Tio
protocol-ssh = SSH
protocol-telnet = Telnet
protocol-serial = Série
sidebar-vault-unlocked-tooltip = Les mots de passe enregistrés sont déverrouillés — cliquez pour verrouiller
sidebar-settings-tooltip = Paramètres ({ $keys })
sidebar-open-terminal = Terminal
sidebar-open-files = Fichiers
sidebar-edit-host = Modifier l’hôte
sidebar-delete-host = Supprimer l’hôte
sidebar-new-host = Nouvel hôte…
tab-files-title = Fichiers : { $name }
broadcast = Diffusion
broadcast-on = Diffusion : ACTIVE
broadcast-tooltip = Envoyer la saisie à tous les terminaux ouverts ({ $keys })
shortcut-new-host = nouvel hôte
shortcut-duplicate-tab = dupliquer l’onglet
shortcut-close-pane = fermer le volet/l’onglet
shortcut-reconnect = reconnecter
shortcut-next-tab = onglet suivant
shortcut-split-right = diviser à droite
shortcut-split-down = diviser en bas
shortcut-move-between-panes = passer d’un volet à l’autre
shortcut-broadcast = diffusion
shortcut-find = rechercher
shortcut-settings = paramètres
empty-title = Aucune session ouverte
empty-hint-connect = Double-cliquez sur un hôte ou saisissez user@host dans le champ de connexion rapide.
empty-hint-menus = Clic droit sur les hôtes, onglets, fichiers et terminaux pour plus d’actions.
empty-hint-import = Importez depuis ~/.ssh/config, PuTTY, WinSCP ou Solar-PuTTY avec Importer… (en bas à gauche).
pane-connecting = Connexion à { $host }…
pane-retry = Réessayer
transfer-progress = { $done } sur { $total } — { $rate }/s
transfer-done = terminé — { $size }
transfer-skipped = ignoré
transfer-cancelled = annulé
transfer-failed = échec : { $error }
transfer-cancel = Annuler
transfers-title = { $count ->
    [one] Transferts — { $count } en cours
   *[other] Transferts — { $count } en cours
}
transfers-clear = Effacer les terminés

## Common buttons

button-cancel = Annuler
button-delete = Supprimer
button-ok = OK
button-save = Enregistrer
button-close = Fermer
button-browse = Parcourir…

## Menus, import

host-delete-title = Supprimer l’hôte
host-delete-confirm = Supprimer l’hôte enregistré « { $name } » ?
sidebar-import = Importer…
sidebar-tools = Outils
import-no-home = aucun dossier personnel
import-pick-putty = Export .reg de PuTTY
import-pick-solar = Export Solar-PuTTY (.json)
import-pick-winscp = WinSCP.ini
import-failed = Échec de l’importation : { $error }
import-summary = { $added } ajouté(s), { $updated } mis à jour, { $notes } avec remarques
import-done = Importé : { $summary }
import-report-title = Rapport d’importation
import-report-attention = À vérifier ({ $count })
import-report-added = Ajoutés ({ $count })
import-report-updated = Mis à jour ({ $count })
import-report-note = Les hôtes listés sous « À vérifier » ont tout de même été importés, sauf si la remarque indique qu’ils ont été ignorés. Clic droit sur un hôte → Modifier… pour l’ajuster.
import-menu-ssh-config = OpenSSH ~/.ssh/config
import-menu-putty-registry = PuTTY (sessions enregistrées sur ce PC)
import-menu-winscp-registry = WinSCP (sites enregistrés sur ce PC)
import-menu-putty-file = Fichier d’export .reg de PuTTY…
import-menu-winscp-file = Fichier WinSCP.ini…
import-menu-solar-file = Export .json de Solar-PuTTY…
menu-open-terminal = Ouvrir un terminal
menu-open-files = Ouvrir les fichiers (SFTP)
menu-edit = Modifier…
menu-duplicate = Dupliquer
menu-copy-address = Copier l’adresse
menu-delete = Supprimer…
menu-reconnect = Reconnecter
menu-split-right = Diviser à droite
menu-split-down = Diviser en bas
menu-send-break = Envoyer un break
menu-follow-terminal = Suivre le dossier du terminal
menu-close = Fermer
menu-close-others = Fermer les autres
menu-close-all = Tout fermer
tools-generate-key = Générer une clé…
tools-inspect-key = Examiner ou convertir une clé…
tools-known-hosts = Hôtes connus…
tools-saved-passwords = Mots de passe enregistrés…
tools-settings-sync = Synchronisation des paramètres…
button-continue = Continuer

## Connection prompts

notice-connection-title = Connexion
hostkey-changed-title = La clé de l’hôte a changé !
hostkey-unknown-title = Hôte inconnu
hostkey-changed-body = ATTENTION : la clé d’hôte de { $host } a CHANGÉ.

    Cela peut signifier que quelqu’un intercepte la connexion (attaque de l’homme du milieu), ou que le serveur a été réinstallé.

    Nouvelle clé { $algorithm } :
    { $fingerprint }

    Ne continuez que si vous savez que la clé a été changée volontairement.
hostkey-unknown-body = C’est la première connexion à { $host }.

    Empreinte de la clé { $algorithm } :
    { $fingerprint }

    Vérifiez-la auprès de l’administrateur du serveur avant de lui faire confiance.
hostkey-reject = Refuser
hostkey-trust-once = Faire confiance une fois
hostkey-replace = Remplacer la clé enregistrée
hostkey-trust-remember = Faire confiance et mémoriser
remember-hint-unlocked = Enregistré chiffré dans votre coffre de mots de passe.
remember-hint-locked = Le mot de passe maître de votre coffre vous sera demandé.
remember-hint-create = Vous allez créer un coffre de mots de passe protégé par un mot de passe maître.
prompt-server-auth-title = Authentification du serveur
prompt-password-title = Mot de passe pour { $user }@{ $host }
prompt-password-label = Mot de passe
prompt-passphrase-title = Phrase secrète pour { $path }
prompt-passphrase-label = Phrase secrète
prompt-sudo-title = Mot de passe sudo pour { $who }
prompt-sudo-wrong-title = Mot de passe incorrect. Mot de passe sudo pour { $who }
secret-rejected-password = Le mot de passe enregistré a été refusé. Saisissez le mot de passe actuel.
secret-rejected-proxy = Le mot de passe du proxy enregistré a été refusé. Saisissez le mot de passe actuel.
secret-rejected-passphrase = La phrase secrète enregistrée a été refusée. Saisissez la phrase actuelle.
secret-rejected-sudo = sudo a refusé le mot de passe enregistré.
secret-update-password = Mettre à jour le mot de passe enregistré
secret-update-proxy = Mettre à jour le mot de passe du proxy enregistré
secret-update-passphrase = Mettre à jour la phrase secrète enregistrée
secret-update-sudo = Mettre à jour le mot de passe sudo enregistré
secret-remember-password = Mémoriser le mot de passe
secret-remember-proxy = Mémoriser le mot de passe du proxy
secret-remember-passphrase = Mémoriser la phrase secrète
secret-remember-sudo = Mémoriser le mot de passe sudo
vault-reason-host = { $name } a des mots de passe enregistrés. Déverrouillez-les avec votre mot de passe maître.
vault-reason-store = Déverrouillez vos mots de passe enregistrés pour enregistrer celui-ci.
vault-reason-create = Créez un coffre pour les mots de passe enregistrés. Un seul mot de passe maître les protège ; il n’est jamais stocké.
notice-not-saved-locked = Non enregistré : le coffre de mots de passe est verrouillé.
notice-save-password-failed = Impossible d’enregistrer le mot de passe : { $error }
notice-update-host-failed = Impossible de mettre à jour { $name } : { $error }
files-need-ssh = Les fichiers nécessitent SSH : { $name } est une connexion { $protocol }, qui ne peut pas transférer de fichiers.
files-root-failed = Impossible d’ouvrir en tant que root : { $error }

## Settings sync

sync-no-changes = aucun changement ici
sync-added = { $count } ajouté(s)
sync-updated = { $count } mis à jour
sync-removed = { $count } supprimé(s)
sync-settings-updated = paramètres mis à jour
sync-mode-sync = Synchronisation
sync-mode-push = Envoi
sync-mode-pull = Récupération
sync-summary = { $mode } : { $count ->
    [one] { $count } hôte synchronisé
   *[other] { $count } hôtes synchronisés
} ; { $changes }
sync-file-updated = fichier de synchronisation mis à jour
vault-reason-sync-startup = Déverrouillez vos mots de passe enregistrés pour synchroniser les paramètres (la phrase secrète de synchronisation y est conservée).
vault-reason-sync = Déverrouillez vos mots de passe enregistrés pour synchroniser les paramètres.
sync-no-folder = Choisissez d’abord un dossier de synchronisation dans Paramètres → Synchronisation des paramètres.
sync-no-passphrase = aucune phrase secrète de synchronisation enregistrée — définissez-la dans Paramètres → Synchronisation des paramètres
sync-passphrase-title = Phrase secrète de synchronisation des paramètres
sync-passphrase-label = Phrase secrète de synchronisation
sync-passphrase-wrong-title = Phrase secrète de synchronisation incorrecte — saisissez celle de vos autres ordinateurs
sync-wrong-passphrase = phrase secrète de synchronisation incorrecte
sync-last-failed = échec : { $error }
sync-done = Synchronisation des paramètres — { $summary }
sync-failed = Échec de la synchronisation des paramètres : { $error }

## Password vault

cred-sync-passphrase = Phrase secrète de synchronisation des paramètres
cred-login-password = mot de passe de connexion
cred-sudo-password = mot de passe sudo
cred-proxy-password = mot de passe du proxy
cred-key-passphrase = phrase secrète de { $file }
cred-key-unused = Phrase secrète de clé — { $path } (utilisée par aucun hôte)
cred-host-deleted = Inutilisé (son hôte a été supprimé)
strength-too-short = Trop court — au moins 8 caractères
strength-strong-passphrase = Phrase secrète robuste
strength-weak = Faible — plus long, c’est mieux (quelques mots conviennent bien)
strength-fair = Moyen
strength-strong = Robuste
vault-locked-error = le coffre est verrouillé
vault-current-wrong = Le mot de passe maître actuel est incorrect.
vault-current-master = Mot de passe maître actuel
vault-master = Mot de passe maître
vault-new-master = Nouveau mot de passe maître
vault-repeat = Confirmer
vault-enter-password = Saisissez un mot de passe.
vault-too-weak = Utilisez au moins 8 caractères — quelques mots sans rapport conviennent bien.
vault-mismatch = Les deux mots de passe ne correspondent pas.
vault-wrong-master = Mot de passe maître incorrect.
vault-strength-empty = Au moins 8 caractères ; quelques mots sans rapport conviennent bien.
vault-never-stored = Le mot de passe maître n’est jamais stocké. Si vous l’oubliez, les mots de passe enregistrés ne peuvent pas être récupérés.
vault-unlocking = Déverrouillage…
vault-encrypting = Chiffrement…
vault-path = Coffre : { $path } (partagé avec la commande hostwick)
vault-title-unlock = Déverrouiller les mots de passe enregistrés
vault-title-create = Créer un coffre de mots de passe
vault-title-change = Changer le mot de passe maître
vault-not-now = Pas maintenant
vault-unlock = Déverrouiller
vault-create = Créer le coffre
vault-change = Changer

## Host editor

form-forward-needs-spec = `{ $token }` nécessite une spécification de port
form-jump-invalid = hôte de rebond `{ $host }` : { $error }
form-port-invalid = { $field } doit être un nombre de 1 à 65535
form-baud-invalid = La vitesse (bauds) doit être un nombre comme 9600 ou 115200
form-bits-invalid = Les bits de données doivent être entre 5 et 8 et les bits d’arrêt 1 ou 2
host-section-general = Général
host-section-auth = Authentification
host-section-connection = Connexion
host-section-terminal = Terminal
host-section-files = Fichiers
host-section-advanced = Avancé
host-folder-placeholder = prod/web (facultatif)
host-host-placeholder = 10.0.0.5 ou host.example.com
host-user-placeholder = deploy (vide = votre nom d’utilisateur local)
host-keepalive-placeholder = secondes, 0 = désactivé
host-optional-placeholder = facultatif
host-x11-placeholder-windows = vide = localhost:0 (VcXsrv, Xming)
host-x11-placeholder = vide = $DISPLAY
host-script-example = Exemple :
host-snippets-empty = Aucun extrait pour cet hôte. Les extraits globaux se trouvent dans Paramètres → Extraits.
host-log-placeholder = vide = désactivé, ou p. ex. &S-&Y&M&D-&T.log
host-sftp-server-placeholder = vide = détection (/usr/lib/openssh/sftp-server, …)
host-error-no-device = Choisissez le port série (périphérique)
host-error-no-host = Saisissez le nom ou l’adresse de l’hôte
host-error-no-auth = Choisissez au moins une méthode de connexion
host-error-forwards = Redirections de ports : { $error }
host-error-keepalive = Le keepalive doit être en secondes (0 à 3600)
host-error-proxy-host = Saisissez le nom d’hôte du proxy
host-error-x11 = Affichage X11 : { $error }
host-error-script = Script de connexion, { $error }
host-saved-forget-pending = Sera oublié à l’enregistrement : { $what }
host-saved-locked = Enregistré dans votre coffre (verrouillé) : { $what }
host-saved = Enregistré dans votre coffre : { $what }
host-saved-missing = Plus dans votre coffre, il vous sera demandé : { $what }
host-saved-keep = Conserver
host-saved-forget = Oublier
host-what-password = mot de passe
host-what-key-passphrase = phrase secrète de la clé
host-what-proxy-password = mot de passe du proxy
host-what-sudo-password = mot de passe sudo
host-pick-key = Clé privée (OpenSSH ou .ppk)
host-name = Nom
host-folder = Dossier
host-protocol = Protocole
host-host = Hôte
host-port = Port
host-user = Utilisateur
host-notes = Notes
host-telnet-hint = Telnet transmet tout en clair ; utilisez-le pour du matériel réseau sur un réseau de confiance. Le nom d’utilisateur est envoyé si le serveur le demande.
host-ports-looking = Recherche…
host-ports-none = Aucun port trouvé
host-ports-count = Ports ({ $count })
host-ports-none-menu = Aucun port série trouvé — saisissez le nom du périphérique
host-device = Port (périphérique)
host-ports-refresh = Rechercher à nouveau les ports série
host-device-hint-windows = COM3, COM4… Choisissez-en un ou saisissez son nom.
host-device-hint = /dev/ttyUSB0, /dev/ttyACM0, /dev/serial/by-id/… Choisissez-en un ou saisissez son chemin.
host-baud = Vitesse (bauds)
host-data-bits = Bits de données
host-parity = Parité
host-parity-none = Aucune
host-parity-odd = Impaire
host-parity-even = Paire
host-stop-bits = Bits d’arrêt
host-flow = Contrôle de flux
host-flow-none = Aucun
host-serial-none-linux = Aucun port trouvé. Les adaptateurs USB apparaissent sous /dev/ttyUSB* ; il faut peut-être être membre du groupe dialout.
host-serial-hint = La plupart des consoles utilisent 9600 8N1 ou 115200 8N1 sans contrôle de flux.
host-auth-not-ssh = Les connexions Telnet et série s’authentifient dans le terminal. Un script de connexion (Terminal) permet de l’automatiser.
host-key-files = Fichiers de clé
host-keys-hint = Clés OpenSSH ou PuTTY .ppk, séparées par des virgules. Un certificat placé à côté d’une clé (id_ed25519-cert.pub) est utilisé automatiquement. Les clés de sécurité (sk-…) fonctionnent via l’agent.
host-auth-agent = Clés de l’agent SSH (Pageant, ssh-agent, 1Password…)
host-auth-password = Mot de passe
host-auth-remember-hint = Cochez « Mémoriser le mot de passe » lors de la demande pour vous connecter automatiquement la prochaine fois.
host-auth-kbd = Interactif au clavier (codes à usage unique, MFA)
host-auth-kerberos = Kerberos (authentification unique)
host-auth-kerberos-forward = Transmettre mon ticket Kerberos au serveur
host-auth-kerberos-hint-windows = Utilise votre connexion au domaine Windows.
host-auth-kerberos-hint = Utilise le ticket de kinit (voir klist). Sans ticket, la méthode suivante est essayée.
host-auth-order = Les méthodes sont essayées dans cet ordre : Kerberos, agent, fichiers de clé, mot de passe, interactif au clavier.
host-connection-serial = Les ports série sont locaux : pas de proxy, d’hôtes de rebond ni de redirections.
host-proxy = Proxy
host-proxy-none = Aucun
host-proxy-host-port = Hôte / port du proxy
host-proxy-port = Port du proxy
host-proxy-user = Utilisateur du proxy
host-proxy-password-hint = Le mot de passe du proxy est demandé à la connexion (cochez Mémoriser pour l’enregistrer).
host-jump-hosts = Hôtes de rebond
host-jump-hint = Séparés par des virgules, le plus externe en premier (comme ssh -J).
host-forwards = Redirections de ports
host-forwards-hint = L = local → distant, R = distant → local, D = proxy SOCKS. Démarrées avec le terminal.
host-keepalive = Keepalive (s)
host-agent-forwarding = Transmettre mon agent SSH (git sur le serveur utilise mes clés)
host-agent-forwarding-hint = Uniquement sur des serveurs de confiance : leurs administrateurs peuvent utiliser vos clés pendant que vous êtes connecté.
host-x11 = Transférer X11 (afficher ici les programmes graphiques distants)
host-x11-display = Affichage X
host-x11-hint-windows = Nécessite un serveur X en cours d’exécution, comme VcXsrv ou Xming.
host-x11-hint-macos = Nécessite XQuartz.
host-x11-hint = Utilise le serveur X de votre bureau (ou XWayland).
host-terminal-type = Type de terminal
host-login-script = Script de connexion
host-login-script-hint = Une commande par ligne, saisie après la connexion. @wait TEXTE attend une sortie (10 s), @wait 30000 TEXTE plus longtemps, @sleep 500 fait une pause ; # commence un commentaire.
host-shell-integration = Intégration du shell (bash, zsh)
host-shell-integration-hint = Permet à l’explorateur de fichiers de suivre le dossier du terminal (clic droit → Ouvrir les fichiers ici). Ajoute discrètement un hook d’invite au démarrage du shell ; rien n’est installé sur le serveur.
host-snippets-title = Extraits pour cet hôte
host-session-log = Journal de session
host-session-log-hint = Jetons : &S session, &H hôte, &P port, &U utilisateur, &Y&M&D date, &T heure. Les chemins relatifs vont dans le dossier des journaux.
host-raw-log = Journaliser la sortie brute (avec les séquences d’échappement)
host-files-not-ssh = L’explorateur de fichiers nécessite SSH.
host-file-protocol = Protocole de fichiers
host-file-protocol-auto = Auto
host-file-protocol-hint = Auto utilise SFTP et se replie sur SCP pour les serveurs qui n’en ont pas (routeurs, ESXi, busybox).
host-sudo = Ouvrir l’explorateur de fichiers en tant que root (sudo)
host-sudo-user = En tant qu’utilisateur
host-sudo-hint = Fonctionne avec sudo sans mot de passe ou avec un mot de passe sudo (demandé ou enregistré). « Rouvrir en tant que root » dans l’explorateur bascule à tout moment.
host-legacy = Autoriser les anciens algorithmes (ssh-rsa/SHA-1, ancien échange de clés) pour les vieux équipements
host-legacy-hint = Uniquement pour les vieux commutateurs, routeurs et appareils qui en ont besoin.
host-id = Identifiant de l’hôte : { $id }
host-title-edit = Modifier l’hôte
host-title-new = Nouvel hôte
loading = Chargement…

## Settings

settings-title = Paramètres
settings-section-appearance = Apparence
settings-section-terminal = Terminal
settings-section-snippets = Extraits
settings-section-connections = Connexions
settings-section-files = Fichiers
settings-section-logging = Journaux de session
settings-section-passwords = Mots de passe enregistrés
settings-section-sync = Synchronisation des paramètres
settings-section-diagnostics = Diagnostic
settings-language = Langue
settings-language-system = Système ({ $language })
settings-language-hint = S’applique dès l’enregistrement. Les boîtes de dialogue ouvertes gardent leur langue jusqu’à leur prochaine ouverture.
settings-theme = Thème
settings-theme-dark = Sombre
settings-theme-light = Clair
settings-terminal-font = Police du terminal
settings-font-size = Taille de police
settings-font-placeholder = vide = police à chasse fixe du système
settings-font-hint = Les changements de police s’appliquent aux terminaux ouverts lors de l’enregistrement.
settings-editor-placeholder = vide = application par défaut du système (p. ex. code -w { "{file}" })
settings-auto-lock-placeholder = 0 = rester déverrouillé jusqu’à la fermeture de Hostwick
settings-sync-folder-placeholder = Dossier OneDrive, Dropbox, iCloud Drive ou git
settings-sync-pass-saved = Enregistrée dans votre coffre — saisissez pour la changer
settings-sync-pass-placeholder = La même phrase secrète sur chaque ordinateur
settings-snippets-empty = Aucun extrait pour l’instant. Les extraits apparaissent sous forme de boutons au-dessus de chaque terminal et dans clic droit → Extraits.
settings-remove-failed = Impossible de le supprimer : { $error }
settings-error-font-size = Terminal → La taille de police doit être un nombre de 6 à 48
settings-error-scrollback = Terminal → L’historique doit être un nombre entier de 100 à 1000000
settings-error-auto-lock = Mots de passe enregistrés → Le délai de verrouillage doit être en minutes (0 à 1440)
settings-error-rules = Terminal → { $error }
settings-error-sync-folder = Synchronisation des paramètres → le dossier { $folder } n’existe pas
settings-folder-missing = Le dossier { $folder } n’existe pas.
settings-sync-pass-short = Utilisez une phrase secrète de synchronisation d’au moins 8 caractères.
settings-sync-pass-save-failed = Impossible d’enregistrer la phrase secrète : { $error }
vault-reason-sync-passphrase = La phrase secrète de synchronisation est conservée dans votre coffre de mots de passe.
settings-save-failed = Impossible d’enregistrer les paramètres : { $error }
settings-scrollback = Lignes d’historique
settings-right-click = Clic droit
settings-right-click-menu = Afficher le menu
settings-right-click-paste = Coller (PuTTY)
settings-copy-on-select = Copier automatiquement le texte sélectionné
settings-mouse = Envoyer les clics et le défilement de la souris aux programmes (htop, mc, vim)
settings-mouse-hint = Maintenez Maj pour sélectionner du texte lorsqu’un programme utilise la souris.
settings-button-bar = Afficher la barre d’extraits au-dessus des terminaux
settings-highlighting = Surlignage des mots-clés
settings-highlighting-hint = Colore le texte correspondant à l’écran. Les premières règles l’emportent en cas de chevauchement.
settings-snippets-hint = Disponibles dans chaque terminal. Les hôtes peuvent ajouter les leurs dans Modifier l’hôte → Terminal.
settings-auto-reconnect = Se reconnecter automatiquement en cas de coupure
settings-auto-reconnect-hint = SSH et Telnet : après 1, 2, 5, 10, puis toutes les 30 secondes, en conservant l’historique. Pas après la commande exit, ni après un échec de connexion ou de clé d’hôte.
settings-restore-tabs = Rouvrir mes onglets au démarrage de Hostwick
settings-restore-tabs-hint = Y compris les volets divisés et leurs tailles. Les onglets se connectent quand vous y passez pour la première fois.
settings-editor = Éditeur externe
settings-editor-pick = Programme d’édition
settings-editor-hint = Utilisé par Modifier (F4). { "{file}" } est remplacé par le chemin du fichier, sinon le chemin est ajouté à la fin. Utilisez l’option d’attente de l’éditeur (code -w, subl -w) s’il en a une.
settings-log-all = Journaliser chaque session dans un fichier
settings-log-folder = Dossier des journaux
settings-log-content = Contenu du journal
settings-log-printable = Texte imprimable
settings-log-raw = Brut (avec les séquences d’échappement)
settings-log-hint = Le réglage de journal propre à un hôte (Modifier l’hôte → Terminal) a priorité.
settings-log-level = Niveau de journal de l’application
settings-open-log-folder = Ouvrir le dossier des journaux
settings-data-folder = Dossier des données : { $path }
settings-vault-none = Pas encore de coffre. Cochez « Mémoriser » quand Hostwick demande un mot de passe, ou créez-en un maintenant.
settings-vault-locked = Verrouillé. Les mots de passe enregistrés sont utilisés après déverrouillage.
settings-vault-unlocked = Déverrouillé pour cette session.
settings-vault-unlock = Déverrouiller…
settings-vault-create = Créer le coffre…
settings-vault-lock = Verrouiller maintenant
settings-vault-change = Changer le mot de passe maître…
settings-vault-empty = Rien d’enregistré pour l’instant.
settings-vault-remove = Supprimer
settings-auto-lock = Verrouiller après (min)
settings-auto-lock-hint = Minutes d’inactivité avant que les mots de passe enregistrés soient de nouveau verrouillés ; 0 les laisse déverrouillés jusqu’à la fermeture de Hostwick.
settings-vault-file = Chiffré avec Argon2id + XChaCha20-Poly1305 dans { $path } — le même coffre que `hostwick vault`. Jamais synchronisé.
settings-sync-never = Jamais synchronisé sur cet ordinateur.
settings-sync-intro = Les hôtes et les paramètres sont placés dans un fichier chiffré, dans un dossier que votre stockage en ligne ou git maintient à jour. Les mots de passe enregistrés ne quittent jamais cet ordinateur.
settings-sync-folder = Dossier
settings-sync-folder-pick = Dossier de synchronisation
settings-sync-passphrase = Phrase secrète
settings-sync-passphrase-hint = Conservée dans votre coffre de mots de passe. Chaque ordinateur a besoin de la même.
settings-sync-auto = Synchroniser au démarrage, après chaque modification et toutes les 5 minutes
settings-sync-now = Synchroniser
settings-sync-push = Remplacer le fichier
settings-sync-push-tooltip = Aligner le fichier de synchronisation sur cet ordinateur (change aussi sa phrase secrète)
settings-sync-pull = Remplacer cet ordinateur
settings-sync-pull-tooltip = Aligner les hôtes et les paramètres de cet ordinateur sur le fichier de synchronisation
settings-sync-last = Dernière synchronisation

## Snippet and highlight-rule editors

snippet-label-placeholder = Libellé du bouton
snippet-text-placeholder = Texte à envoyer (plusieurs lignes sont collées en un seul bloc)
snippet-press-enter = Appuyer sur Entrée
snippet-move-up = Monter
snippet-move-down = Descendre
editor-remove = Supprimer
snippet-add = Ajouter un extrait
rule-error-empty = motif vide
rule-error-color = la couleur doit ressembler à #ff8800, pas `{ $color }`
rule-error-regex = expression régulière incorrecte
rule-pattern-placeholder = mot ou expression régulière
rule-error = Surlignage `{ $pattern }` : { $error }
rule-enabled = Activée
rule-regex-on = Expression régulière (cliquez pour un mot simple)
rule-regex-off = Mot simple, sans tenir compte de la casse (cliquez pour une expression régulière)
rule-add = Ajouter une règle
rule-defaults = Rétablir les valeurs par défaut

## Terminal

script-timeout = attente de { $after } pour « { $text } », qui n’est jamais apparu
script-session-ended = la session s’est terminée avant
search-too-complex = motif trop complexe
term-exit-status = terminé avec le code { $code }
term-exit-signal = tué par le signal { $signal }
term-connection-lost = connexion perdue
term-connection-closed = connexion fermée
term-reconnected-marker = reconnecté à { $time }
term-reconnect-failed = échec de la reconnexion : { $error }
term-break-failed = Impossible d’envoyer le break
term-no-break = Les sessions { $protocol } n’ont pas de signal break
term-script-not-run = Script de connexion non exécuté : { $error }
term-script-stopped = Script arrêté : { $error }
search-placeholder = Rechercher dans l’historique
search-no-results = Aucun résultat
search-position = { $index } sur { $total }
search-matches = { $count ->
    [one] { $count } résultat
   *[other] { $count } résultats
}
search-match-case = Respecter la casse
search-regex = Expression régulière
search-older = Résultat précédent (plus ancien) — Entrée, F3
search-newer = Résultat suivant (plus récent) — Maj+Entrée, Maj+F3
search-close = Fermer (Échap)
term-reconnect-in = Connexion perdue{ $reason }. Reconnexion dans { $seconds } s…
term-reconnect-attempt = Reconnexion (tentative { $attempt })…
term-log-badge = JOURNAL
term-reconnect-now = Maintenant
term-session-ended = Session terminée — { $reason }. Appuyez sur Entrée pour vous reconnecter.
term-menu-copy = Copier
term-menu-paste = Coller
term-menu-select-all = Tout sélectionner
term-menu-find = Rechercher…
term-menu-snippets = Extraits
term-menu-copy-folder = Copier le dossier courant
term-menu-stop-script = Arrêter le script
term-menu-clear = Effacer l’historique
term-menu-stop-log = Arrêter le journal de session
term-menu-start-log = Journaliser la session dans un fichier…
term-menu-files-here = Ouvrir les fichiers ici
term-menu-duplicate = Dupliquer la session
term-menu-reconnect-ended = Reconnecter (session terminée)

## File browser

files-move = Déplacer
files-upload = Envoyer
files-download = Télécharger
files-move-confirm = { $count ->
    [one] Déplacer « { $name } » vers { $dir } ?
   *[other] Déplacer { $count } éléments vers { $dir } ?
}
files-upload-confirm = { $count ->
    [one] Envoyer « { $name } » vers { $dir } ?
   *[other] Envoyer { $count } éléments vers { $dir } ?
}
files-download-confirm = { $count ->
    [one] Télécharger « { $name } » dans { $dir } ?
   *[other] Télécharger { $count } éléments dans { $dir } ?
}
files-same-place = { $name } : la source et la destination sont identiques
files-skipped-unsafe = Non affichés, car leurs noms ne sont pas sûrs sur cet ordinateur : { $names }
files-not-removed = { $name } : copié, mais non supprimé : { $error }
files-pick-download = Télécharger ici
files-exists = « { $name } » existe déjà
files-delete-title = Supprimer
files-delete-folder-confirm = Supprimer définitivement le dossier « { $name } » et tout son contenu ?
files-delete-file-confirm = Supprimer définitivement le fichier « { $name } » ?
files-delete-many-confirm = Supprimer définitivement { $count } éléments (dossiers avec tout leur contenu) ?
files-name-invalid = Saisissez un nom sans « / »
files-new-folder = Nouveau dossier
files-create = Créer
files-folder-name = Nom du dossier
files-rename = Renommer
files-new-name = Nouveau nom
files-new-symlink = Nouveau lien symbolique
files-symlink-placeholder = cible -> nom du lien
files-symlink-format = Écrivez-le sous la forme « cible -> nom du lien »
files-symlink-empty = La cible est vide
edit-conflict-title = Modifié sur le serveur
edit-conflict-body = « { $name } » a été modifié sur le serveur depuis que vous l’avez ouvert. Que faire de votre version enregistrée ?
edit-conflict-later = Décider plus tard
edit-conflict-theirs = Télécharger la leur
edit-conflict-copy = Enregistrer une copie
edit-conflict-overwrite = Écraser
edit-editor-failed = impossible de lancer l’éditeur « { $program } » : { $error }
keep-up-to-date = à jour
keep-synced = { $count ->
    [one] { $count } fichier synchronisé, { $size }
   *[other] { $count } fichiers synchronisés, { $size }
}
keep-failed = { $count } en échec
keep-error = erreur (nouvelle tentative) : { $error }
keep-starting = démarrage…
keep-status = Maintien à jour de { $local } → { $remote } : { $status }
edits = Modifications
edits-busy = Modifications ({ $count }) : { $status }
edits-tooltip = Fichiers ouverts dans un éditeur ; chaque enregistrement est envoyé automatiquement
edits-stop = Arrêter la surveillance
edits-stop-all = Arrêter toutes les surveillances
files-hide-local = Masquer le local
files-show-local = Afficher le local
files-local-tooltip = Afficher ou masquer les fichiers de l’ordinateur local
files-upload-pick = Envoyer…
files-synchronize = Synchroniser…
files-synchronize-tooltip = Comparer les dossiers, vérifier les changements, puis les appliquer
files-keep-stop = Arrêter le maintien à jour
files-keep = Maintenir à jour…
files-keep-tooltip = Envoyer automatiquement les changements d’un dossier local vers un dossier distant
files-following = Suit le terminal
files-follow = Suivre le terminal
files-follow-tooltip = Afficher le dossier où se trouve le terminal « { $terminal } », au fur et à mesure
files-root-switching = Basculement…
files-root-on = Root : activé
files-root-reopen = Rouvrir en tant que root
files-root-tooltip = Parcourir le serveur en tant que root (ou l’utilisateur sudo de l’hôte) via sudo
files-path-placeholder = /chemin
files-this-computer = Cet ordinateur
files-selected-of = { $selected } sur { $count } sélectionné(s) ({ $size })
files-item-count = { $count ->
    [one] { $count } élément
   *[other] { $count } éléments
}
files-selected = { $count ->
    [one] { $count } élément
   *[other] { $count } éléments
}
files-root-badge = ROOT
files-up-tooltip = Dossier parent (Retour arrière)
files-refresh-tooltip = Actualiser (Ctrl+R)
files-mkdir-tooltip = Nouveau dossier (F7)
files-col-name = Nom
files-col-size = Taille
files-col-modified = Modifié
files-col-mode = Droits
files-download-pick = Télécharger…
files-menu-open = Ouvrir
files-menu-edit = Modifier
files-menu-move = Déplacer de l’autre côté
files-menu-rename = Renommer…
files-menu-new-folder = Nouveau dossier…
files-menu-new-symlink = Nouveau lien symbolique…
files-menu-copy-path = Copier le chemin
files-menu-refresh = Actualiser
files-menu-properties = Propriétés
edit-status-downloading = téléchargement…
edit-status-watching = surveillé
edit-status-uploading = envoi…
edit-status-uploaded = envoyé à { $time }
edit-status-uploaded-in-place = envoyé à { $time } (réécrit sur place)
edit-status-conflict = modifié sur le serveur — en attente de votre décision
edit-status-error = erreur : { $error }
edit-gone = { $path } : n’existe plus
edit-kept-folder = Les fichiers modifiés non envoyés ont été conservés dans { $path }
edit-is-folder = { $path } est un dossier
edit-missing-after-upload = { $path } : absent après l’envoi
edit-size-mismatch = { $path } : le serveur a { $server } octets après l’envoi de { $uploaded }
edit-no-copy-name = aucun nom libre pour la copie

## File properties

props-title = Propriétés
props-applying = Application…
props-apply = Appliquer
props-calculating = calcul…
props-error = erreur : { $error }
props-usage = { $size } ({ $bytes } octets) dans { $files ->
    [one] { $files } fichier
   *[other] { $files } fichiers
}, { $folders ->
    [one] { $folders } dossier
   *[other] { $folders } dossiers
}
props-usage-partial = (arrêté tôt : trop d’entrées)
props-bad-id = « { $id } » n’est pas un identifiant numérique
props-bad-octal = Les droits doivent être en octal, p. ex. 0644
props-kind-folder = Dossier
props-kind-file = Fichier
props-kind-symlink = Lien symbolique → { $target }
props-kind-special = Fichier spécial
props-unknown-name = (nom inconnu)
props-not-reported = non fourni par le serveur
props-read = Lecture
props-write = Écriture
props-execute = Exécution
props-owner = Propriétaire
props-group = Groupe
props-others = Autres
props-special = Spécial
props-octal = Octal
props-calculate = Calculer
props-size = { $size } ({ $bytes } octets)
props-location = Emplacement
props-type = Type
props-checksum = Somme de contrôle
props-unknown = inconnu
props-permissions = Droits
props-recursive = Appliquer à tout le contenu (les fichiers gardent leurs bits d’exécution)

## Synchronize dialog

files-synchronize-title = Synchroniser
sync-keep = Maintenir le distant à jour
sync-keep-tooltip = Surveiller le dossier local et envoyer chaque changement (local → distant uniquement)
sync-comparing = Comparaison…
sync-preview = Aperçu
sync-run-checked = Synchroniser la sélection
sync-exclude-placeholder = p. ex. .git, *.tmp, node_modules
sync-choose-folders = Choisissez les deux dossiers
sync-use-local = Garder le local
sync-use-remote = Garder le distant
sync-total-uploads = { $count ->
    [one] { $count } envoi, { $size }
   *[other] { $count } envois, { $size }
}
sync-total-downloads = { $count ->
    [one] { $count } téléchargement, { $size }
   *[other] { $count } téléchargements, { $size }
}
sync-total-deletions = { $count ->
    [one] { $count } suppression
   *[other] { $count } suppressions
}
sync-total-conflicts = { $count ->
    [one] { $count } conflit — choisissez un côté ou laissez-le
   *[other] { $count } conflits — choisissez un côté ou laissez-les
}
sync-in-sync = Les deux dossiers sont déjà synchronisés.
sync-checked = Sélection : { $items }
sync-local-folder = Dossier local
sync-remote-folder = Dossier distant
sync-direction = Sens
sync-dir-up = Local → distant
sync-dir-down = Distant → local
sync-dir-both = Les deux (le plus récent l’emporte)
sync-mirror = Supprimer les fichiers absents de la source (miroir)
sync-exclude = Exclure
sync-comparing-folders = Comparaison des dossiers…
sync-result = Synchronisation : { $summary }
sync-running = Synchronisation…
sync-run-failed = Échec de la synchronisation : { $error }
sync-kind-upload = Envoyer
sync-kind-download = Télécharger
sync-kind-delete-local = Supprimer en local
sync-kind-delete-remote = Supprimer à distance
sync-kind-conflict = Conflit
sync-kind-skip = Ignorer
sync-reason-new = nouveau
sync-reason-newer = plus récent
sync-reason-size = taille différente
sync-reason-differs = différent
sync-reason-not-in-source = absent de la source
sync-reason-both-changed = modifié des deux côtés (même heure, taille différente)
sync-reason-unknown = comparaison impossible (pas d’horodatage)
sync-reason-destination-newer = la destination est plus récente
sync-reason-new-folder = nouveau dossier
sync-reason-case-conflict = un autre fichier ne diffère que par la casse
sync-review-deletions = Cette synchronisation supprime des fichiers. Vérifiez la liste ci-dessous, puis cliquez de nouveau sur Synchroniser.
sync-report-uploaded = { $count } envoyé(s)
sync-report-downloaded = { $count } téléchargé(s)
sync-report-deleted = { $count } supprimé(s)
sync-report-folders = { $count ->
    [one] { $count } dossier créé
   *[other] { $count } dossiers créés
}
sync-report-failed = { $count } en échec
sync-report-nothing = rien à faire

## Key tools

keyformat-openssh-hint = Pour OpenSSH, Hostwick, Git et la plupart des outils (un fichier .pub est aussi écrit).
keyformat-ppk3-hint = Pour PuTTY 0.75+ et WinSCP (protection Argon2).
keyformat-ppk2-hint = Pour les anciennes versions de PuTTY, WinSCP et FileZilla.
key-fingerprint = Empreinte : { $fingerprint }
keygen-title = Générer une clé
keygen-generate = Générer
keygen-passphrase-placeholder = Phrase secrète (recommandée ; vide = aucune)
keygen-repeat = Confirmer
keygen-mismatch = Les phrases secrètes ne correspondent pas.
keygen-no-path = Choisissez où enregistrer la clé.
keygen-exists = { $path } existe déjà. Choisissez un autre nom ou cochez « Écraser ».
keygen-used-in-host = { $name } se connecte désormais avec { $path }. Ajoutez la clé publique au fichier ~/.ssh/authorized_keys du serveur.
keygen-ed25519-recommended = Ed25519 (recommandé)
keygen-saved = { $file } (clé privée) et { $file }.pub enregistrés dans { $dir }
keygen-public-key = Clé publique (pour ~/.ssh/authorized_keys) :
keygen-copy-public = Copier la clé publique
keygen-use-in-host = Utiliser dans un hôte…
keygen-type = Type
keygen-comment = Commentaire
keygen-format = Format
keygen-save-as = Enregistrer sous
keygen-overwrite = Écraser un fichier existant
keygen-generating-rsa = Génération… RSA 4096 peut prendre quelques secondes.
keygen-generating = Génération…
keyconv-title = Examiner ou convertir une clé
keyconv-path-placeholder = ~/.ssh/id_ed25519 ou C:\keys\work.ppk
keyconv-current-placeholder = Phrase secrète actuelle (s’il y en a une)
keyconv-new-placeholder = Nouvelle phrase secrète (vide = aucune)
keyconv-repeat-placeholder = Confirmer la nouvelle phrase secrète
keyconv-output = Fichier de sortie
keyconv-mismatch = Les nouvelles phrases secrètes ne correspondent pas.
keyconv-pass-changed = Phrase secrète changée ({ $format }).
keyconv-pass-removed = Phrase secrète supprimée ({ $format }).
keyconv-wrote = { $path } écrit ({ $format }).
keyconv-needs-passphrase = Cette clé a une phrase secrète : saisissez-la dans « Phrase secrète actuelle ».
keyconv-wrong-passphrase = Phrase secrète incorrecte.
keyconv-exists = { $path } existe déjà ; choisissez un autre fichier de sortie.
keyconv-key-file = Fichier de clé
keyconv-type-bits = { $kind } { $bits } bits ({ $algorithm })
keyconv-protected = Protégée
keyconv-public-only = clé publique uniquement
keyconv-encrypted = oui, par une phrase secrète
keyconv-no-passphrase = pas de phrase secrète
keyconv-fingerprint = Empreinte
keyconv-inspect = Examiner
keyconv-current = Actuelle
keyconv-new = Nouvelle phrase secrète
keyconv-change = Changer la phrase secrète
keyconv-convert-to = Convertir en
keyconv-convert = Convertir
keyconv-keeps-passphrase = Conserve la phrase secrète actuelle, sauf si vous en saisissez une nouvelle.
keyconv-working = Traitement…
kh-title = Hôtes connus
kh-search-placeholder = Rechercher un hôte, un type de clé ou une empreinte
kh-forget-placeholder = hôte ou hôte:port
kh-nothing-matched = Aucune correspondance ; le fichier n’a pas changé.
kh-removed = { $count ->
    [one] { $count } clé supprimée.
   *[other] { $count } clés supprimées.
}
kh-type-host = Saisissez un nom d’hôte ou hôte:port.
kh-col-line = Ligne
kh-col-hosts = Hôte(s)
kh-empty = Aucun hôte connu pour l’instant. Les clés sont ajoutées quand vous faites confiance à un serveur.
kh-no-match = Rien ne correspond à la recherche.
kh-reload = Recharger
kh-remove-selected = Supprimer la sélection ({ $count })
kh-forget-host = Oublier un hôte
kh-forget-button = Supprimer toutes ses clés
kh-footer = { $path } — { $shown } sur { $total } affichés. Les noms hachés sont pris en compte quand vous oubliez un hôte par son nom.
files-opening-root = ouverture des fichiers en tant que root
files-opening = ouverture de l’explorateur de fichiers

## Keyboard and screen readers

sidebar-vault-lock = Verrouiller les mots de passe enregistrés
sidebar-quick-connect = Connexion rapide (user@host:port, Entrée)
sidebar-hosts-label = Hôtes enregistrés
sidebar-hosts-keys = Les flèches choisissent un hôte. Entrée ouvre un terminal, Maj+Entrée les fichiers, F2 modifie, Suppr supprime, Maj+F10 ouvre le menu.
tabs-label = Sessions ouvertes
tabs-keys = Gauche et Droite changent d’onglet, Entrée entre dans l’onglet, Suppr le ferme, Maj+F10 ouvre son menu. Ctrl+F6 passe de la liste des hôtes aux onglets puis à la session.
tab-state-not-connected = pas encore connecté
tab-state-connected = connecté
tab-state-disconnected = déconnecté
tab-spoken = { $title }, { $state }, onglet { $index } sur { $count }
tab-close = Fermer { $title }
pane-divider = Séparateur de volets

## Proxy test

vault-reason-proxy-test = Déverrouillez vos mots de passe enregistrés pour utiliser le mot de passe du proxy enregistré.
proxy-test-closed = Le proxy s’est connecté, mais { $target } a aussitôt fermé la connexion.
proxy-test-ok-ssh = Connecté à { $target } via le proxy. Le serveur indique : { $server }
proxy-test-ok = Connecté à { $target } via le proxy (le serveur n’est pas un serveur SSH).
proxy-test-ok-quiet = Connecté à { $target } via le proxy (le serveur n’a encore rien envoyé).
proxy-test-read-failed = Connecté à { $target } via le proxy, puis : { $error }
host-proxy-test = Tester la connexion
host-proxy-testing = Test en cours…
host-proxy-test-tooltip = Se connecter via le proxy au port de cet hôte (ou du premier hôte de rebond) et indiquer le résultat
host-baud-common = Vitesses courantes
dismiss = Fermer
term-no-output = Pas encore de sortie.
term-text-view-title = Sortie du terminal : { $title }
term-text-view-hint = { $lines ->
    [one] { $lines } ligne. Lisez-la avec les flèches ; Échap revient au terminal.
   *[other] { $lines } lignes. Lisez-les avec les flèches ; Échap revient au terminal.
}
term-a11y-label = Terminal : { $title }
term-a11y-keys = La nouvelle sortie est lue dès qu’elle se stabilise. { $read } relit la dernière sortie, { $view } affiche toute la sortie sous forme de texte. Ctrl+F6 quitte le terminal.
term-menu-read-output = Lire la dernière sortie
term-menu-text-view = Afficher la sortie sous forme de texte…
files-path-label = Dossier : { $side }
files-list-label = { $side } : { $dir }
files-list-keys = Les flèches déplacent, Maj+flèches étend la sélection, Espace ajoute ou retire une ligne, Entrée ouvre, Retour arrière remonte, F5 copie de l’autre côté, F6 déplace, F2 renomme, Suppr supprime, Alt+Entrée affiche les propriétés, Tab change de côté, Maj+F10 ouvre le menu.
files-kind-link = lien
a11y-sections = Sections
a11y-section-problem = { $section } (à vérifier)
kh-row-label = Ligne { $line } : { $hosts }
rule-color = Couleur (#rrggbb)
toast-info = Information
toast-success = Terminé
toast-warning = Avertissement
toast-error = Erreur
toast-close = Fermer la notification
menu-close-pane = Fermer le volet
pane-files-title = Fichiers : { $host }
