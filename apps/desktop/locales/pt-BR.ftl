# Hostwick desktop: português (Brasil).
#
# Tradução de en.ftl (original). As chaves e as variáveis ({ $name }) não
# mudam; nomes de produtos e protocolos (SSH, SFTP, SCP, sudo, Kerberos,
# PuTTY, WinSCP, X11, SOCKS) não são traduzidos.

## General

error-internal = erro interno: { $error }

## Main window

sidebar-search-placeholder = Pesquisar hosts
split-only-terminals = Somente abas de terminal podem ser divididas.
files-follow-no-folder = O terminal ainda não informou sua pasta. Ative “Integração com o shell” em Editar host → Terminal para que o navegador de arquivos possa acompanhá-lo (bash e zsh).
host-copy-name = { $name } (cópia)
size-b = B
size-kib = KiB
size-mib = MiB
size-gib = GiB
size-tib = TiB
protocol-ssh = SSH
protocol-telnet = Telnet
protocol-serial = Serial
sidebar-vault-unlocked-tooltip = As senhas salvas estão desbloqueadas — clique para bloquear
sidebar-settings-tooltip = Configurações ({ $keys })
sidebar-open-terminal = Terminal
sidebar-open-files = Arquivos
sidebar-edit-host = Editar host
sidebar-delete-host = Excluir host
sidebar-new-host = Novo host…
tab-files-title = Arquivos: { $name }
broadcast = Transmitir
broadcast-on = Transmitir: LIGADO
broadcast-tooltip = Enviar as teclas digitadas a todos os terminais abertos ({ $keys })
shortcut-new-host = novo host
shortcut-duplicate-tab = duplicar aba
shortcut-close-pane = fechar painel/aba
shortcut-reconnect = reconectar
shortcut-next-tab = próxima aba
shortcut-split-right = dividir à direita
shortcut-split-down = dividir abaixo
shortcut-move-between-panes = alternar entre painéis
shortcut-broadcast = transmitir
shortcut-find = localizar
shortcut-settings = configurações
empty-title = Nenhuma sessão aberta
empty-hint-connect = Clique duas vezes em um host ou digite user@host na caixa de conexão rápida.
empty-hint-menus = Clique com o botão direito em hosts, abas, arquivos e terminais para mais ações.
empty-hint-import = Importe do ~/.ssh/config, PuTTY, WinSCP ou Solar-PuTTY com Importar… (canto inferior esquerdo).
pane-connecting = Conectando a { $host }…
pane-retry = Tentar novamente
transfer-progress = { $done } de { $total } — { $rate }/s
transfer-done = concluído — { $size }
transfer-skipped = ignorado
transfer-cancelled = cancelado
transfer-failed = falhou: { $error }
transfer-cancel = Cancelar
transfers-title = { $count ->
    [one] Transferências — { $count } ativa
   *[other] Transferências — { $count } ativas
}
transfers-clear = Limpar concluídas

## Common buttons

button-cancel = Cancelar
button-delete = Excluir
button-ok = OK
button-save = Salvar
button-close = Fechar
button-browse = Procurar…

## Menus, import

host-delete-title = Excluir host
host-delete-confirm = Excluir o host salvo “{ $name }”?
sidebar-import = Importar…
sidebar-tools = Ferramentas
import-no-home = nenhuma pasta pessoal
import-pick-putty = Exportação .reg do PuTTY
import-pick-solar = Exportação do Solar-PuTTY (.json)
import-pick-winscp = WinSCP.ini
import-failed = Falha na importação: { $error }
import-summary = { $added } adicionados, { $updated } atualizados, { $notes } com observações
import-done = Importado: { $summary }
import-report-title = Relatório de importação
import-report-attention = Precisam de atenção ({ $count })
import-report-added = Adicionados ({ $count })
import-report-updated = Atualizados ({ $count })
import-report-note = Os hosts listados em “Precisam de atenção” foram importados mesmo assim, a menos que a observação diga que foram ignorados. Clique com o botão direito em um host → Editar… para ajustá-lo.
import-menu-ssh-config = OpenSSH ~/.ssh/config
import-menu-putty-registry = PuTTY (sessões salvas neste PC)
import-menu-winscp-registry = WinSCP (sites salvos neste PC)
import-menu-putty-file = Arquivo de exportação .reg do PuTTY…
import-menu-winscp-file = Arquivo WinSCP.ini…
import-menu-solar-file = Exportação .json do Solar-PuTTY…
menu-open-terminal = Abrir terminal
menu-open-files = Abrir arquivos (SFTP)
menu-edit = Editar…
menu-duplicate = Duplicar
menu-copy-address = Copiar endereço
menu-delete = Excluir…
menu-reconnect = Reconectar
menu-split-right = Dividir à direita
menu-split-down = Dividir abaixo
menu-send-break = Enviar break
menu-follow-terminal = Acompanhar a pasta do terminal
menu-close = Fechar
menu-close-others = Fechar as outras
menu-close-all = Fechar todas
tools-generate-key = Gerar chave…
tools-inspect-key = Inspecionar ou converter chave…
tools-known-hosts = Hosts conhecidos…
tools-saved-passwords = Senhas salvas…
tools-settings-sync = Sincronização das configurações…
button-continue = Continuar

## Connection prompts

notice-connection-title = Conexão
hostkey-changed-title = A chave do host mudou!
hostkey-unknown-title = Host desconhecido
hostkey-changed-body = ATENÇÃO: a chave do host { $host } MUDOU.

    Isso pode significar que alguém está interceptando a conexão (ataque man-in-the-middle) ou que o servidor foi reinstalado.

    Nova chave { $algorithm }:
    { $fingerprint }

    Só continue se você souber que a chave foi trocada de propósito.
hostkey-unknown-body = Esta é a primeira conexão com { $host }.

    Impressão digital da chave { $algorithm }:
    { $fingerprint }

    Confira-a com o administrador do servidor antes de confiar nela.
hostkey-reject = Rejeitar
hostkey-trust-once = Confiar uma vez
hostkey-replace = Substituir a chave salva
hostkey-trust-remember = Confiar e lembrar
remember-hint-unlocked = Salva com criptografia no seu cofre de senhas.
remember-hint-locked = Será pedida a senha mestra do seu cofre.
remember-hint-create = Você vai criar um cofre de senhas com uma senha mestra.
prompt-server-auth-title = Autenticação do servidor
prompt-password-title = Senha de { $user }@{ $host }
prompt-password-label = Senha
prompt-passphrase-title = Frase secreta de { $path }
prompt-passphrase-label = Frase secreta
prompt-sudo-title = Senha do sudo para { $who }
prompt-sudo-wrong-title = Senha incorreta. Senha do sudo para { $who }
secret-rejected-password = A senha salva não foi aceita. Digite a atual.
secret-rejected-proxy = A senha do proxy salva não foi aceita. Digite a atual.
secret-rejected-passphrase = A frase secreta salva não foi aceita. Digite a atual.
secret-rejected-sudo = O sudo não aceitou a senha salva.
secret-update-password = Atualizar a senha salva
secret-update-proxy = Atualizar a senha do proxy salva
secret-update-passphrase = Atualizar a frase secreta salva
secret-update-sudo = Atualizar a senha do sudo salva
secret-remember-password = Lembrar senha
secret-remember-proxy = Lembrar senha do proxy
secret-remember-passphrase = Lembrar frase secreta
secret-remember-sudo = Lembrar senha do sudo
vault-reason-host = { $name } tem senhas salvas. Desbloqueie-as com sua senha mestra.
vault-reason-store = Desbloqueie suas senhas salvas para guardar esta.
vault-reason-create = Crie um cofre para as senhas salvas. Uma senha mestra as protege; ela nunca é armazenada.
notice-not-saved-locked = Não salvo: o cofre de senhas está bloqueado.
notice-save-password-failed = Não foi possível salvar a senha: { $error }
notice-update-host-failed = Não foi possível atualizar { $name }: { $error }
files-need-ssh = Arquivos exigem SSH: { $name } é uma conexão { $protocol }, que não transfere arquivos.
files-root-failed = Não foi possível abrir como root: { $error }

## Settings sync

sync-no-changes = nenhuma alteração aqui
sync-added = { $count } adicionados
sync-updated = { $count } atualizados
sync-removed = { $count } removidos
sync-settings-updated = configurações atualizadas
sync-mode-sync = Sincronizar
sync-mode-push = Enviar
sync-mode-pull = Receber
sync-summary = { $mode }: { $count ->
    [one] { $count } host sincronizado
   *[other] { $count } hosts sincronizados
}; { $changes }
sync-file-updated = arquivo de sincronização atualizado
vault-reason-sync-startup = Desbloqueie suas senhas salvas para sincronizar as configurações (a frase de sincronização fica lá).
vault-reason-sync = Desbloqueie suas senhas salvas para sincronizar as configurações.
sync-no-folder = Escolha primeiro uma pasta de sincronização em Configurações → Sincronização das configurações.
sync-no-passphrase = nenhuma frase de sincronização salva — defina-a em Configurações → Sincronização das configurações
sync-passphrase-title = Frase secreta da sincronização das configurações
sync-passphrase-label = Frase de sincronização
sync-passphrase-wrong-title = Frase de sincronização incorreta — digite a que seus outros computadores usam
sync-wrong-passphrase = frase de sincronização incorreta
sync-last-failed = falhou: { $error }
sync-done = Sincronização das configurações — { $summary }
sync-failed = Falha na sincronização das configurações: { $error }

## Password vault

cred-sync-passphrase = Frase de sincronização das configurações
cred-login-password = senha de login
cred-sudo-password = senha do sudo
cred-proxy-password = senha do proxy
cred-key-passphrase = frase secreta de { $file }
cred-key-unused = Frase secreta de chave — { $path } (nenhum host a usa)
cred-host-deleted = Sem uso (o host foi excluído)
strength-too-short = Curta demais — use pelo menos 8 caracteres
strength-strong-passphrase = Frase secreta forte
strength-weak = Fraca — quanto mais longa, melhor (algumas palavras funcionam bem)
strength-fair = Razoável
strength-strong = Forte
vault-locked-error = o cofre está bloqueado
vault-current-wrong = A senha mestra atual está incorreta.
vault-current-master = Senha mestra atual
vault-master = Senha mestra
vault-new-master = Nova senha mestra
vault-repeat = Repita
vault-enter-password = Digite uma senha.
vault-too-weak = Use pelo menos 8 caracteres — algumas palavras sem relação funcionam bem.
vault-mismatch = As duas senhas não coincidem.
vault-wrong-master = Senha mestra incorreta.
vault-strength-empty = Pelo menos 8 caracteres; algumas palavras sem relação funcionam bem.
vault-never-stored = A senha mestra nunca é armazenada. Se você esquecê-la, as senhas salvas não poderão ser recuperadas.
vault-unlocking = Desbloqueando…
vault-encrypting = Criptografando…
vault-path = Cofre: { $path } (compartilhado com o comando hostwick)
vault-title-unlock = Desbloquear senhas salvas
vault-title-create = Criar um cofre de senhas
vault-title-change = Alterar a senha mestra
vault-not-now = Agora não
vault-unlock = Desbloquear
vault-create = Criar cofre
vault-change = Alterar

## Host editor

form-forward-needs-spec = `{ $token }` precisa de uma especificação de porta
form-jump-invalid = host de salto `{ $host }`: { $error }
form-port-invalid = { $field } deve ser um número de 1 a 65535
form-baud-invalid = A velocidade (baud) deve ser um número como 9600 ou 115200
form-bits-invalid = Os bits de dados devem ser de 5 a 8 e os bits de parada 1 ou 2
host-section-general = Geral
host-section-auth = Autenticação
host-section-connection = Conexão
host-section-terminal = Terminal
host-section-files = Arquivos
host-section-advanced = Avançado
host-folder-placeholder = prod/web (opcional)
host-host-placeholder = 10.0.0.5 ou host.example.com
host-user-placeholder = deploy (vazio = seu nome de usuário local)
host-keepalive-placeholder = segundos, 0 = desligado
host-optional-placeholder = opcional
host-x11-placeholder-windows = vazio = localhost:0 (VcXsrv, Xming)
host-x11-placeholder = vazio = $DISPLAY
host-script-example = Exemplo:
host-snippets-empty = Nenhum snippet para este host. Os snippets globais ficam em Configurações → Snippets.
host-log-placeholder = vazio = desligado, ou p. ex. &S-&Y&M&D-&T.log
host-sftp-server-placeholder = vazio = detectar (/usr/lib/openssh/sftp-server, …)
host-error-no-device = Escolha a porta serial (dispositivo)
host-error-no-host = Digite o nome ou o endereço do host
host-error-no-auth = Escolha pelo menos uma forma de login
host-error-forwards = Encaminhamentos de porta: { $error }
host-error-keepalive = O keepalive deve ser em segundos (0 a 3600)
host-error-proxy-host = Digite o nome do host do proxy
host-error-x11 = Display X11: { $error }
host-error-script = Script de login, { $error }
host-saved-forget-pending = Será esquecida ao salvar: { $what }
host-saved-locked = Salva no seu cofre (bloqueado): { $what }
host-saved = Salva no seu cofre: { $what }
host-saved-missing = Não está mais no seu cofre; será pedida: { $what }
host-saved-keep = Manter
host-saved-forget = Esquecer
host-what-password = senha
host-what-key-passphrase = frase secreta da chave
host-what-proxy-password = senha do proxy
host-what-sudo-password = senha do sudo
host-pick-key = Chave privada (OpenSSH ou .ppk)
host-name = Nome
host-folder = Pasta
host-protocol = Protocolo
host-host = Host
host-port = Porta
host-user = Usuário
host-notes = Observações
host-telnet-hint = O Telnet envia tudo sem criptografia; use-o para equipamentos de rede em uma rede confiável. O nome de usuário é enviado se o servidor pedir.
host-ports-looking = Procurando…
host-ports-none = Nenhuma porta encontrada
host-ports-count = Portas ({ $count })
host-ports-none-menu = Nenhuma porta serial encontrada — digite o nome do dispositivo
host-device = Porta (dispositivo)
host-ports-refresh = Procurar portas seriais novamente
host-device-hint-windows = COM3, COM4… Escolha uma ou digite o nome.
host-device-hint = /dev/ttyUSB0, /dev/ttyACM0, /dev/serial/by-id/… Escolha uma ou digite o caminho.
host-baud = Velocidade (baud)
host-data-bits = Bits de dados
host-parity = Paridade
host-parity-none = Nenhuma
host-parity-odd = Ímpar
host-parity-even = Par
host-stop-bits = Bits de parada
host-flow = Controle de fluxo
host-flow-none = Nenhum
host-serial-none-linux = Nenhuma porta encontrada. Adaptadores USB aparecem como /dev/ttyUSB*; talvez você precise estar no grupo dialout.
host-serial-hint = A maioria dos consoles usa 9600 8N1 ou 115200 8N1 sem controle de fluxo.
host-auth-not-ssh = Conexões Telnet e seriais fazem login dentro do terminal. Use um script de login (Terminal) para automatizar.
host-key-files = Arquivos de chave
host-keys-hint = Chaves OpenSSH ou PuTTY .ppk, separadas por vírgulas. Um certificado ao lado de uma chave (id_ed25519-cert.pub) é usado automaticamente. Chaves de segurança (sk-…) funcionam pelo agente.
host-auth-agent = Chaves no agente SSH (Pageant, ssh-agent, 1Password…)
host-auth-password = Senha
host-auth-remember-hint = Marque “Lembrar senha” quando for solicitada para entrar automaticamente da próxima vez.
host-auth-kbd = Interativo pelo teclado (códigos de uso único, MFA)
host-auth-kerberos = Kerberos (login único)
host-auth-kerberos-forward = Encaminhar meu tíquete Kerberos ao servidor
host-auth-kerberos-hint-windows = Usa seu login de domínio do Windows.
host-auth-kerberos-hint = Usa o tíquete do kinit (veja klist). Sem ele, o próximo método é tentado.
host-auth-order = Os métodos são tentados nesta ordem: Kerberos, agente, arquivos de chave, senha, interativo pelo teclado.
host-connection-serial = Portas seriais são locais: sem proxy, hosts de salto ou encaminhamentos.
host-proxy = Proxy
host-proxy-none = Nenhum
host-proxy-host-port = Host / porta do proxy
host-proxy-port = Porta do proxy
host-proxy-user = Usuário do proxy
host-proxy-password-hint = A senha do proxy é pedida ao conectar (marque Lembrar para salvá-la).
host-jump-hosts = Hosts de salto
host-jump-hint = Separados por vírgulas, o mais externo primeiro (como ssh -J).
host-forwards = Encaminhamentos de porta
host-forwards-hint = L = local → remoto, R = remoto → local, D = proxy SOCKS. Iniciados com o terminal.
host-keepalive = Keepalive (s)
host-agent-forwarding = Encaminhar meu agente SSH (o git no servidor usa minhas chaves)
host-agent-forwarding-hint = Somente em servidores confiáveis: os administradores deles podem usar suas chaves enquanto você estiver conectado.
host-x11 = Encaminhar X11 (mostrar aqui programas gráficos remotos)
host-x11-display = Display X
host-x11-hint-windows = Precisa de um servidor X em execução, como VcXsrv ou Xming.
host-x11-hint-macos = Precisa do XQuartz.
host-x11-hint = Usa o servidor X da sua área de trabalho (ou XWayland).
host-terminal-type = Tipo de terminal
host-login-script = Script de login
host-login-script-hint = Um comando por linha, digitado após o login. @wait TEXTO espera essa saída (10 s), @wait 30000 TEXTO espera mais, @sleep 500 pausa; # inicia um comentário.
host-shell-integration = Integração com o shell (bash, zsh)
host-shell-integration-hint = Permite que o navegador de arquivos acompanhe a pasta do terminal (botão direito → Abrir arquivos aqui). Adiciona discretamente um hook no prompt quando o shell inicia; nada é instalado no servidor.
host-snippets-title = Snippets deste host
host-session-log = Log da sessão
host-session-log-hint = Marcadores: &S sessão, &H host, &P porta, &U usuário, &Y&M&D data, &T hora. Caminhos relativos vão para a pasta de logs.
host-raw-log = Registrar a saída bruta (com sequências de escape)
host-files-not-ssh = O navegador de arquivos precisa de SSH.
host-file-protocol = Protocolo de arquivos
host-file-protocol-auto = Automático
host-file-protocol-hint = Automático usa SFTP e recorre ao SCP em servidores sem SFTP (roteadores, ESXi, busybox).
host-sudo = Abrir o navegador de arquivos como root (sudo)
host-sudo-user = Como usuário
host-sudo-hint = Funciona com sudo sem senha ou com senha do sudo (pedida ou salva). “Reabrir como root” no navegador alterna a qualquer momento.
host-legacy = Permitir algoritmos legados (ssh-rsa/SHA-1, troca de chaves antiga) para dispositivos antigos
host-legacy-hint = Somente para switches, roteadores e appliances antigos que precisam deles.
host-id = ID do host: { $id }
host-title-edit = Editar host
host-title-new = Novo host
loading = Carregando…

## Settings

settings-title = Configurações
settings-section-appearance = Aparência
settings-section-terminal = Terminal
settings-section-snippets = Snippets
settings-section-connections = Conexões
settings-section-files = Arquivos
settings-section-logging = Logs de sessão
settings-section-passwords = Senhas salvas
settings-section-sync = Sincronização das configurações
settings-section-diagnostics = Diagnóstico
settings-language = Idioma
settings-language-system = Sistema ({ $language })
settings-language-hint = Vale assim que você salvar. Diálogos abertos mantêm o idioma até serem abertos de novo.
settings-theme = Tema
settings-theme-dark = Escuro
settings-theme-light = Claro
settings-terminal-font = Fonte do terminal
settings-font-size = Tamanho da fonte
settings-font-placeholder = vazio = fonte monoespaçada do sistema
settings-font-hint = Alterações de fonte valem para os terminais abertos ao salvar.
settings-editor-placeholder = vazio = aplicativo padrão do sistema (p. ex. code -w { "{file}" })
settings-auto-lock-placeholder = 0 = manter desbloqueado até fechar o Hostwick
settings-sync-folder-placeholder = Pasta do OneDrive, Dropbox, iCloud Drive ou git
settings-sync-pass-saved = Salva no seu cofre — digite para alterar
settings-sync-pass-placeholder = A mesma frase secreta em todos os computadores
settings-snippets-empty = Ainda não há snippets. Eles aparecem como botões acima de cada terminal e em botão direito → Snippets.
settings-remove-failed = Não foi possível remover: { $error }
settings-error-font-size = Terminal → O tamanho da fonte deve ser um número de 6 a 48
settings-error-scrollback = Terminal → O histórico deve ser um número inteiro de 100 a 1000000
settings-error-auto-lock = Senhas salvas → Bloquear após deve ser em minutos (0 a 1440)
settings-error-rules = Terminal → { $error }
settings-error-sync-folder = Sincronização das configurações → a pasta { $folder } não existe
settings-folder-missing = A pasta { $folder } não existe.
settings-sync-pass-short = Use uma frase de sincronização com pelo menos 8 caracteres.
settings-sync-pass-save-failed = Não foi possível salvar a frase secreta: { $error }
vault-reason-sync-passphrase = A frase de sincronização fica guardada no seu cofre de senhas.
settings-save-failed = Não foi possível salvar as configurações: { $error }
settings-scrollback = Linhas de histórico
settings-right-click = Botão direito
settings-right-click-menu = Mostrar menu
settings-right-click-paste = Colar (PuTTY)
settings-copy-on-select = Copiar automaticamente o texto selecionado
settings-mouse = Enviar cliques e rolagem do mouse aos programas (htop, mc, vim)
settings-mouse-hint = Segure Shift para selecionar texto enquanto um programa usa o mouse.
settings-button-bar = Mostrar a barra de snippets acima dos terminais
settings-highlighting = Destaque de palavras-chave
settings-highlighting-hint = Colore o texto correspondente na tela. Regras anteriores prevalecem quando se sobrepõem.
settings-snippets-hint = Disponíveis em todos os terminais. Os hosts podem ter os seus em Editar host → Terminal.
settings-auto-reconnect = Reconectar automaticamente quando a conexão cair
settings-auto-reconnect-hint = SSH e Telnet: após 1, 2, 5, 10 e depois a cada 30 segundos, mantendo o histórico. Não depois de você digitar exit, nem após falha de login ou de chave do host.
settings-restore-tabs = Reabrir minhas abas ao iniciar o Hostwick
settings-restore-tabs-hint = Incluindo painéis divididos e seus tamanhos. As abas se conectam quando você as abre pela primeira vez.
settings-editor = Editor externo
settings-editor-pick = Programa editor
settings-editor-hint = Usado por Editar (F4). { "{file}" } é trocado pelo caminho do arquivo; caso contrário, o caminho é adicionado no final. Use a opção de espera do editor (code -w, subl -w), se houver.
settings-log-all = Registrar cada sessão em um arquivo
settings-log-folder = Pasta de logs
settings-log-content = Conteúdo do log
settings-log-printable = Texto imprimível
settings-log-raw = Bruto (com sequências de escape)
settings-log-hint = A configuração de log própria do host (Editar host → Terminal) tem prioridade.
settings-log-level = Nível de log do aplicativo
settings-open-log-folder = Abrir pasta de logs
settings-data-folder = Pasta de dados: { $path }
settings-vault-none = Ainda não há cofre. Marque “Lembrar” quando o Hostwick pedir uma senha, ou crie um agora.
settings-vault-locked = Bloqueado. As senhas salvas são usadas depois que você as desbloqueia.
settings-vault-unlocked = Desbloqueado nesta sessão.
settings-vault-unlock = Desbloquear…
settings-vault-create = Criar cofre…
settings-vault-lock = Bloquear agora
settings-vault-change = Alterar a senha mestra…
settings-vault-empty = Nada salvo ainda.
settings-vault-remove = Remover
settings-auto-lock = Bloquear após (min)
settings-auto-lock-hint = Minutos ociosos até as senhas salvas serem bloqueadas de novo; 0 as mantém desbloqueadas até fechar o Hostwick.
settings-vault-file = Criptografado com Argon2id + XChaCha20-Poly1305 em { $path } — o mesmo cofre do `hostwick vault`. Nunca é sincronizado.
settings-sync-never = Nunca sincronizado neste computador.
settings-sync-intro = Hosts e configurações vão para um arquivo criptografado em uma pasta que sua nuvem ou o git mantém atualizada. As senhas salvas nunca saem deste computador.
settings-sync-folder = Pasta
settings-sync-folder-pick = Pasta de sincronização
settings-sync-passphrase = Frase secreta
settings-sync-passphrase-hint = Guardada no seu cofre de senhas. Todos os computadores precisam da mesma.
settings-sync-auto = Sincronizar ao iniciar, após alterações e a cada 5 minutos
settings-sync-now = Sincronizar agora
settings-sync-push = Substituir arquivo
settings-sync-push-tooltip = Fazer o arquivo de sincronização igual a este computador (também troca a frase secreta dele)
settings-sync-pull = Substituir este computador
settings-sync-pull-tooltip = Fazer os hosts e as configurações deste computador iguais ao arquivo de sincronização
settings-sync-last = Última sincronização

## Snippet and highlight-rule editors

snippet-label-placeholder = Rótulo do botão
snippet-text-placeholder = Texto a enviar (várias linhas são coladas como um bloco)
snippet-press-enter = Pressionar Enter
snippet-move-up = Mover para cima
snippet-move-down = Mover para baixo
editor-remove = Remover
snippet-add = Adicionar snippet
rule-error-empty = padrão vazio
rule-error-color = a cor deve ter o formato #ff8800, não `{ $color }`
rule-error-regex = expressão regular inválida
rule-pattern-placeholder = palavra ou expressão regular
rule-error = Destaque `{ $pattern }`: { $error }
rule-enabled = Ativada
rule-regex-on = Expressão regular (clique para usar uma palavra simples)
rule-regex-off = Palavra simples, sem diferenciar maiúsculas (clique para usar uma expressão regular)
rule-add = Adicionar regra
rule-defaults = Restaurar padrões

## Terminal

script-timeout = esperou { $after } por “{ $text }”, que nunca apareceu
script-session-ended = a sessão terminou antes
search-too-complex = padrão complexo demais
term-exit-status = encerrado com status { $code }
term-exit-signal = encerrado pelo sinal { $signal }
term-connection-lost = conexão perdida
term-connection-closed = conexão encerrada
term-reconnected-marker = reconectado às { $time }
term-reconnect-failed = falha ao reconectar: { $error }
term-break-failed = Não foi possível enviar o break
term-no-break = Sessões { $protocol } não têm sinal de break
term-script-not-run = Script de login não executado: { $error }
term-script-stopped = Script interrompido: { $error }
search-placeholder = Localizar no histórico
search-no-results = Nenhum resultado
search-position = { $index } de { $total }
search-matches = { $count ->
    [one] { $count } ocorrência
   *[other] { $count } ocorrências
}
search-match-case = Diferenciar maiúsculas
search-regex = Expressão regular
search-older = Ocorrência anterior (mais antiga) — Enter, F3
search-newer = Próxima ocorrência (mais recente) — Shift+Enter, Shift+F3
search-close = Fechar (Esc)
term-reconnect-in = Conexão perdida{ $reason }. Reconectando em { $seconds } s…
term-reconnect-attempt = Reconectando (tentativa { $attempt })…
term-log-badge = LOG
term-reconnect-now = Agora
term-session-ended = Sessão encerrada — { $reason }. Pressione Enter para reconectar.
term-menu-copy = Copiar
term-menu-paste = Colar
term-menu-select-all = Selecionar tudo
term-menu-find = Localizar…
term-menu-snippets = Snippets
term-menu-copy-folder = Copiar a pasta atual
term-menu-stop-script = Parar script
term-menu-clear = Limpar histórico
term-menu-stop-log = Parar o log da sessão
term-menu-start-log = Registrar a sessão em arquivo…
term-menu-files-here = Abrir arquivos aqui
term-menu-duplicate = Duplicar sessão
term-menu-reconnect-ended = Reconectar (sessão encerrada)

## File browser

files-move = Mover
files-upload = Enviar
files-download = Baixar
files-move-confirm = { $count ->
    [one] Mover “{ $name }” para { $dir }?
   *[other] Mover { $count } itens para { $dir }?
}
files-upload-confirm = { $count ->
    [one] Enviar “{ $name }” para { $dir }?
   *[other] Enviar { $count } itens para { $dir }?
}
files-download-confirm = { $count ->
    [one] Baixar “{ $name }” para { $dir }?
   *[other] Baixar { $count } itens para { $dir }?
}
files-same-place = { $name }: a origem e o destino são o mesmo
files-skipped-unsafe = Não exibidos porque os nomes não são seguros neste computador: { $names }
files-not-removed = { $name }: copiado, mas não removido: { $error }
files-pick-download = Baixar aqui
files-exists = “{ $name }” já existe
files-delete-title = Excluir
files-delete-folder-confirm = Excluir permanentemente a pasta “{ $name }” e tudo o que ela contém?
files-delete-file-confirm = Excluir permanentemente o arquivo “{ $name }”?
files-delete-many-confirm = Excluir permanentemente { $count } itens (pastas com todo o conteúdo)?
files-name-invalid = Digite um nome sem “/”
files-new-folder = Nova pasta
files-create = Criar
files-folder-name = Nome da pasta
files-rename = Renomear
files-new-name = Novo nome
files-new-symlink = Novo link simbólico
files-symlink-placeholder = destino -> nome do link
files-symlink-format = Escreva como “destino -> nome do link”
files-symlink-empty = O destino está vazio
edit-conflict-title = Alterado no servidor
edit-conflict-body = “{ $name }” foi alterado no servidor depois que você o abriu. O que deve acontecer com a sua versão salva?
edit-conflict-later = Decidir depois
edit-conflict-theirs = Baixar a do servidor
edit-conflict-copy = Salvar como cópia
edit-conflict-overwrite = Sobrescrever
edit-editor-failed = não foi possível iniciar o editor “{ $program }”: { $error }
keep-up-to-date = atualizado
keep-synced = { $count ->
    [one] { $count } arquivo sincronizado, { $size }
   *[other] { $count } arquivos sincronizados, { $size }
}
keep-failed = { $count } com falha
keep-error = erro (tentando novamente): { $error }
keep-starting = iniciando…
keep-status = Mantendo { $local } → { $remote } atualizado: { $status }
edits = Edições
edits-busy = Edições ({ $count }): { $status }
edits-tooltip = Arquivos abertos em um editor; cada salvamento é enviado automaticamente
edits-stop = Parar de monitorar
edits-stop-all = Parar de monitorar todos
files-hide-local = Ocultar local
files-show-local = Mostrar local
files-local-tooltip = Mostrar ou ocultar os arquivos do computador local
files-upload-pick = Enviar…
files-synchronize = Sincronizar…
files-synchronize-tooltip = Comparar pastas, revisar as mudanças e então aplicá-las
files-keep-stop = Parar de manter atualizado
files-keep = Manter atualizado…
files-keep-tooltip = Enviar automaticamente as mudanças de uma pasta local para uma pasta remota
files-following = Acompanhando o terminal
files-follow = Acompanhar o terminal
files-follow-tooltip = Mostrar a pasta em que o terminal “{ $terminal }” está, conforme ela muda
files-root-switching = Alternando…
files-root-on = Root: ligado
files-root-reopen = Reabrir como root
files-root-tooltip = Navegar no servidor como root (ou como o usuário sudo do host) via sudo
files-path-placeholder = /caminho
files-this-computer = Este computador
files-selected-of = { $selected } de { $count } selecionados ({ $size })
files-item-count = { $count ->
    [one] { $count } item
   *[other] { $count } itens
}
files-selected = { $count ->
    [one] { $count } item
   *[other] { $count } itens
}
files-root-badge = ROOT
files-up-tooltip = Acima (Backspace)
files-refresh-tooltip = Atualizar (Ctrl+R)
files-mkdir-tooltip = Nova pasta (F7)
files-col-name = Nome
files-col-size = Tamanho
files-col-modified = Modificado
files-col-mode = Permissões
files-download-pick = Baixar…
files-menu-open = Abrir
files-menu-edit = Editar
files-menu-move = Mover para o outro lado
files-menu-rename = Renomear…
files-menu-new-folder = Nova pasta…
files-menu-new-symlink = Novo link simbólico…
files-menu-copy-path = Copiar caminho
files-menu-refresh = Atualizar
files-menu-properties = Propriedades
edit-status-downloading = baixando…
edit-status-watching = monitorando
edit-status-uploading = enviando…
edit-status-uploaded = enviado às { $time }
edit-status-uploaded-in-place = enviado às { $time } (regravado no lugar)
edit-status-conflict = alterado no servidor — aguardando você
edit-status-error = erro: { $error }
edit-gone = { $path }: não existe mais
edit-kept-folder = Os arquivos editados que não foram enviados foram mantidos em { $path }
edit-is-folder = { $path } é uma pasta
edit-missing-after-upload = { $path }: ausente após o envio
edit-size-mismatch = { $path }: o servidor tem { $server } bytes após enviar { $uploaded }
edit-no-copy-name = nenhum nome livre para a cópia

## File properties

props-title = Propriedades
props-applying = Aplicando…
props-apply = Aplicar
props-calculating = calculando…
props-error = erro: { $error }
props-usage = { $size } ({ $bytes } bytes) em { $files ->
    [one] { $files } arquivo
   *[other] { $files } arquivos
}, { $folders ->
    [one] { $folders } pasta
   *[other] { $folders } pastas
}
props-usage-partial = (interrompido antes: entradas demais)
props-bad-id = “{ $id }” não é um ID numérico
props-bad-octal = As permissões devem ser em octal, p. ex. 0644
props-kind-folder = Pasta
props-kind-file = Arquivo
props-kind-symlink = Link simbólico → { $target }
props-kind-special = Arquivo especial
props-unknown-name = (nome desconhecido)
props-not-reported = não informado pelo servidor
props-read = Leitura
props-write = Gravação
props-execute = Execução
props-owner = Dono
props-group = Grupo
props-others = Outros
props-special = Especiais
props-octal = Octal
props-calculate = Calcular
props-size = { $size } ({ $bytes } bytes)
props-location = Local
props-type = Tipo
props-checksum = Soma de verificação
props-unknown = desconhecido
props-permissions = Permissões
props-recursive = Aplicar a todo o conteúdo (arquivos mantêm seus bits de execução)

## Synchronize dialog

files-synchronize-title = Sincronizar
sync-keep = Manter o remoto atualizado
sync-keep-tooltip = Monitorar a pasta local e enviar cada mudança (somente local → remoto)
sync-comparing = Comparando…
sync-preview = Visualizar
sync-run-checked = Sincronizar os marcados
sync-exclude-placeholder = p. ex. .git, *.tmp, node_modules
sync-choose-folders = Escolha as duas pastas
sync-use-local = Usar o local
sync-use-remote = Usar o remoto
sync-total-uploads = { $count ->
    [one] { $count } envio, { $size }
   *[other] { $count } envios, { $size }
}
sync-total-downloads = { $count ->
    [one] { $count } download, { $size }
   *[other] { $count } downloads, { $size }
}
sync-total-deletions = { $count ->
    [one] { $count } exclusão
   *[other] { $count } exclusões
}
sync-total-conflicts = { $count ->
    [one] { $count } conflito — escolha um lado ou deixe-o
   *[other] { $count } conflitos — escolha um lado ou deixe-os
}
sync-in-sync = As duas pastas já estão sincronizadas.
sync-checked = Marcados: { $items }
sync-local-folder = Pasta local
sync-remote-folder = Pasta remota
sync-direction = Direção
sync-dir-up = Local → remoto
sync-dir-down = Remoto → local
sync-dir-both = Ambos (o mais novo vence)
sync-mirror = Excluir arquivos ausentes na origem (espelho)
sync-exclude = Excluir da sincronização
sync-comparing-folders = Comparando pastas…
sync-result = Sincronizar: { $summary }
sync-running = Sincronizando…
sync-run-failed = Falha ao sincronizar: { $error }
sync-kind-upload = Enviar
sync-kind-download = Baixar
sync-kind-delete-local = Excluir local
sync-kind-delete-remote = Excluir remoto
sync-kind-conflict = Conflito
sync-kind-skip = Ignorar
sync-reason-new = novo
sync-reason-newer = mais novo
sync-reason-size = tamanho diferente
sync-reason-differs = diferente
sync-reason-not-in-source = não está na origem
sync-reason-both-changed = alterado nos dois lados (mesma hora, tamanho diferente)
sync-reason-unknown = impossível comparar (sem data/hora)
sync-reason-destination-newer = o destino é mais novo
sync-reason-new-folder = pasta nova
sync-reason-case-conflict = outro arquivo difere só em maiúsculas e minúsculas
sync-review-deletions = Esta sincronização exclui arquivos. Confira a lista abaixo e clique em Sincronizar de novo.
sync-report-uploaded = { $count } enviados
sync-report-downloaded = { $count } baixados
sync-report-deleted = { $count } excluídos
sync-report-folders = { $count ->
    [one] { $count } pasta criada
   *[other] { $count } pastas criadas
}
sync-report-failed = { $count } com falha
sync-report-nothing = nada a fazer

## Key tools

keyformat-openssh-hint = Para OpenSSH, Hostwick, Git e a maioria das ferramentas (um arquivo .pub também é gravado).
keyformat-ppk3-hint = Para PuTTY 0.75+ e WinSCP (protegido com Argon2).
keyformat-ppk2-hint = Para versões antigas do PuTTY, WinSCP e FileZilla.
key-fingerprint = Impressão digital: { $fingerprint }
keygen-title = Gerar chave
keygen-generate = Gerar
keygen-passphrase-placeholder = Frase secreta (recomendada; vazio = nenhuma)
keygen-repeat = Repetir
keygen-mismatch = As frases secretas não coincidem.
keygen-no-path = Escolha onde salvar a chave.
keygen-exists = { $path } já existe. Escolha outro nome ou marque “Sobrescrever”.
keygen-used-in-host = { $name } agora faz login com { $path }. Adicione a chave pública ao ~/.ssh/authorized_keys do servidor.
keygen-ed25519-recommended = Ed25519 (recomendada)
keygen-saved = { $file } (chave privada) e { $file }.pub salvos em { $dir }
keygen-public-key = Chave pública (para ~/.ssh/authorized_keys):
keygen-copy-public = Copiar chave pública
keygen-use-in-host = Usar em um host…
keygen-type = Tipo
keygen-comment = Comentário
keygen-format = Formato
keygen-save-as = Salvar como
keygen-overwrite = Sobrescrever um arquivo existente
keygen-generating-rsa = Gerando… RSA 4096 pode levar alguns segundos.
keygen-generating = Gerando…
keyconv-title = Inspecionar ou converter chave
keyconv-path-placeholder = ~/.ssh/id_ed25519 ou C:\keys\work.ppk
keyconv-current-placeholder = Frase secreta atual (se houver)
keyconv-new-placeholder = Nova frase secreta (vazio = nenhuma)
keyconv-repeat-placeholder = Repetir a nova frase secreta
keyconv-output = Arquivo de saída
keyconv-mismatch = As novas frases secretas não coincidem.
keyconv-pass-changed = Frase secreta alterada ({ $format }).
keyconv-pass-removed = Frase secreta removida ({ $format }).
keyconv-wrote = { $path } gravado ({ $format }).
keyconv-needs-passphrase = Esta chave tem frase secreta: digite-a em “Frase secreta atual”.
keyconv-wrong-passphrase = Frase secreta incorreta.
keyconv-exists = { $path } já existe; escolha outro arquivo de saída.
keyconv-key-file = Arquivo de chave
keyconv-type-bits = { $kind } de { $bits } bits ({ $algorithm })
keyconv-protected = Protegida
keyconv-public-only = somente chave pública
keyconv-encrypted = sim, com frase secreta
keyconv-no-passphrase = sem frase secreta
keyconv-fingerprint = Impressão digital
keyconv-inspect = Inspecionar
keyconv-current = Atual
keyconv-new = Nova frase secreta
keyconv-change = Alterar frase secreta
keyconv-convert-to = Converter para
keyconv-convert = Converter
keyconv-keeps-passphrase = Mantém a frase secreta atual, a menos que você digite uma nova.
keyconv-working = Processando…
kh-title = Hosts conhecidos
kh-search-placeholder = Pesquisar host, tipo de chave ou impressão digital
kh-forget-placeholder = host ou host:porta
kh-nothing-matched = Nada correspondeu; o arquivo não mudou.
kh-removed = { $count ->
    [one] { $count } chave removida.
   *[other] { $count } chaves removidas.
}
kh-type-host = Digite um nome de host ou host:porta.
kh-col-line = Linha
kh-col-hosts = Host(s)
kh-empty = Ainda não há hosts conhecidos. As chaves são adicionadas quando você confia em um servidor.
kh-no-match = Nada corresponde à pesquisa.
kh-reload = Recarregar
kh-remove-selected = Remover selecionados ({ $count })
kh-forget-host = Esquecer host
kh-forget-button = Remover todas as chaves dele
kh-footer = { $path } — { $shown } de { $total } exibidos. Nomes com hash também são considerados quando você esquece um host pelo nome.
files-opening-root = abrindo arquivos como root
files-opening = abrindo o navegador de arquivos

## Keyboard and screen readers

sidebar-vault-lock = Bloquear senhas salvas
sidebar-quick-connect = Conexão rápida (user@host:port, Enter)
sidebar-hosts-label = Hosts salvos
sidebar-hosts-keys = As setas escolhem um host. Enter abre um terminal, Shift+Enter os arquivos, F2 edita, Delete exclui, Shift+F10 abre o menu.
tabs-label = Sessões abertas
tabs-keys = Esquerda e Direita trocam de aba, Enter entra na aba, Delete a fecha, Shift+F10 abre o menu dela. Ctrl+F6 alterna entre a lista de hosts, as abas e a sessão.
tab-state-not-connected = ainda não conectada
tab-state-connected = conectada
tab-state-disconnected = desconectada
tab-spoken = { $title }, { $state }, aba { $index } de { $count }
tab-close = Fechar { $title }
pane-divider = Divisor de painéis

## Proxy test

vault-reason-proxy-test = Desbloqueie suas senhas salvas para usar a senha do proxy salva.
proxy-test-closed = O proxy conectou, mas { $target } fechou a conexão imediatamente.
proxy-test-ok-ssh = Conectado a { $target } pelo proxy. O servidor diz: { $server }
proxy-test-ok = Conectado a { $target } pelo proxy (o servidor não é um servidor SSH).
proxy-test-ok-quiet = Conectado a { $target } pelo proxy (o servidor ainda não enviou nada).
proxy-test-read-failed = Conectado a { $target } pelo proxy e então: { $error }
host-proxy-test = Testar conexão
host-proxy-testing = Testando…
host-proxy-test-tooltip = Conectar pelo proxy à porta deste host (ou do primeiro host de salto) e informar o resultado
host-baud-common = Velocidades comuns
dismiss = Dispensar
term-no-output = Ainda não há saída.
term-text-view-title = Saída do terminal: { $title }
term-text-view-hint = { $lines ->
    [one] { $lines } linha. Leia com as setas; Esc volta ao terminal.
   *[other] { $lines } linhas. Leia com as setas; Esc volta ao terminal.
}
term-a11y-label = Terminal: { $title }
term-a11y-keys = A nova saída é lida quando se estabiliza. { $read } lê a última saída de novo, { $view } mostra toda a saída como texto. Ctrl+F6 sai do terminal.
term-menu-read-output = Ler a última saída
term-menu-text-view = Mostrar a saída como texto…
files-path-label = Pasta: { $side }
files-list-label = { $side }: { $dir }
files-list-keys = As setas movem, Shift+setas amplia a seleção, Espaço adiciona ou remove uma linha, Enter abre, Backspace sobe, F5 copia para o outro lado, F6 move, F2 renomeia, Delete exclui, Alt+Enter mostra as propriedades, Tab troca de lado, Shift+F10 abre o menu.
files-kind-link = link
a11y-sections = Seções
a11y-section-problem = { $section } (precisa de atenção)
kh-row-label = Linha { $line }: { $hosts }
rule-color = Cor (#rrggbb)
toast-info = Informação
toast-success = Concluído
toast-warning = Aviso
toast-error = Erro
toast-close = Fechar notificação
