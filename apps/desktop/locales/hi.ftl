# Hostwick desktop: हिन्दी।
#
# en.ftl (मूल) का अनुवाद। कुंजियाँ और वेरिएबल ({ $name }) नहीं बदलते;
# उत्पादों और प्रोटोकॉल के नाम (SSH, SFTP, SCP, sudo, Kerberos, PuTTY,
# WinSCP, X11, SOCKS) अनुवादित नहीं किए जाते।

## General

error-internal = आंतरिक त्रुटि: { $error }

## Main window

sidebar-search-placeholder = होस्ट खोजें
split-only-terminals = केवल टर्मिनल टैब को विभाजित किया जा सकता है।
files-follow-no-folder = टर्मिनल ने अभी तक अपना फ़ोल्डर नहीं बताया है। होस्ट संपादित करें → टर्मिनल में “शेल इंटीग्रेशन” चालू करें ताकि फ़ाइल ब्राउज़र उसका अनुसरण कर सके (bash और zsh)।
host-copy-name = { $name } (प्रतिलिपि)
size-b = B
size-kib = KiB
size-mib = MiB
size-gib = GiB
size-tib = TiB
protocol-ssh = SSH
protocol-telnet = Telnet
protocol-serial = सीरियल
sidebar-vault-unlocked-tooltip = सहेजे गए पासवर्ड अनलॉक हैं — लॉक करने के लिए क्लिक करें
sidebar-settings-tooltip = सेटिंग्स ({ $keys })
sidebar-open-terminal = टर्मिनल
sidebar-open-files = फ़ाइलें
sidebar-edit-host = होस्ट संपादित करें
sidebar-delete-host = होस्ट हटाएँ
sidebar-new-host = नया होस्ट…
tab-files-title = फ़ाइलें: { $name }
broadcast = ब्रॉडकास्ट
broadcast-on = ब्रॉडकास्ट: चालू
broadcast-tooltip = हर खुले टर्मिनल में कीस्ट्रोक भेजें ({ $keys })
shortcut-new-host = नया होस्ट
shortcut-duplicate-tab = टैब की प्रतिलिपि
shortcut-close-pane = पेन/टैब बंद करें
shortcut-reconnect = फिर से कनेक्ट करें
shortcut-next-tab = अगला टैब
shortcut-split-right = दाईं ओर विभाजित करें
shortcut-split-down = नीचे विभाजित करें
shortcut-move-between-panes = पेन के बीच जाएँ
shortcut-broadcast = ब्रॉडकास्ट
shortcut-find = खोजें
shortcut-settings = सेटिंग्स
empty-title = कोई सत्र खुला नहीं है
empty-hint-connect = किसी होस्ट पर डबल-क्लिक करें, या त्वरित-कनेक्ट बॉक्स में user@host लिखें।
empty-hint-menus = और कार्यों के लिए होस्ट, टैब, फ़ाइलों और टर्मिनल पर राइट-क्लिक करें।
empty-hint-import = ~/.ssh/config, PuTTY, WinSCP या Solar-PuTTY से आयात करने के लिए “आयात करें…” (नीचे बाईं ओर) का उपयोग करें।
pane-connecting = { $host } से कनेक्ट हो रहा है…
pane-retry = फिर से प्रयास करें
transfer-progress = { $total } में से { $done } — { $rate }/s
transfer-done = पूरा — { $size }
transfer-skipped = छोड़ा गया
transfer-cancelled = रद्द किया गया
transfer-failed = विफल: { $error }
transfer-cancel = रद्द करें
transfers-title = { $count ->
    [one] स्थानांतरण — { $count } सक्रिय
   *[other] स्थानांतरण — { $count } सक्रिय
}
transfers-clear = पूरे हुए हटाएँ

## Common buttons

button-cancel = रद्द करें
button-delete = हटाएँ
button-ok = ठीक है
button-save = सहेजें
button-close = बंद करें
button-browse = ब्राउज़ करें…

## Menus, import

host-delete-title = होस्ट हटाएँ
host-delete-confirm = सहेजे गए होस्ट “{ $name }” को हटाएँ?
sidebar-import = आयात करें…
sidebar-tools = टूल
import-no-home = कोई होम फ़ोल्डर नहीं
import-pick-putty = PuTTY .reg निर्यात
import-pick-solar = Solar-PuTTY निर्यात (.json)
import-pick-winscp = WinSCP.ini
import-failed = आयात विफल रहा: { $error }
import-summary = { $added } जोड़े गए, { $updated } अपडेट किए गए, { $notes } में टिप्पणियाँ
import-done = आयात किया गया: { $summary }
import-report-title = आयात रिपोर्ट
import-report-attention = ध्यान देना ज़रूरी ({ $count })
import-report-added = जोड़े गए ({ $count })
import-report-updated = अपडेट किए गए ({ $count })
import-report-note = “ध्यान देना ज़रूरी” में दिखे होस्ट फिर भी आयात किए गए हैं, जब तक टिप्पणी में छोड़े जाने की बात न हो। किसी होस्ट को बदलने के लिए उस पर राइट-क्लिक करें → संपादित करें…।
import-menu-ssh-config = OpenSSH ~/.ssh/config
import-menu-putty-registry = PuTTY (इस PC पर सहेजे गए सत्र)
import-menu-winscp-registry = WinSCP (इस PC पर सहेजी गई साइटें)
import-menu-putty-file = PuTTY .reg निर्यात फ़ाइल…
import-menu-winscp-file = WinSCP.ini फ़ाइल…
import-menu-solar-file = Solar-PuTTY .json निर्यात…
menu-open-terminal = टर्मिनल खोलें
menu-open-files = फ़ाइलें खोलें (SFTP)
menu-edit = संपादित करें…
menu-duplicate = प्रतिलिपि बनाएँ
menu-copy-address = पता कॉपी करें
menu-delete = हटाएँ…
menu-reconnect = फिर से कनेक्ट करें
menu-split-right = दाईं ओर विभाजित करें
menu-split-down = नीचे विभाजित करें
menu-send-break = ब्रेक भेजें
menu-follow-terminal = टर्मिनल के फ़ोल्डर का अनुसरण करें
menu-close = बंद करें
menu-close-others = अन्य बंद करें
menu-close-all = सभी बंद करें
tools-generate-key = कुंजी बनाएँ…
tools-inspect-key = कुंजी जाँचें या बदलें…
tools-known-hosts = ज्ञात होस्ट…
tools-saved-passwords = सहेजे गए पासवर्ड…
tools-settings-sync = सेटिंग्स सिंक…
button-continue = जारी रखें

## Connection prompts

notice-connection-title = कनेक्शन
hostkey-changed-title = होस्ट कुंजी बदल गई है!
hostkey-unknown-title = अज्ञात होस्ट
hostkey-changed-body = चेतावनी: { $host } की होस्ट कुंजी बदल गई है।

    इसका अर्थ हो सकता है कि कोई कनेक्शन को बीच में पकड़ रहा है (मैन-इन-द-मिडल), या सर्वर दोबारा इंस्टॉल किया गया है।

    नई { $algorithm } कुंजी:
    { $fingerprint }

    तभी जारी रखें जब आपको पता हो कि कुंजी जान-बूझकर बदली गई है।
hostkey-unknown-body = { $host } से यह पहला कनेक्शन है।

    { $algorithm } कुंजी का फ़िंगरप्रिंट:
    { $fingerprint }

    इस पर भरोसा करने से पहले सर्वर के व्यवस्थापक से इसकी पुष्टि करें।
hostkey-reject = अस्वीकार करें
hostkey-trust-once = एक बार भरोसा करें
hostkey-replace = सहेजी गई कुंजी बदलें
hostkey-trust-remember = भरोसा करें और याद रखें
remember-hint-unlocked = आपके पासवर्ड वॉल्ट में एन्क्रिप्ट करके सहेजा जाएगा।
remember-hint-locked = आपसे वॉल्ट का मास्टर पासवर्ड पूछा जाएगा।
remember-hint-create = आप मास्टर पासवर्ड वाला एक पासवर्ड वॉल्ट बनाएँगे।
prompt-server-auth-title = सर्वर प्रमाणीकरण
prompt-password-title = { $user }@{ $host } का पासवर्ड
prompt-password-label = पासवर्ड
prompt-passphrase-label = पासफ़्रेज़
prompt-sudo-title = { $who } का sudo पासवर्ड
prompt-sudo-wrong-title = गलत पासवर्ड। { $who } का sudo पासवर्ड
secret-rejected-password = सहेजा गया पासवर्ड स्वीकार नहीं हुआ। वर्तमान पासवर्ड लिखें।
secret-rejected-proxy = सहेजा गया प्रॉक्सी पासवर्ड स्वीकार नहीं हुआ। वर्तमान पासवर्ड लिखें।
secret-rejected-passphrase = सहेजा गया पासफ़्रेज़ स्वीकार नहीं हुआ। वर्तमान पासफ़्रेज़ लिखें।
secret-rejected-sudo = sudo ने सहेजा गया पासवर्ड स्वीकार नहीं किया।
secret-update-password = सहेजा गया पासवर्ड अपडेट करें
secret-update-proxy = सहेजा गया प्रॉक्सी पासवर्ड अपडेट करें
secret-update-passphrase = सहेजा गया पासफ़्रेज़ अपडेट करें
secret-update-sudo = सहेजा गया sudo पासवर्ड अपडेट करें
secret-remember-password = पासवर्ड याद रखें
secret-remember-proxy = प्रॉक्सी पासवर्ड याद रखें
secret-remember-passphrase = पासफ़्रेज़ याद रखें
secret-remember-sudo = sudo पासवर्ड याद रखें
vault-reason-host = { $name } के पासवर्ड सहेजे गए हैं। उन्हें अपने मास्टर पासवर्ड से अनलॉक करें।
vault-reason-store = इसे सहेजने के लिए अपने सहेजे गए पासवर्ड अनलॉक करें।
vault-reason-create = सहेजे गए पासवर्ड के लिए एक वॉल्ट बनाएँ। एक मास्टर पासवर्ड उनकी सुरक्षा करता है; उसे कभी संग्रहीत नहीं किया जाता।
notice-not-saved-locked = सहेजा नहीं गया: पासवर्ड वॉल्ट लॉक है।
notice-save-password-failed = पासवर्ड सहेजा नहीं जा सका: { $error }
notice-update-host-failed = { $name } अपडेट नहीं हो सका: { $error }
files-need-ssh = फ़ाइलों के लिए SSH चाहिए: { $name } एक { $protocol } कनेक्शन है, जो फ़ाइलें स्थानांतरित नहीं कर सकता।
files-root-failed = root के रूप में नहीं खुल सका: { $error }

## Settings sync

sync-no-changes = यहाँ कोई बदलाव नहीं
sync-added = { $count } जोड़े गए
sync-updated = { $count } अपडेट किए गए
sync-removed = { $count } हटाए गए
sync-settings-updated = सेटिंग्स अपडेट हुईं
sync-mode-sync = सिंक
sync-mode-push = भेजें
sync-mode-pull = लाएँ
sync-summary = { $mode }: { $count ->
    [one] { $count } होस्ट सिंक में
   *[other] { $count } होस्ट सिंक में
}; { $changes }
sync-file-updated = सिंक फ़ाइल अपडेट हुई
vault-reason-sync-startup = सेटिंग्स सिंक करने के लिए अपने सहेजे गए पासवर्ड अनलॉक करें (सिंक पासफ़्रेज़ वहीं रखा है)।
vault-reason-sync = सेटिंग्स सिंक करने के लिए अपने सहेजे गए पासवर्ड अनलॉक करें।
sync-no-folder = पहले सेटिंग्स → सेटिंग्स सिंक में एक सिंक फ़ोल्डर चुनें।
sync-no-passphrase = कोई सिंक पासफ़्रेज़ सहेजा नहीं गया — उसे सेटिंग्स → सेटिंग्स सिंक में सेट करें
sync-passphrase-title = सेटिंग्स सिंक पासफ़्रेज़
sync-passphrase-label = सिंक पासफ़्रेज़
sync-passphrase-wrong-title = गलत सिंक पासफ़्रेज़ — वह लिखें जो आपके दूसरे कंप्यूटर उपयोग करते हैं
sync-wrong-passphrase = गलत सिंक पासफ़्रेज़
sync-last-failed = विफल: { $error }
sync-done = सेटिंग्स सिंक — { $summary }
sync-failed = सेटिंग्स सिंक विफल रहा: { $error }

## Password vault

cred-sync-passphrase = सेटिंग्स सिंक पासफ़्रेज़
cred-login-password = लॉगिन पासवर्ड
cred-sudo-password = sudo पासवर्ड
cred-proxy-password = प्रॉक्सी पासवर्ड
cred-key-passphrase = { $file } का पासफ़्रेज़
cred-key-unused = कुंजी पासफ़्रेज़ — { $path } (कोई होस्ट इसका उपयोग नहीं करता)
cred-host-deleted = अप्रयुक्त (इसका होस्ट हटा दिया गया)
strength-too-short = बहुत छोटा — कम से कम 8 वर्ण उपयोग करें
strength-strong-passphrase = मज़बूत पासफ़्रेज़
strength-weak = कमज़ोर — लंबा बेहतर है (कुछ शब्द अच्छे रहते हैं)
strength-fair = ठीक-ठाक
strength-strong = मज़बूत
vault-locked-error = वॉल्ट लॉक है
vault-current-wrong = वर्तमान मास्टर पासवर्ड गलत है।
vault-current-master = वर्तमान मास्टर पासवर्ड
vault-master = मास्टर पासवर्ड
vault-new-master = नया मास्टर पासवर्ड
vault-repeat = फिर से लिखें
vault-enter-password = पासवर्ड लिखें।
vault-too-weak = कम से कम 8 वर्ण उपयोग करें — कुछ असंबंधित शब्द अच्छे रहते हैं।
vault-mismatch = दोनों पासवर्ड मेल नहीं खाते।
vault-wrong-master = गलत मास्टर पासवर्ड।
vault-strength-empty = कम से कम 8 वर्ण; कुछ असंबंधित शब्द अच्छे रहते हैं।
vault-never-stored = मास्टर पासवर्ड कभी संग्रहीत नहीं किया जाता। अगर आप इसे भूल गए, तो सहेजे गए पासवर्ड वापस नहीं पाए जा सकते।
vault-unlocking = अनलॉक हो रहा है…
vault-encrypting = एन्क्रिप्ट हो रहा है…
vault-path = वॉल्ट: { $path } (hostwick कमांड के साथ साझा)
vault-title-unlock = सहेजे गए पासवर्ड अनलॉक करें
vault-title-create = पासवर्ड वॉल्ट बनाएँ
vault-title-change = मास्टर पासवर्ड बदलें
vault-not-now = अभी नहीं
vault-unlock = अनलॉक करें
vault-create = वॉल्ट बनाएँ
vault-change = बदलें

## Host editor

form-forward-needs-spec = `{ $token }` के लिए पोर्ट विवरण चाहिए
form-jump-invalid = जंप होस्ट `{ $host }`: { $error }
form-port-invalid = { $field } 1 से 65535 के बीच की संख्या होनी चाहिए
form-baud-invalid = गति (बॉड) 9600 या 115200 जैसी संख्या होनी चाहिए
form-bits-invalid = डेटा बिट 5–8 और स्टॉप बिट 1 या 2 होने चाहिए
host-section-general = सामान्य
host-section-auth = प्रमाणीकरण
host-section-connection = कनेक्शन
host-section-terminal = टर्मिनल
host-section-files = फ़ाइलें
host-section-advanced = उन्नत
host-folder-placeholder = prod/web (वैकल्पिक)
host-host-placeholder = 10.0.0.5 या host.example.com
host-user-placeholder = deploy (खाली = आपका स्थानीय उपयोगकर्ता नाम)
host-keepalive-placeholder = सेकंड, 0 = बंद
host-optional-placeholder = वैकल्पिक
host-x11-placeholder-windows = खाली = localhost:0 (VcXsrv, Xming)
host-x11-placeholder = खाली = $DISPLAY
host-script-example = उदाहरण:
host-snippets-empty = इस होस्ट के लिए कोई स्निपेट नहीं। सभी के लिए स्निपेट सेटिंग्स → स्निपेट में हैं।
host-log-placeholder = खाली = बंद, या जैसे &S-&Y&M&D-&T.log
host-sftp-server-placeholder = खाली = अपने-आप पता लगाएँ (/usr/lib/openssh/sftp-server, …)
host-error-no-device = सीरियल पोर्ट (डिवाइस) चुनें
host-error-no-host = होस्ट नाम या पता लिखें
host-error-no-auth = लॉग इन करने का कम से कम एक तरीका चुनें
host-error-forwards = पोर्ट फ़ॉरवर्ड: { $error }
host-error-keepalive = कीपअलाइव सेकंड में होना चाहिए (0 से 3600)
host-error-proxy-host = प्रॉक्सी का होस्ट नाम लिखें
host-error-x11 = X11 डिस्प्ले: { $error }
host-error-script = लॉगिन स्क्रिप्ट, { $error }
host-saved-forget-pending = सहेजने पर भुला दिया जाएगा: { $what }
host-saved-locked = आपके वॉल्ट में सहेजा गया (लॉक): { $what }
host-saved = आपके वॉल्ट में सहेजा गया: { $what }
host-saved-missing = अब आपके वॉल्ट में नहीं है, आपसे पूछा जाएगा: { $what }
host-saved-keep = रखें
host-saved-forget = भूल जाएँ
host-what-password = पासवर्ड
host-what-key-passphrase = कुंजी पासफ़्रेज़
host-what-proxy-password = प्रॉक्सी पासवर्ड
host-what-sudo-password = sudo पासवर्ड
host-pick-key = निजी कुंजी (OpenSSH या .ppk)
host-name = नाम
host-folder = फ़ोल्डर
host-protocol = प्रोटोकॉल
host-host = होस्ट
host-port = पोर्ट
host-user = उपयोगकर्ता
host-notes = नोट्स
host-telnet-hint = Telnet सब कुछ बिना एन्क्रिप्शन के भेजता है; इसे भरोसेमंद नेटवर्क पर नेटवर्क उपकरणों के लिए उपयोग करें। सर्वर के पूछने पर उपयोगकर्ता नाम भेजा जाता है।
host-ports-looking = खोज रहे हैं…
host-ports-none = कोई पोर्ट नहीं मिला
host-ports-count = पोर्ट ({ $count })
host-ports-none-menu = कोई सीरियल पोर्ट नहीं मिला — डिवाइस का नाम लिखें
host-device = पोर्ट (डिवाइस)
host-ports-refresh = सीरियल पोर्ट फिर से खोजें
host-device-hint-windows = COM3, COM4… कोई एक चुनें या उसका नाम लिखें।
host-device-hint = /dev/ttyUSB0, /dev/ttyACM0, /dev/serial/by-id/… कोई एक चुनें या उसका पाथ लिखें।
host-baud = गति (बॉड)
host-data-bits = डेटा बिट
host-parity = पैरिटी
host-parity-none = कोई नहीं
host-parity-odd = विषम
host-parity-even = सम
host-stop-bits = स्टॉप बिट
host-flow = फ़्लो नियंत्रण
host-flow-none = कोई नहीं
host-serial-none-linux = कोई पोर्ट नहीं मिला। USB एडेप्टर /dev/ttyUSB* के रूप में दिखते हैं; शायद आपको dialout समूह में होना चाहिए।
host-serial-hint = अधिकांश कंसोल बिना फ़्लो नियंत्रण के 9600 8N1 या 115200 8N1 उपयोग करते हैं।
host-auth-not-ssh = Telnet और सीरियल कनेक्शन टर्मिनल के अंदर लॉग इन करते हैं। इसे स्वचालित करने के लिए लॉगिन स्क्रिप्ट (टर्मिनल) उपयोग करें।
host-key-files = कुंजी फ़ाइलें
host-keys-hint = OpenSSH या PuTTY .ppk कुंजियाँ, अल्पविराम से अलग। किसी कुंजी के पास रखा प्रमाणपत्र (id_ed25519-cert.pub) अपने-आप उपयोग होता है। सुरक्षा कुंजियाँ (sk-…) एजेंट के ज़रिए काम करती हैं।
host-auth-agent = SSH एजेंट की कुंजियाँ (Pageant, ssh-agent, 1Password…)
host-auth-password = पासवर्ड
host-auth-remember-hint = अगली बार अपने-आप लॉग इन करने के लिए पूछे जाने पर “पासवर्ड याद रखें” चुनें।
host-auth-kbd = कीबोर्ड-इंटरैक्टिव (एक-बार के कोड, MFA)
host-auth-kerberos = Kerberos (सिंगल साइन-ऑन)
host-auth-kerberos-forward = मेरा Kerberos टिकट सर्वर को भेजें
host-auth-kerberos-hint-windows = आपके Windows डोमेन लॉगिन का उपयोग करता है।
host-auth-kerberos-hint = kinit के टिकट का उपयोग करता है (klist देखें)। टिकट न हो तो अगला तरीका आज़माया जाता है।
host-auth-order = तरीके इस क्रम में आज़माए जाते हैं: Kerberos, एजेंट, कुंजी फ़ाइलें, पासवर्ड, कीबोर्ड-इंटरैक्टिव।
host-connection-serial = सीरियल पोर्ट स्थानीय होते हैं: कोई प्रॉक्सी, जंप होस्ट या फ़ॉरवर्ड नहीं।
host-proxy = प्रॉक्सी
host-proxy-none = कोई नहीं
host-proxy-host-port = प्रॉक्सी होस्ट / पोर्ट
host-proxy-port = प्रॉक्सी पोर्ट
host-proxy-user = प्रॉक्सी उपयोगकर्ता
host-proxy-password-hint = कनेक्ट करते समय प्रॉक्सी पासवर्ड पूछा जाता है (सहेजने के लिए याद रखें चुनें)।
host-jump-hosts = जंप होस्ट
host-jump-hint = अल्पविराम से अलग, सबसे बाहरी पहले (ssh -J की तरह)।
host-forwards = पोर्ट फ़ॉरवर्ड
host-forwards-hint = L = स्थानीय → रिमोट, R = रिमोट → स्थानीय, D = SOCKS प्रॉक्सी। टर्मिनल के साथ शुरू होते हैं।
host-keepalive = कीपअलाइव (s)
host-agent-forwarding = मेरा SSH एजेंट फ़ॉरवर्ड करें (सर्वर पर git मेरी कुंजियाँ उपयोग करे)
host-agent-forwarding-hint = केवल भरोसेमंद सर्वर पर: आपके कनेक्ट रहते उनके व्यवस्थापक आपकी कुंजियाँ उपयोग कर सकते हैं।
host-x11 = X11 फ़ॉरवर्ड करें (रिमोट GUI प्रोग्राम यहाँ दिखाएँ)
host-x11-display = X डिस्प्ले
host-x11-hint-windows = VcXsrv या Xming जैसा X सर्वर चलता होना चाहिए।
host-x11-hint-macos = XQuartz चाहिए।
host-x11-hint = आपके डेस्कटॉप के X सर्वर (या XWayland) का उपयोग करता है।
host-terminal-type = टर्मिनल प्रकार
host-login-script = लॉगिन स्क्रिप्ट
host-login-script-hint = हर पंक्ति में एक कमांड, लॉग इन के बाद लिखी जाती है। @wait TEXT आउटपुट की प्रतीक्षा करता है (10 s), @wait 30000 TEXT अधिक देर, @sleep 500 रुकता है; # से टिप्पणी शुरू होती है।
host-shell-integration = शेल इंटीग्रेशन (bash, zsh)
host-shell-integration-hint = फ़ाइल ब्राउज़र को टर्मिनल के फ़ोल्डर का अनुसरण करने देता है (राइट-क्लिक → फ़ाइलें यहाँ खोलें)। शेल शुरू होने पर चुपचाप एक प्रॉम्प्ट हुक जोड़ता है; सर्वर पर कुछ भी इंस्टॉल नहीं होता।
host-snippets-title = इस होस्ट के स्निपेट
host-session-log = सत्र लॉग
host-session-log-hint = टोकन: &S सत्र, &H होस्ट, &P पोर्ट, &U उपयोगकर्ता, &Y&M&D तारीख, &T समय। सापेक्ष पाथ लॉग फ़ोल्डर में जाते हैं।
host-raw-log = रॉ आउटपुट लॉग करें (एस्केप कोड सहित)
host-files-not-ssh = फ़ाइल ब्राउज़र के लिए SSH चाहिए।
host-file-protocol = फ़ाइल प्रोटोकॉल
host-file-protocol-auto = स्वचालित
host-file-protocol-hint = स्वचालित SFTP उपयोग करता है और जिन सर्वरों पर यह नहीं है (राउटर, ESXi, busybox) वहाँ SCP पर चला जाता है।
host-sudo = फ़ाइल ब्राउज़र को root के रूप में खोलें (sudo)
host-sudo-user = उपयोगकर्ता के रूप में
host-sudo-hint = बिना पासवर्ड वाले sudo या sudo पासवर्ड (पूछा गया या सहेजा गया) के साथ काम करता है। ब्राउज़र में “root के रूप में फिर से खोलें” कभी भी बदल देता है।
host-legacy = पुराने उपकरणों के लिए पुराने एल्गोरिदम की अनुमति दें (ssh-rsa/SHA-1, पुराना कुंजी आदान-प्रदान)
host-legacy-hint = केवल उन पुराने स्विच, राउटर और उपकरणों के लिए जिन्हें इनकी ज़रूरत है।
host-id = होस्ट id: { $id }
host-title-edit = होस्ट संपादित करें
host-title-new = नया होस्ट
loading = लोड हो रहा है…

## Settings

settings-title = सेटिंग्स
settings-section-appearance = रूप-रंग
settings-section-terminal = टर्मिनल
settings-section-snippets = स्निपेट
settings-section-connections = कनेक्शन
settings-section-files = फ़ाइलें
settings-section-logging = सत्र लॉगिंग
settings-section-passwords = सहेजे गए पासवर्ड
settings-section-sync = सेटिंग्स सिंक
settings-section-diagnostics = निदान
settings-language = भाषा
settings-language-system = सिस्टम ({ $language })
settings-language-hint = सहेजते ही लागू होती है। खुले संवाद तब तक अपनी भाषा रखते हैं जब तक उन्हें फिर से न खोला जाए।
settings-theme = थीम
settings-theme-dark = गहरी
settings-theme-light = हल्की
settings-terminal-font = टर्मिनल फ़ॉन्ट
settings-font-size = फ़ॉन्ट आकार
settings-font-placeholder = खाली = सिस्टम मोनोस्पेस
settings-font-hint = सहेजने पर फ़ॉन्ट के बदलाव खुले टर्मिनलों पर भी लागू होते हैं।
settings-editor-placeholder = खाली = सिस्टम का डिफ़ॉल्ट ऐप (जैसे code -w { "{file}" })
settings-auto-lock-placeholder = 0 = Hostwick बंद होने तक अनलॉक रखें
settings-sync-folder-placeholder = OneDrive, Dropbox, iCloud Drive या git फ़ोल्डर
settings-sync-pass-saved = आपके वॉल्ट में सहेजा गया — बदलने के लिए लिखें
settings-sync-pass-placeholder = हर कंप्यूटर पर एक ही पासफ़्रेज़
settings-snippets-empty = अभी कोई स्निपेट नहीं। स्निपेट हर टर्मिनल के ऊपर बटन के रूप में और राइट-क्लिक → स्निपेट में दिखते हैं।
settings-remove-failed = हटाया नहीं जा सका: { $error }
settings-error-font-size = टर्मिनल → फ़ॉन्ट आकार 6 से 48 के बीच की संख्या होनी चाहिए
settings-error-scrollback = टर्मिनल → स्क्रॉलबैक 100 से 1000000 के बीच की पूर्ण संख्या होनी चाहिए
settings-error-auto-lock = सहेजे गए पासवर्ड → लॉक होने का समय मिनटों में होना चाहिए (0 से 1440)
settings-error-rules = टर्मिनल → { $error }
settings-error-sync-folder = सेटिंग्स सिंक → फ़ोल्डर { $folder } मौजूद नहीं है
settings-folder-missing = फ़ोल्डर { $folder } मौजूद नहीं है।
settings-sync-pass-short = कम से कम 8 वर्णों का सिंक पासफ़्रेज़ उपयोग करें।
settings-sync-pass-save-failed = पासफ़्रेज़ सहेजा नहीं जा सका: { $error }
vault-reason-sync-passphrase = सिंक पासफ़्रेज़ आपके पासवर्ड वॉल्ट में रखा जाता है।
settings-save-failed = सेटिंग्स सहेजी नहीं जा सकीं: { $error }
settings-scrollback = स्क्रॉलबैक पंक्तियाँ
settings-right-click = राइट-क्लिक
settings-right-click-menu = मेनू दिखाएँ
settings-right-click-paste = पेस्ट करें (PuTTY)
settings-copy-on-select = चुना गया टेक्स्ट अपने-आप कॉपी करें
settings-mouse = माउस क्लिक और स्क्रॉल प्रोग्रामों को भेजें (htop, mc, vim)
settings-mouse-hint = जब कोई प्रोग्राम माउस उपयोग कर रहा हो, तब टेक्स्ट चुनने के लिए Shift दबाए रखें।
settings-button-bar = टर्मिनलों के ऊपर स्निपेट बटन बार दिखाएँ
settings-highlighting = कीवर्ड हाइलाइटिंग
settings-highlighting-hint = स्क्रीन पर मेल खाते टेक्स्ट को रंगता है। ओवरलैप होने पर पहले वाले नियम लागू होते हैं।
settings-snippets-hint = हर टर्मिनल में उपलब्ध। होस्ट अपने स्निपेट होस्ट संपादित करें → टर्मिनल में जोड़ सकते हैं।
settings-auto-reconnect = कनेक्शन टूटने पर अपने-आप फिर से कनेक्ट करें
settings-auto-reconnect-hint = SSH और Telnet: 1, 2, 5, 10 के बाद, फिर हर 30 सेकंड में, स्क्रॉलबैक रखते हुए। exit लिखने के बाद नहीं, न ही लॉगिन या होस्ट-कुंजी विफल होने के बाद।
settings-restore-tabs = Hostwick शुरू होने पर मेरे टैब फिर से खोलें
settings-restore-tabs-hint = विभाजित पेन और उनके आकार सहित। टैब तब कनेक्ट होते हैं जब आप पहली बार उन पर जाते हैं।
settings-editor = बाहरी एडिटर
settings-editor-pick = एडिटर प्रोग्राम
settings-editor-hint = संपादित करें (F4) द्वारा उपयोग होता है। { "{file}" } को फ़ाइल के पाथ से बदला जाता है, नहीं तो पाथ अंत में जोड़ा जाता है। अगर एडिटर में प्रतीक्षा विकल्प (code -w, subl -w) हो, तो उसका उपयोग करें।
settings-log-all = हर सत्र को फ़ाइल में लॉग करें
settings-log-folder = लॉग फ़ोल्डर
settings-log-content = लॉग सामग्री
settings-log-printable = प्रिंट करने योग्य टेक्स्ट
settings-log-raw = रॉ (एस्केप कोड सहित)
settings-log-hint = होस्ट की अपनी सत्र-लॉग सेटिंग (होस्ट संपादित करें → टर्मिनल) इन्हें ओवरराइड करती है।
settings-log-level = ऐप लॉग स्तर
settings-open-log-folder = लॉग फ़ोल्डर खोलें
settings-data-folder = डेटा फ़ोल्डर: { $path }
settings-vault-none = अभी कोई वॉल्ट नहीं। जब Hostwick पासवर्ड पूछे तब “याद रखें” चुनें, या अभी एक बनाएँ।
settings-vault-locked = लॉक है। अनलॉक करने के बाद सहेजे गए पासवर्ड उपयोग होते हैं।
settings-vault-unlocked = इस सत्र के लिए अनलॉक है।
settings-vault-unlock = अनलॉक करें…
settings-vault-create = वॉल्ट बनाएँ…
settings-vault-lock = अभी लॉक करें
settings-vault-change = मास्टर पासवर्ड बदलें…
settings-vault-empty = अभी कुछ भी सहेजा नहीं गया।
settings-vault-remove = हटाएँ
settings-auto-lock = इतने समय बाद लॉक करें (मिनट)
settings-auto-lock-hint = निष्क्रियता के इतने मिनट बाद सहेजे गए पासवर्ड फिर से लॉक होते हैं; 0 उन्हें Hostwick बंद होने तक अनलॉक रखता है।
settings-vault-file = { $path } में Argon2id + XChaCha20-Poly1305 से एन्क्रिप्ट — वही वॉल्ट जो `hostwick vault` उपयोग करता है। कभी सिंक नहीं होता।
settings-sync-never = इस कंप्यूटर पर कभी सिंक नहीं हुआ।
settings-sync-intro = होस्ट और सेटिंग्स एक एन्क्रिप्टेड फ़ाइल में जाते हैं, ऐसे फ़ोल्डर में जिसे आपकी क्लाउड ड्राइव या git अद्यतन रखती है। सहेजे गए पासवर्ड कभी इस कंप्यूटर से बाहर नहीं जाते।
settings-sync-folder = फ़ोल्डर
settings-sync-folder-pick = सिंक फ़ोल्डर
settings-sync-passphrase = पासफ़्रेज़
settings-sync-passphrase-hint = आपके पासवर्ड वॉल्ट में रखा जाता है। हर कंप्यूटर को वही पासफ़्रेज़ चाहिए।
settings-sync-auto = शुरू होने पर, बदलावों के बाद और हर 5 मिनट में सिंक करें
settings-sync-now = अभी सिंक करें
settings-sync-push = सिंक फ़ाइल बदलें
settings-sync-push-tooltip = सिंक फ़ाइल को इस कंप्यूटर जैसा बनाएँ (उसका पासफ़्रेज़ भी बदलता है)
settings-sync-pull = यह कंप्यूटर बदलें
settings-sync-pull-tooltip = इस कंप्यूटर के होस्ट और सेटिंग्स को सिंक फ़ाइल जैसा बनाएँ
settings-sync-last = पिछला सिंक

## Snippet and highlight-rule editors

snippet-label-placeholder = बटन का लेबल
snippet-text-placeholder = भेजने के लिए टेक्स्ट (कई पंक्तियाँ एक ब्लॉक के रूप में पेस्ट होती हैं)
snippet-press-enter = Enter दबाएँ
snippet-move-up = ऊपर ले जाएँ
snippet-move-down = नीचे ले जाएँ
editor-remove = हटाएँ
snippet-add = स्निपेट जोड़ें
rule-error-empty = खाली पैटर्न
rule-error-color = रंग #ff8800 जैसा होना चाहिए, `{ $color }` नहीं
rule-error-regex = गलत रेगुलर एक्सप्रेशन
rule-pattern-placeholder = शब्द या रेगुलर एक्सप्रेशन
rule-error = हाइलाइट `{ $pattern }`: { $error }
rule-enabled = सक्षम
rule-regex-on = रेगुलर एक्सप्रेशन (साधारण शब्द के लिए क्लिक करें)
rule-regex-off = साधारण शब्द, किसी भी केस में (रेगुलर एक्सप्रेशन के लिए क्लिक करें)
rule-add = नियम जोड़ें
rule-defaults = डिफ़ॉल्ट बहाल करें

## Terminal

script-timeout = “{ $text }” के लिए { $after } प्रतीक्षा की, वह कभी नहीं दिखा
script-session-ended = सत्र पहले ही समाप्त हो गया
search-too-complex = पैटर्न बहुत जटिल है
term-exit-status = स्थिति { $code } के साथ बाहर निकला
term-exit-signal = सिग्नल { $signal } से समाप्त
term-connection-lost = कनेक्शन टूट गया
term-connection-closed = कनेक्शन बंद हुआ
term-reconnected-marker = { $time } पर फिर से कनेक्ट हुआ
term-reconnect-failed = फिर से कनेक्ट नहीं हो सका: { $error }
term-break-failed = ब्रेक नहीं भेजा जा सका
term-no-break = { $protocol } सत्रों में ब्रेक सिग्नल नहीं होता
term-script-not-run = लॉगिन स्क्रिप्ट नहीं चली: { $error }
term-script-stopped = स्क्रिप्ट रुकी: { $error }
search-placeholder = स्क्रॉलबैक में खोजें
search-no-results = कोई परिणाम नहीं
search-position = { $total } में से { $index }
search-matches = { $count ->
    [one] { $count } मिलान
   *[other] { $count } मिलान
}
search-match-case = केस मिलाएँ
search-regex = रेगुलर एक्सप्रेशन
search-older = पिछला (पुराना) मिलान — Enter, F3
search-newer = अगला (नया) मिलान — Shift+Enter, Shift+F3
search-close = बंद करें (Esc)
term-reconnect-in = कनेक्शन टूट गया{ $reason }। { $seconds } s में फिर से कनेक्ट होगा…
term-reconnect-attempt = फिर से कनेक्ट हो रहा है (प्रयास { $attempt })…
term-log-badge = लॉग
term-reconnect-now = अभी
term-session-ended = सत्र समाप्त — { $reason }। फिर से कनेक्ट करने के लिए Enter दबाएँ।
term-menu-copy = कॉपी करें
term-menu-paste = पेस्ट करें
term-menu-select-all = सभी चुनें
term-menu-find = खोजें…
term-menu-snippets = स्निपेट
term-menu-copy-folder = वर्तमान फ़ोल्डर कॉपी करें
term-menu-stop-script = स्क्रिप्ट रोकें
term-menu-clear = स्क्रॉलबैक साफ़ करें
term-menu-stop-log = सत्र लॉग बंद करें
term-menu-start-log = सत्र को फ़ाइल में लॉग करें…
term-menu-files-here = फ़ाइलें यहाँ खोलें
term-menu-duplicate = सत्र की प्रतिलिपि
term-menu-reconnect-ended = फिर से कनेक्ट करें (सत्र समाप्त)

## File browser

files-move = ले जाएँ
files-upload = अपलोड करें
files-download = डाउनलोड करें
files-move-confirm = { $count ->
    [one] “{ $name }” को { $dir } में ले जाएँ?
   *[other] { $count } आइटम { $dir } में ले जाएँ?
}
files-upload-confirm = { $count ->
    [one] “{ $name }” को { $dir } में अपलोड करें?
   *[other] { $count } आइटम { $dir } में अपलोड करें?
}
files-download-confirm = { $count ->
    [one] “{ $name }” को { $dir } में डाउनलोड करें?
   *[other] { $count } आइटम { $dir } में डाउनलोड करें?
}
files-same-place = { $name }: स्रोत और गंतव्य एक ही हैं
files-skipped-unsafe = नहीं दिखाए गए, क्योंकि इनके नाम इस कंप्यूटर पर सुरक्षित नहीं हैं: { $names }
files-not-removed = { $name }: कॉपी हुआ, पर हटाया नहीं गया: { $error }
files-pick-download = यहाँ डाउनलोड करें
files-exists = “{ $name }” पहले से मौजूद है
files-delete-title = हटाएँ
files-delete-folder-confirm = फ़ोल्डर “{ $name }” और उसके भीतर की हर चीज़ स्थायी रूप से हटाएँ?
files-delete-file-confirm = फ़ाइल “{ $name }” स्थायी रूप से हटाएँ?
files-delete-many-confirm = { $count } आइटम स्थायी रूप से हटाएँ (फ़ोल्डर उनकी पूरी सामग्री सहित)?
files-name-invalid = “/” के बिना नाम लिखें
files-new-folder = नया फ़ोल्डर
files-create = बनाएँ
files-folder-name = फ़ोल्डर का नाम
files-rename = नाम बदलें
files-new-name = नया नाम
files-new-symlink = नया सिमलिंक
files-symlink-placeholder = लक्ष्य -> लिंक का नाम
files-symlink-format = इसे “लक्ष्य -> लिंक का नाम” के रूप में लिखें
files-symlink-empty = लक्ष्य खाली है
edit-conflict-title = सर्वर पर बदला गया
edit-conflict-body = आपके खोलने के बाद “{ $name }” सर्वर पर बदला गया है। आपके सहेजे गए संस्करण का क्या करें?
edit-conflict-later = बाद में तय करें
edit-conflict-theirs = सर्वर वाला डाउनलोड करें
edit-conflict-copy = प्रतिलिपि के रूप में सहेजें
edit-conflict-overwrite = अधिलेखित करें
edit-editor-failed = एडिटर “{ $program }” शुरू नहीं हो सका: { $error }
keep-up-to-date = अद्यतन है
keep-synced = { $count ->
    [one] { $count } फ़ाइल सिंक हुई, { $size }
   *[other] { $count } फ़ाइलें सिंक हुईं, { $size }
}
keep-failed = { $count } विफल
keep-error = त्रुटि (फिर से प्रयास हो रहा है): { $error }
keep-starting = शुरू हो रहा है…
keep-status = { $local } → { $remote } अद्यतन रखा जा रहा है: { $status }
edits = संपादन
edits-busy = संपादन ({ $count }): { $status }
edits-tooltip = एडिटर में खुली फ़ाइलें; सहेजने पर अपने-आप अपलोड होती हैं
edits-stop = निगरानी बंद करें
edits-stop-all = सभी की निगरानी बंद करें
files-hide-local = स्थानीय छिपाएँ
files-show-local = स्थानीय दिखाएँ
files-local-tooltip = स्थानीय कंप्यूटर की फ़ाइलें दिखाएँ या छिपाएँ
files-upload-pick = अपलोड करें…
files-synchronize = सिंक्रनाइज़ करें…
files-synchronize-tooltip = फ़ोल्डरों की तुलना करें, बदलावों की समीक्षा करें, फिर उन्हें लागू करें
files-keep-stop = अद्यतन रखना बंद करें
files-keep = अद्यतन रखें…
files-keep-tooltip = स्थानीय फ़ोल्डर के बदलाव अपने-आप रिमोट फ़ोल्डर में अपलोड करें
files-following = टर्मिनल का अनुसरण कर रहा है
files-follow = टर्मिनल का अनुसरण करें
files-follow-tooltip = वह फ़ोल्डर दिखाएँ जिसमें टर्मिनल “{ $terminal }” है, जैसे-जैसे वह बदले
files-root-switching = बदल रहा है…
files-root-on = Root: चालू
files-root-reopen = root के रूप में फिर से खोलें
files-root-tooltip = sudo के ज़रिए सर्वर को root (या होस्ट के sudo उपयोगकर्ता) के रूप में ब्राउज़ करें
files-path-placeholder = /पाथ
files-this-computer = यह कंप्यूटर
files-selected-of = { $count } में से { $selected } चुने गए ({ $size })
files-item-count = { $count ->
    [one] { $count } आइटम
   *[other] { $count } आइटम
}
files-selected = { $count ->
    [one] { $count } आइटम
   *[other] { $count } आइटम
}
files-root-badge = ROOT
files-up-tooltip = ऊपर (Backspace)
files-refresh-tooltip = रीफ़्रेश करें (Ctrl+R)
files-mkdir-tooltip = नया फ़ोल्डर (F7)
files-col-name = नाम
files-col-size = आकार
files-col-modified = संशोधित
files-col-mode = अनुमतियाँ
files-download-pick = डाउनलोड करें…
files-menu-open = खोलें
files-menu-edit = संपादित करें
files-menu-move = दूसरी ओर ले जाएँ
files-menu-rename = नाम बदलें…
files-menu-new-folder = नया फ़ोल्डर…
files-menu-new-symlink = नया सिमलिंक…
files-menu-copy-path = पाथ कॉपी करें
files-menu-refresh = रीफ़्रेश करें
files-menu-properties = गुण
edit-status-downloading = डाउनलोड हो रहा है…
edit-status-watching = निगरानी में
edit-status-uploading = अपलोड हो रहा है…
edit-status-uploaded = { $time } पर अपलोड हुआ
edit-status-uploaded-in-place = { $time } पर अपलोड हुआ (उसी फ़ाइल में दोबारा लिखा गया)
edit-status-conflict = सर्वर पर बदला गया — आपकी प्रतीक्षा है
edit-status-error = त्रुटि: { $error }
edit-gone = { $path }: अब मौजूद नहीं है
edit-kept-folder = जो संपादित फ़ाइलें अपलोड नहीं हुईं, वे { $path } में रखी गई हैं
edit-is-folder = { $path } एक फ़ोल्डर है
edit-missing-after-upload = { $path }: अपलोड के बाद नहीं मिला
edit-size-mismatch = { $path }: { $uploaded } अपलोड करने के बाद सर्वर पर { $server } बाइट हैं
edit-no-copy-name = प्रतिलिपि के लिए कोई खाली नाम नहीं

## File properties

props-title = गुण
props-applying = लागू हो रहा है…
props-apply = लागू करें
props-calculating = गणना हो रही है…
props-error = त्रुटि: { $error }
props-usage = { $size } ({ $bytes } बाइट), { $files ->
    [one] { $files } फ़ाइल
   *[other] { $files } फ़ाइलों
} और { $folders ->
    [one] { $folders } फ़ोल्डर
   *[other] { $folders } फ़ोल्डरों
} में
props-usage-partial = (जल्दी रुका: बहुत अधिक प्रविष्टियाँ)
props-bad-id = “{ $id }” संख्यात्मक id नहीं है
props-bad-octal = अनुमतियाँ ऑक्टल में होनी चाहिए, जैसे 0644
props-kind-folder = फ़ोल्डर
props-kind-file = फ़ाइल
props-kind-symlink = सिम्बॉलिक लिंक → { $target }
props-kind-special = विशेष फ़ाइल
props-unknown-name = (अज्ञात नाम)
props-not-reported = सर्वर ने नहीं बताया
props-read = पढ़ें
props-write = लिखें
props-execute = चलाएँ
props-owner = स्वामी
props-group = समूह
props-others = अन्य
props-special = विशेष
props-octal = ऑक्टल
props-calculate = गणना करें
props-size = { $size } ({ $bytes } बाइट)
props-location = स्थान
props-type = प्रकार
props-checksum = चेकसम
props-unknown = अज्ञात
props-permissions = अनुमतियाँ
props-recursive = भीतर की हर चीज़ पर लागू करें (फ़ाइलें अपने execute बिट रखती हैं)

## Synchronize dialog

files-synchronize-title = सिंक्रनाइज़ करें
sync-keep = रिमोट को अद्यतन रखें
sync-keep-tooltip = स्थानीय फ़ोल्डर पर नज़र रखें और हर बदलाव अपलोड करें (केवल स्थानीय → रिमोट)
sync-comparing = तुलना हो रही है…
sync-preview = पूर्वावलोकन
sync-run-checked = चुने हुए सिंक्रनाइज़ करें
sync-exclude-placeholder = जैसे .git, *.tmp, node_modules
sync-choose-folders = दोनों फ़ोल्डर चुनें
sync-use-local = स्थानीय रखें
sync-use-remote = रिमोट रखें
sync-total-uploads = { $count ->
    [one] { $count } अपलोड, { $size }
   *[other] { $count } अपलोड, { $size }
}
sync-total-downloads = { $count ->
    [one] { $count } डाउनलोड, { $size }
   *[other] { $count } डाउनलोड, { $size }
}
sync-total-deletions = { $count ->
    [one] { $count } हटाना
   *[other] { $count } हटाना
}
sync-total-conflicts = { $count ->
    [one] { $count } टकराव — एक पक्ष चुनें या ऐसे ही छोड़ दें
   *[other] { $count } टकराव — एक पक्ष चुनें या ऐसे ही छोड़ दें
}
sync-in-sync = दोनों फ़ोल्डर पहले से सिंक में हैं।
sync-checked = चुने गए: { $items }
sync-local-folder = स्थानीय फ़ोल्डर
sync-remote-folder = रिमोट फ़ोल्डर
sync-direction = दिशा
sync-dir-up = स्थानीय → रिमोट
sync-dir-down = रिमोट → स्थानीय
sync-dir-both = दोनों (नया जीतता है)
sync-mirror = स्रोत में न मौजूद फ़ाइलें हटाएँ (मिरर)
sync-exclude = छोड़ें
sync-comparing-folders = फ़ोल्डरों की तुलना हो रही है…
sync-result = सिंक्रनाइज़: { $summary }
sync-running = सिंक्रनाइज़ हो रहा है…
sync-run-failed = सिंक्रनाइज़ विफल रहा: { $error }
sync-kind-upload = अपलोड
sync-kind-download = डाउनलोड
sync-kind-delete-local = स्थानीय हटाएँ
sync-kind-delete-remote = रिमोट हटाएँ
sync-kind-conflict = टकराव
sync-kind-skip = छोड़ें
sync-reason-new = नया
sync-reason-newer = अधिक नया
sync-reason-size = आकार अलग है
sync-reason-differs = अलग है
sync-reason-not-in-source = स्रोत में नहीं
sync-reason-both-changed = दोनों ओर बदला गया (समय एक, आकार अलग)
sync-reason-unknown = तुलना नहीं हो सकती (टाइमस्टैम्प नहीं)
sync-reason-destination-newer = गंतव्य अधिक नया है
sync-reason-new-folder = नया फ़ोल्डर
sync-reason-case-conflict = दूसरी फ़ाइल का नाम केवल अक्षरों के छोटे-बड़े होने में अलग है
sync-review-deletions = यह सिंक्रनाइज़ेशन फ़ाइलें हटाता है। नीचे दी गई सूची जाँचें, फिर दोबारा सिंक्रनाइज़ करें दबाएँ।
sync-report-uploaded = { $count } अपलोड हुए
sync-report-downloaded = { $count } डाउनलोड हुए
sync-report-deleted = { $count } हटाए गए
sync-report-folders = { $count ->
    [one] { $count } फ़ोल्डर बना
   *[other] { $count } फ़ोल्डर बने
}
sync-report-failed = { $count } विफल
sync-report-nothing = करने को कुछ नहीं

## Key tools

keyformat-openssh-hint = OpenSSH, Hostwick, Git और अधिकांश टूल के लिए (.pub फ़ाइल भी लिखी जाती है)।
keyformat-ppk3-hint = PuTTY 0.75+ और WinSCP के लिए (Argon2 से सुरक्षित)।
keyformat-ppk2-hint = PuTTY, WinSCP और FileZilla के पुराने संस्करणों के लिए।
key-fingerprint = फ़िंगरप्रिंट: { $fingerprint }
keygen-title = कुंजी बनाएँ
keygen-generate = बनाएँ
keygen-passphrase-placeholder = पासफ़्रेज़ (अनुशंसित; खाली = कोई नहीं)
keygen-repeat = फिर से लिखें
keygen-mismatch = पासफ़्रेज़ मेल नहीं खाते।
keygen-no-path = चुनें कि कुंजी कहाँ सहेजनी है।
keygen-exists = { $path } पहले से मौजूद है। कोई दूसरा नाम चुनें, या “अधिलेखित करें” चुनें।
keygen-used-in-host = { $name } अब { $path } से लॉग इन करता है। सार्वजनिक कुंजी सर्वर की ~/.ssh/authorized_keys में जोड़ें।
keygen-ed25519-recommended = Ed25519 (अनुशंसित)
keygen-saved = { $file } (निजी कुंजी) और { $file }.pub को { $dir } में सहेजा गया
keygen-public-key = सार्वजनिक कुंजी (~/.ssh/authorized_keys के लिए):
keygen-copy-public = सार्वजनिक कुंजी कॉपी करें
keygen-use-in-host = होस्ट में उपयोग करें…
keygen-type = प्रकार
keygen-comment = टिप्पणी
keygen-format = फ़ॉर्मेट
keygen-save-as = इस रूप में सहेजें
keygen-overwrite = मौजूदा फ़ाइल को अधिलेखित करें
keygen-generating-rsa = बन रही है… RSA 4096 में कुछ सेकंड लग सकते हैं।
keygen-generating = बन रही है…
keyconv-title = कुंजी जाँचें या बदलें
keyconv-path-placeholder = ~/.ssh/id_ed25519 या C:\keys\work.ppk
keyconv-current-placeholder = वर्तमान पासफ़्रेज़ (अगर है)
keyconv-new-placeholder = नया पासफ़्रेज़ (खाली = कोई नहीं)
keyconv-repeat-placeholder = नया पासफ़्रेज़ फिर से लिखें
keyconv-output = आउटपुट फ़ाइल
keyconv-mismatch = नए पासफ़्रेज़ मेल नहीं खाते।
keyconv-pass-changed = पासफ़्रेज़ बदला गया ({ $format })।
keyconv-pass-removed = पासफ़्रेज़ हटाया गया ({ $format })।
keyconv-wrote = { $path } लिखी गई ({ $format })।
keyconv-needs-passphrase = इस कुंजी का पासफ़्रेज़ है: उसे “वर्तमान पासफ़्रेज़” में लिखें।
keyconv-wrong-passphrase = गलत पासफ़्रेज़।
keyconv-exists = { $path } पहले से मौजूद है; कोई दूसरी आउटपुट फ़ाइल चुनें।
keyconv-key-file = कुंजी फ़ाइल
keyconv-type-bits = { $kind } { $bits } बिट ({ $algorithm })
keyconv-protected = सुरक्षित
keyconv-public-only = केवल सार्वजनिक कुंजी
keyconv-encrypted = हाँ, पासफ़्रेज़ के साथ
keyconv-no-passphrase = कोई पासफ़्रेज़ नहीं
keyconv-fingerprint = फ़िंगरप्रिंट
keyconv-inspect = जाँचें
keyconv-current = वर्तमान
keyconv-new = नया पासफ़्रेज़
keyconv-change = पासफ़्रेज़ बदलें
keyconv-convert-to = इसमें बदलें
keyconv-convert = बदलें
keyconv-keeps-passphrase = जब तक आप नया न लिखें, वर्तमान पासफ़्रेज़ बना रहता है।
keyconv-working = काम हो रहा है…
kh-title = ज्ञात होस्ट
kh-search-placeholder = होस्ट, कुंजी प्रकार या फ़िंगरप्रिंट खोजें
kh-forget-placeholder = होस्ट या होस्ट:पोर्ट
kh-nothing-matched = कुछ मेल नहीं खाया; फ़ाइल नहीं बदली।
kh-removed = { $count ->
    [one] { $count } कुंजी हटाई गई।
   *[other] { $count } कुंजियाँ हटाई गईं।
}
kh-type-host = होस्ट नाम, या होस्ट:पोर्ट लिखें।
kh-col-line = पंक्ति
kh-col-hosts = होस्ट
kh-empty = अभी कोई ज्ञात होस्ट नहीं। किसी सर्वर पर भरोसा करने पर कुंजियाँ जुड़ती हैं।
kh-no-match = खोज से कुछ मेल नहीं खाता।
kh-reload = फिर से लोड करें
kh-remove-selected = चुने हुए हटाएँ ({ $count })
kh-forget-host = होस्ट भूल जाएँ
kh-forget-button = इसकी सभी कुंजियाँ हटाएँ
kh-footer = { $path } — { $total } में से { $shown } दिखाए गए। नाम से होस्ट भूलने पर हैश किए गए नाम भी मिलाए जाते हैं।
files-opening-root = root के रूप में फ़ाइलें खोली जा रही हैं
files-opening = फ़ाइल ब्राउज़र खोला जा रहा है

## Keyboard and screen readers

sidebar-vault-lock = सहेजे गए पासवर्ड लॉक करें
sidebar-quick-connect = त्वरित कनेक्ट (user@host:port, Enter)
sidebar-hosts-label = सहेजे गए होस्ट
sidebar-hosts-keys = तीर कुंजियाँ चलाती हैं (Shift+तीर कई चुनता है), बाएँ और दाएँ फ़ोल्डर समेटते/खोलते हैं। Enter टर्मिनल खोलता है, Ctrl+Enter फ़ाइलें, Shift+Enter नया विभाजन, Alt+Enter संपादित करता है, F2 नाम बदलता है, Delete हटाता है, Ctrl+A सब चुनता है, Shift+F10 मेनू खोलता है।
tabs-label = खुले सत्र
tabs-keys = बाएँ और दाएँ टैब बदलते हैं, Enter टैब के अंदर जाता है, Delete उसे बंद करता है, Shift+F10 उसका मेनू खोलता है। Ctrl+F6 होस्ट सूची, टैब और सत्र के बीच ले जाता है।
tab-state-not-connected = अभी कनेक्ट नहीं
tab-state-connected = कनेक्टेड
tab-state-disconnected = डिस्कनेक्टेड
tab-spoken = { $title }, { $state }, टैब { $index } / { $count }
tab-close = { $title } बंद करें
pane-divider = पेन विभाजक

## Proxy test

vault-reason-proxy-test = सहेजा गया प्रॉक्सी पासवर्ड उपयोग करने के लिए अपने सहेजे गए पासवर्ड अनलॉक करें।
proxy-test-closed = प्रॉक्सी कनेक्ट हुआ, पर { $target } ने तुरंत कनेक्शन बंद कर दिया।
proxy-test-ok-ssh = प्रॉक्सी के ज़रिए { $target } से कनेक्ट हुआ। सर्वर कहता है: { $server }
proxy-test-ok = प्रॉक्सी के ज़रिए { $target } से कनेक्ट हुआ (सर्वर SSH सर्वर नहीं है)।
proxy-test-ok-quiet = प्रॉक्सी के ज़रिए { $target } से कनेक्ट हुआ (सर्वर ने अभी कुछ नहीं भेजा)।
proxy-test-read-failed = प्रॉक्सी के ज़रिए { $target } से कनेक्ट हुआ, फिर: { $error }
host-proxy-test = कनेक्शन जाँचें
host-proxy-testing = जाँच हो रही है…
host-proxy-test-tooltip = प्रॉक्सी के ज़रिए इस होस्ट के पोर्ट (या पहले जंप होस्ट) से कनेक्ट करें और परिणाम बताएँ
host-baud-common = सामान्य गतियाँ
dismiss = हटाएँ
term-no-output = अभी कोई आउटपुट नहीं।
term-text-view-title = टर्मिनल आउटपुट: { $title }
term-text-view-hint = { $lines ->
    [one] { $lines } पंक्ति। इसे तीर कुंजियों से पढ़ें; Esc टर्मिनल पर लौटाता है।
   *[other] { $lines } पंक्तियाँ। इन्हें तीर कुंजियों से पढ़ें; Esc टर्मिनल पर लौटाता है।
}
term-a11y-label = टर्मिनल: { $title }
term-a11y-keys = नया आउटपुट स्थिर होते ही पढ़ा जाता है। { $read } नवीनतम आउटपुट फिर से पढ़ता है, { $view } पूरा आउटपुट टेक्स्ट के रूप में दिखाता है। Ctrl+F6 टर्मिनल से बाहर ले जाता है।
term-menu-read-output = नवीनतम आउटपुट पढ़ें
term-menu-text-view = आउटपुट टेक्स्ट के रूप में दिखाएँ…
files-path-label = फ़ोल्डर: { $side }
files-list-label = { $side }: { $dir }
files-list-keys = तीर कुंजियाँ चलाती हैं, Shift+तीर चयन बढ़ाते हैं, Space पंक्ति जोड़ता या हटाता है, Enter खोलता है, Backspace ऊपर जाता है, F5 दूसरी ओर कॉपी करता है, F6 ले जाता है, F2 नाम बदलता है, Delete हटाता है, Alt+Enter गुण दिखाता है, Tab ओर बदलता है, Shift+F10 मेनू खोलता है।
files-kind-link = लिंक
a11y-sections = खंड
a11y-section-problem = { $section } (ध्यान देना ज़रूरी)
kh-row-label = पंक्ति { $line }: { $hosts }
rule-color = रंग (#rrggbb)
toast-info = जानकारी
toast-success = हो गया
toast-warning = चेतावनी
toast-error = त्रुटि
toast-close = सूचना बंद करें
menu-close-pane = पैन बंद करें
pane-files-title = फ़ाइलें: { $host }

## Command palette, sidebar, bulk edit, welcome (0.4)

prompt-passphrase-title-host = { $path } का पासफ़्रेज़ ({ $host })
prompt-server-auth-title-host = सर्वर प्रमाणीकरण — { $host }
act-broadcast = ब्रॉडकास्ट चालू या बंद करें
act-new-folder = नया फ़ोल्डर
act-quick-connect = त्वरित कनेक्ट…
act-settings-section = सेटिंग्स: { $section }
act-toggle-theme = गहरी और हल्की थीम के बीच बदलें
bulk-applies = { $count ->
    [one] { $count } होस्ट पर लागू
   *[other] { $count } होस्ट पर लागू
}
bulk-apply = लागू करें
bulk-changed = बदलेगा
bulk-done = { $count ->
    [one] { $count } होस्ट अपडेट हुआ
   *[other] { $count } होस्ट अपडेट हुए
}
bulk-error-proxy = प्रॉक्सी: { $error }
bulk-intro = { $count ->
    [one] { $count } होस्ट की सेटिंग्स बदलें।
   *[other] { $count } होस्ट की सेटिंग्स एक साथ बदलें।
} हर फ़ील्ड वह मान दिखाता है जो सबमें समान है; खाली फ़ील्ड का मतलब है कि मान अलग-अलग हैं। केवल वही फ़ील्ड सहेजे जाते हैं जिन्हें आप बदलते हैं।
bulk-keep = जैसा है रखें
bulk-mixed = अलग-अलग — रखने के लिए खाली छोड़ें
bulk-only-ssh = केवल SSH होस्ट बदले जाते हैं ({ $total } में से { $ssh })।
bulk-proxy-hint = socks5://[user@]host:1080, socks4://… या http://…; “none” प्रॉक्सी हटाता है।
bulk-serial-skipped = { $count ->
    [one] { $count } सीरियल होस्ट में फ़ोल्डर के अलावा कुछ नहीं बदलता।
   *[other] { $count } सीरियल होस्ट में फ़ोल्डर के अलावा कुछ नहीं बदलता।
}
bulk-title = { $count ->
    [one] { $count } होस्ट संपादित करें
   *[other] { $count } होस्ट संपादित करें
}
folder-delete-body = { $count ->
    [one] फ़ोल्डर “{ $name }” हटाएँ? इसका { $count } होस्ट और सभी उप-फ़ोल्डर “{ $target }” में चले जाएँगे।
   *[other] फ़ोल्डर “{ $name }” हटाएँ? इसके { $count } होस्ट और सभी उप-फ़ोल्डर “{ $target }” में चले जाएँगे।
}
folder-delete-ok = फ़ोल्डर हटाएँ
folder-delete-title = फ़ोल्डर हटाएँ
folder-new-name = नया फ़ोल्डर
group-broadcast-on = ब्रॉडकास्ट चालू है: आप जो लिखते हैं वह हर टर्मिनल में जाता है। { $keys } इसे बंद करता है।
group-confirm-body = { $count ->
    [one] इससे { $count } कनेक्शन खुलेगा।
   *[other] इससे एक साथ { $count } कनेक्शन खुलेंगे।
} पासवर्ड जैसे प्रश्न एक-एक करके आते हैं, हर एक में उसके होस्ट का नाम होता है।
group-confirm-open = सभी खोलें
group-confirm-title = { $count ->
    [one] { $count } होस्ट खोलें?
   *[other] { $count } होस्ट खोलें?
}
hosts-delete-body = ये सहेजे गए होस्ट हटाए जाएँगे: { $names }।
hosts-delete-body-more = ये सहेजे गए होस्ट हटाए जाएँगे: { $names } और { $more } अन्य।
hosts-delete-title = { $count ->
    [one] { $count } होस्ट हटाएँ?
   *[other] { $count } होस्ट हटाएँ?
}
hosts-save-failed = सहेजा नहीं जा सका: { $error }
import-menu-scan = इस कंप्यूटर को स्कैन करें…
keys-palette = कमांड पैलेट
keys-shortcuts = कीबोर्ड शॉर्टकट
keys-new-host = नया होस्ट
keys-settings = सेटिंग्स
keys-broadcast = सभी टर्मिनल में इनपुट ब्रॉडकास्ट करें
keys-cycle-focus = फ़ोकस बदलें: होस्ट सूची, टैब, सत्र
keys-context-menu = फ़ोकस वाले आइटम का संदर्भ मेनू
keys-duplicate-tab = टैब की प्रति बनाएँ
keys-close-pane = पैन बंद करें, फिर टैब
keys-reconnect = फिर से कनेक्ट करें
keys-next-tab = अगला टैब
keys-prev-tab = पिछला टैब
keys-split-right = दाईं ओर विभाजित करें
keys-split-down = नीचे विभाजित करें
keys-move-pane = विभाजित पैन के बीच जाएँ
keys-resize-pane = फ़ोकस वाले पैन का आकार बदलें
keys-host-open = टर्मिनल खोलें
keys-host-files = फ़ाइलें खोलें
keys-host-split = नए विभाजन में खोलें
keys-host-edit = संपादित करें (कई होस्ट: एक साथ संपादित करें)
keys-host-rename = होस्ट या फ़ोल्डर का नाम बदलें
keys-host-delete = हटाएँ
keys-host-select-all = सभी होस्ट चुनें
keys-host-multi = कई होस्ट चुनें
keys-host-range = होस्ट की एक श्रेणी चुनें
keys-copy = कॉपी करें
keys-paste = पेस्ट करें
keys-find = स्क्रॉलबैक में खोजें
keys-find-next = पिछला / अगला मिलान
keys-scroll-back = पीछे स्क्रॉल करें
keys-select-in-app = जब कोई प्रोग्राम माउस उपयोग कर रहा हो तब टेक्स्ट चुनें
keys-send-break = ब्रेक भेजें (telnet, सीरियल)
keys-read-output = नवीनतम आउटपुट फिर से पढ़ें (स्क्रीन रीडर)
keys-text-view = आउटपुट सादे टेक्स्ट के रूप में
keys-files-open = खोलें
keys-files-up = मूल फ़ोल्डर
keys-files-edit = अपने एडिटर में संपादित करें
keys-files-copy = अपलोड / डाउनलोड
keys-files-move = ले जाएँ
keys-files-rename = नाम बदलें
keys-files-new-folder = नया फ़ोल्डर
keys-files-delete = हटाएँ
keys-files-refresh = रीफ़्रेश करें
keys-files-switch = दूसरी ओर
keys-files-properties = गुण
keys-files-select-all = सब चुनें
keys-files-toggle-row = पंक्ति चिह्नित करें
keys-group-app = ऐप
keys-group-tabs = टैब और पैन
keys-group-hosts = होस्ट सूची
keys-group-terminal = टर्मिनल
keys-group-files = फ़ाइलें
keys-search = शॉर्टकट खोजें
keys-note = सादी Ctrl+अक्षर कुंजियाँ हमेशा शेल को जाती हैं। टर्मिनल के अंदर F1 और Ctrl+/ प्रोग्राम (htop, mc) के हैं; Ctrl+Shift+/ यह सूची कहीं से भी खोलता है। macOS पर Ctrl+Shift की जगह ⌘ है।
menu-bulk-edit = { $count ->
    [one] { $count } होस्ट संपादित करें…
   *[other] { $count } होस्ट संपादित करें…
}
menu-clear-recent = हाल के कनेक्शन साफ़ करें
menu-delete-folder = फ़ोल्डर हटाएँ…
menu-delete-hosts = { $count ->
    [one] { $count } होस्ट हटाएँ…
   *[other] { $count } होस्ट हटाएँ…
}
menu-favourite = पसंदीदा में जोड़ें
menu-unfavourite = पसंदीदा से हटाएँ
menu-new-subfolder = नया उप-फ़ोल्डर
menu-open-all-files = सभी को फ़ाइल ब्राउज़र में खोलें
menu-open-all-grid = सभी को ग्रिड में खोलें
menu-open-all-grid-broadcast = सभी को ब्रॉडकास्ट चालू करके ग्रिड में खोलें
menu-open-all-tabs = सभी को टैब में खोलें
menu-open-all-tabs-count = सभी को टैब में खोलें ({ $count })
menu-open-split = नए विभाजन में खोलें
menu-rename = नाम बदलें
palette-connect-to = { $target } से कनेक्ट करें
palette-save-as-host = { $target } को होस्ट के रूप में सहेजें…
palette-group-actions = क्रियाएँ
palette-group-connect = कनेक्ट
palette-group-snippets = स्निपेट
palette-group-ssh-config = ~/.ssh/config से
palette-group-tabs = खुले टैब
palette-hint-open = खोलें
palette-hint-files = फ़ाइलें
palette-hint-split = विभाजित
palette-hint-edit = संपादित
palette-hint-close = बंद
palette-no-terminal = स्निपेट टर्मिनल में लिखे जाते हैं; पहले एक टर्मिनल खोलें।
palette-nothing = कुछ नहीं मिला
palette-placeholder = होस्ट और क्रियाएँ खोजें, या user@host:port लिखें
palette-quick-detail = त्वरित कनेक्ट
palette-results = परिणाम
palette-title = कमांड पैलेट
quick-hint = ↵ कनेक्ट · { $files } फ़ाइलें · ↑↓ चुनें
quick-suggestions = सुझाव
rename-no-slash = नाम में / या \ नहीं हो सकता।
shortcut-palette = कमांड पैलेट
shortcut-all-shortcuts = सभी शॉर्टकट
sidebar-drag-hosts = { $count ->
    [one] { $count } होस्ट
   *[other] { $count } होस्ट
}
sidebar-favourite-toggle = पसंदीदा: { $name }
sidebar-folder-spoken = { $count ->
    [one] फ़ोल्डर { $name }, { $count } होस्ट
   *[other] फ़ोल्डर { $name }, { $count } होस्ट
}
sidebar-help-tooltip = कीबोर्ड शॉर्टकट ({ $keys })
sidebar-palette-tooltip = होस्ट और क्रियाएँ खोजें ({ $keys })
sidebar-quick-spoken = हाल का त्वरित कनेक्शन { $target }
sidebar-section-favourites = पसंदीदा
sidebar-section-recent = हाल के
sidebar-section-hosts = होस्ट
sidebar-selected-count = { $count } चुने गए
welcome-title = Hostwick में आपका स्वागत है
welcome-intro = Hostwick उन होस्ट को ला सकता है जिन्हें दूसरे SSH टूल ने इस कंप्यूटर पर सहेजा है। जो आयात करना है उसे चुनें; दूसरे प्रोग्राम नहीं बदलते।
welcome-scanning = सहेजे गए सत्र खोजे जा रहे हैं…
welcome-found = इस कंप्यूटर पर मिला
welcome-src-ssh = { $count ->
    [one] OpenSSH कॉन्फ़िग ({ $path }): { $count } होस्ट
   *[other] OpenSSH कॉन्फ़िग ({ $path }): { $count } होस्ट
}
welcome-src-putty = { $count ->
    [one] PuTTY सत्र: { $count } होस्ट
   *[other] PuTTY सत्र: { $count } होस्ट
}
welcome-src-winscp-registry = { $count ->
    [one] WinSCP साइटें: { $count } होस्ट
   *[other] WinSCP साइटें: { $count } होस्ट
}
welcome-src-winscp-ini = { $count ->
    [one] WinSCP साइटें ({ $path }): { $count } होस्ट
   *[other] WinSCP साइटें ({ $path }): { $count } होस्ट
}
welcome-src-solar = { $count ->
    [one] Solar-PuTTY निर्यात ({ $path }): { $count } होस्ट
   *[other] Solar-PuTTY निर्यात ({ $path }): { $count } होस्ट
}
welcome-keys = { $count ->
    [one] ~/.ssh की मेरी कुंजी फ़ाइल त्वरित कनेक्ट और नए होस्ट के लिए उपयोग करें
   *[other] ~/.ssh की मेरी { $count } कुंजी फ़ाइलें त्वरित कनेक्ट और नए होस्ट के लिए उपयोग करें
}
welcome-agent = { $count ->
    [one] आपके SSH एजेंट (Pageant, ssh-agent…) में { $count } कुंजी तैयार है; होस्ट इसे अपने आप उपयोग करते हैं।
   *[other] आपके SSH एजेंट (Pageant, ssh-agent…) में { $count } कुंजियाँ तैयार हैं; होस्ट इन्हें अपने आप उपयोग करते हैं।
}
welcome-agent-asking = SSH एजेंट से उसकी कुंजियाँ पूछी जा रही हैं…
welcome-agent-none = कुंजियों वाला कोई SSH एजेंट नहीं चल रहा है।
welcome-nothing = दूसरे SSH टूल द्वारा सहेजे गए कोई सत्र नहीं मिले। कोई होस्ट जोड़ें, या त्वरित कनेक्ट में user@host लिखें।
welcome-note = आप यह कभी भी फिर कर सकते हैं: आयात करें… → इस कंप्यूटर को स्कैन करें।
welcome-import = चुने हुए आयात करें
welcome-skip = छोड़ें
welcome-continue = जारी रखें
