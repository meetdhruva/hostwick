# Hostwick desktop：简体中文。
#
# 译自 en.ftl（源文件）。键名和变量（{ $name }）保持不变；产品和协议名称
# （SSH、SFTP、SCP、sudo、Kerberos、PuTTY、WinSCP、X11、SOCKS）不翻译。

## General

error-internal = 内部错误：{ $error }

## Main window

sidebar-search-placeholder = 搜索主机
split-only-terminals = 只能拆分终端标签页。
files-follow-no-folder = 终端尚未报告其所在文件夹。请在“编辑主机 → 终端”中开启“Shell 集成”，文件浏览器才能跟随终端（bash 和 zsh）。
host-copy-name = { $name }（副本）
size-b = B
size-kib = KiB
size-mib = MiB
size-gib = GiB
size-tib = TiB
protocol-ssh = SSH
protocol-telnet = Telnet
protocol-serial = 串口
sidebar-vault-unlocked-tooltip = 已保存的密码已解锁 — 点击锁定
sidebar-settings-tooltip = 设置（{ $keys }）
sidebar-open-terminal = 终端
sidebar-open-files = 文件
sidebar-edit-host = 编辑主机
sidebar-delete-host = 删除主机
sidebar-new-host = 新建主机…
tab-files-title = 文件：{ $name }
broadcast = 广播
broadcast-on = 广播：开
broadcast-tooltip = 将按键发送到所有打开的终端（{ $keys }）
shortcut-new-host = 新建主机
shortcut-duplicate-tab = 复制标签页
shortcut-close-pane = 关闭窗格/标签页
shortcut-reconnect = 重新连接
shortcut-next-tab = 下一个标签页
shortcut-split-right = 向右拆分
shortcut-split-down = 向下拆分
shortcut-move-between-panes = 在窗格间切换
shortcut-broadcast = 广播
shortcut-find = 查找
shortcut-settings = 设置
empty-title = 没有打开的会话
empty-hint-connect = 双击一个主机，或在快速连接框中输入 user@host。
empty-hint-menus = 右键单击主机、标签页、文件和终端可执行更多操作。
empty-hint-import = 使用左下角的“导入…”从 ~/.ssh/config、PuTTY、WinSCP 或 Solar-PuTTY 导入。
pane-connecting = 正在连接到 { $host }…
pane-retry = 重试
transfer-progress = { $done } / { $total } — { $rate }/s
transfer-done = 完成 — { $size }
transfer-skipped = 已跳过
transfer-cancelled = 已取消
transfer-failed = 失败：{ $error }
transfer-cancel = 取消
transfers-title = 传输 — { $count } 个进行中
transfers-clear = 清除已完成

## Common buttons

button-cancel = 取消
button-delete = 删除
button-ok = 确定
button-save = 保存
button-close = 关闭
button-browse = 浏览…

## Menus, import

host-delete-title = 删除主机
host-delete-confirm = 要删除已保存的主机“{ $name }”吗？
sidebar-import = 导入…
sidebar-tools = 工具
import-no-home = 没有主目录
import-pick-putty = PuTTY .reg 导出文件
import-pick-solar = Solar-PuTTY 导出文件（.json）
import-pick-winscp = WinSCP.ini
import-failed = 导入失败：{ $error }
import-summary = 新增 { $added } 个，更新 { $updated } 个，{ $notes } 个有提示
import-done = 已导入：{ $summary }
import-report-title = 导入报告
import-report-attention = 需要注意（{ $count }）
import-report-added = 已新增（{ $count }）
import-report-updated = 已更新（{ $count }）
import-report-note = “需要注意”下列出的主机仍已导入，除非提示说明已跳过。右键单击主机 → 编辑… 可以调整。
import-menu-ssh-config = OpenSSH ~/.ssh/config
import-menu-putty-registry = PuTTY（此电脑上保存的会话）
import-menu-winscp-registry = WinSCP（此电脑上保存的站点）
import-menu-putty-file = PuTTY .reg 导出文件…
import-menu-winscp-file = WinSCP.ini 文件…
import-menu-solar-file = Solar-PuTTY .json 导出文件…
menu-open-terminal = 打开终端
menu-open-files = 打开文件（SFTP）
menu-edit = 编辑…
menu-duplicate = 复制
menu-copy-address = 复制地址
menu-delete = 删除…
menu-reconnect = 重新连接
menu-split-right = 向右拆分
menu-split-down = 向下拆分
menu-send-break = 发送 Break
menu-follow-terminal = 跟随终端文件夹
menu-close = 关闭
menu-close-others = 关闭其他
menu-close-all = 全部关闭
tools-generate-key = 生成密钥…
tools-inspect-key = 查看或转换密钥…
tools-known-hosts = 已知主机…
tools-saved-passwords = 已保存的密码…
tools-settings-sync = 设置同步…
button-continue = 继续

## Connection prompts

notice-connection-title = 连接
hostkey-changed-title = 主机密钥已更改！
hostkey-unknown-title = 未知主机
hostkey-changed-body = 警告：{ $host } 的主机密钥已更改。

    这可能意味着有人正在拦截连接（中间人攻击），也可能是服务器重新安装过。

    新的 { $algorithm } 密钥：
    { $fingerprint }

    只有在确认密钥是有意更改时才继续。
hostkey-unknown-body = 这是首次连接到 { $host }。

    { $algorithm } 密钥指纹：
    { $fingerprint }

    信任之前，请先向服务器管理员核实。
hostkey-reject = 拒绝
hostkey-trust-once = 仅信任一次
hostkey-replace = 替换已保存的密钥
hostkey-trust-remember = 信任并记住
remember-hint-unlocked = 将加密保存在你的密码库中。
remember-hint-locked = 将要求输入密码库的主密码。
remember-hint-create = 你将创建一个用主密码保护的密码库。
prompt-server-auth-title = 服务器身份验证
prompt-password-title = { $user }@{ $host } 的密码
prompt-password-label = 密码
prompt-passphrase-title = { $path } 的密码短语
prompt-passphrase-label = 密码短语
prompt-sudo-title = { $who } 的 sudo 密码
prompt-sudo-wrong-title = 密码错误。{ $who } 的 sudo 密码
secret-rejected-password = 已保存的密码未被接受。请输入当前密码。
secret-rejected-proxy = 已保存的代理密码未被接受。请输入当前密码。
secret-rejected-passphrase = 已保存的密码短语未被接受。请输入当前的密码短语。
secret-rejected-sudo = sudo 未接受已保存的密码。
secret-update-password = 更新已保存的密码
secret-update-proxy = 更新已保存的代理密码
secret-update-passphrase = 更新已保存的密码短语
secret-update-sudo = 更新已保存的 sudo 密码
secret-remember-password = 记住密码
secret-remember-proxy = 记住代理密码
secret-remember-passphrase = 记住密码短语
secret-remember-sudo = 记住 sudo 密码
vault-reason-host = { $name } 有已保存的密码。请用主密码解锁。
vault-reason-store = 请解锁已保存的密码以保存此密码。
vault-reason-create = 为已保存的密码创建一个密码库。它们由一个主密码保护，主密码本身从不存储。
notice-not-saved-locked = 未保存：密码库已锁定。
notice-save-password-failed = 无法保存密码：{ $error }
notice-update-host-failed = 无法更新 { $name }：{ $error }
files-need-ssh = 文件功能需要 SSH：{ $name } 是 { $protocol } 连接，无法传输文件。
files-root-failed = 无法以 root 身份打开：{ $error }

## Settings sync

sync-no-changes = 此处没有变化
sync-added = 新增 { $count } 个
sync-updated = 更新 { $count } 个
sync-removed = 删除 { $count } 个
sync-settings-updated = 设置已更新
sync-mode-sync = 同步
sync-mode-push = 推送
sync-mode-pull = 拉取
sync-summary = { $mode }：{ $count } 个主机已同步；{ $changes }
sync-file-updated = 同步文件已更新
vault-reason-sync-startup = 请解锁已保存的密码以同步设置（同步密码短语保存在其中）。
vault-reason-sync = 请解锁已保存的密码以同步设置。
sync-no-folder = 请先在“设置 → 设置同步”中选择同步文件夹。
sync-no-passphrase = 未保存同步密码短语 — 请在“设置 → 设置同步”中设置
sync-passphrase-title = 设置同步密码短语
sync-passphrase-label = 同步密码短语
sync-passphrase-wrong-title = 同步密码短语错误 — 请输入其他电脑使用的那一个
sync-wrong-passphrase = 同步密码短语错误
sync-last-failed = 失败：{ $error }
sync-done = 设置同步 — { $summary }
sync-failed = 设置同步失败：{ $error }

## Password vault

cred-sync-passphrase = 设置同步密码短语
cred-login-password = 登录密码
cred-sudo-password = sudo 密码
cred-proxy-password = 代理密码
cred-key-passphrase = { $file } 的密码短语
cred-key-unused = 密钥密码短语 — { $path }（没有主机使用）
cred-host-deleted = 未使用（其主机已删除）
strength-too-short = 太短 — 至少使用 8 个字符
strength-strong-passphrase = 强密码短语
strength-weak = 较弱 — 越长越好（几个单词效果很好）
strength-fair = 一般
strength-strong = 强
vault-locked-error = 密码库已锁定
vault-current-wrong = 当前主密码错误。
vault-current-master = 当前主密码
vault-master = 主密码
vault-new-master = 新主密码
vault-repeat = 再次输入
vault-enter-password = 请输入密码。
vault-too-weak = 至少使用 8 个字符 — 几个不相关的单词效果很好。
vault-mismatch = 两次输入的密码不一致。
vault-wrong-master = 主密码错误。
vault-strength-empty = 至少 8 个字符；几个不相关的单词效果很好。
vault-never-stored = 主密码从不存储。如果忘记，已保存的密码将无法恢复。
vault-unlocking = 正在解锁…
vault-encrypting = 正在加密…
vault-path = 密码库：{ $path }（与 hostwick 命令共用）
vault-title-unlock = 解锁已保存的密码
vault-title-create = 创建密码库
vault-title-change = 更改主密码
vault-not-now = 暂不
vault-unlock = 解锁
vault-create = 创建密码库
vault-change = 更改

## Host editor

form-forward-needs-spec = `{ $token }` 需要端口说明
form-jump-invalid = 跳板机 `{ $host }`：{ $error }
form-port-invalid = { $field } 必须是 1 到 65535 之间的数字
form-baud-invalid = 速率（波特）必须是 9600 或 115200 之类的数字
form-bits-invalid = 数据位必须为 5–8，停止位必须为 1 或 2
host-section-general = 常规
host-section-auth = 身份验证
host-section-connection = 连接
host-section-terminal = 终端
host-section-files = 文件
host-section-advanced = 高级
host-folder-placeholder = prod/web（可选）
host-host-placeholder = 10.0.0.5 或 host.example.com
host-user-placeholder = deploy（留空 = 本地用户名）
host-keepalive-placeholder = 秒，0 = 关闭
host-optional-placeholder = 可选
host-x11-placeholder-windows = 留空 = localhost:0（VcXsrv、Xming）
host-x11-placeholder = 留空 = $DISPLAY
host-script-example = 示例：
host-snippets-empty = 此主机没有代码片段。全局代码片段在“设置 → 代码片段”中。
host-log-placeholder = 留空 = 关闭，例如 &S-&Y&M&D-&T.log
host-sftp-server-placeholder = 留空 = 自动检测（/usr/lib/openssh/sftp-server，…）
host-error-no-device = 请选择串口（设备）
host-error-no-host = 请输入主机名或地址
host-error-no-auth = 请至少选择一种登录方式
host-error-forwards = 端口转发：{ $error }
host-error-keepalive = 保活时间必须以秒为单位（0 到 3600）
host-error-proxy-host = 请输入代理的主机名
host-error-x11 = X11 显示：{ $error }
host-error-script = 登录脚本，{ $error }
host-saved-forget-pending = 保存时将忘记：{ $what }
host-saved-locked = 已保存在密码库中（已锁定）：{ $what }
host-saved = 已保存在密码库中：{ $what }
host-saved-missing = 已不在密码库中，届时会询问：{ $what }
host-saved-keep = 保留
host-saved-forget = 忘记
host-what-password = 密码
host-what-key-passphrase = 密钥密码短语
host-what-proxy-password = 代理密码
host-what-sudo-password = sudo 密码
host-pick-key = 私钥（OpenSSH 或 .ppk）
host-name = 名称
host-folder = 文件夹
host-protocol = 协议
host-host = 主机
host-port = 端口
host-user = 用户
host-notes = 备注
host-telnet-hint = Telnet 以明文发送所有内容；请仅在可信网络中用于网络设备。服务器询问时会发送用户名。
host-ports-looking = 正在查找…
host-ports-none = 未找到端口
host-ports-count = 端口（{ $count }）
host-ports-none-menu = 未找到串口 — 请输入设备名称
host-device = 端口（设备）
host-ports-refresh = 重新查找串口
host-device-hint-windows = COM3、COM4… 选择一个或输入其名称。
host-device-hint = /dev/ttyUSB0、/dev/ttyACM0、/dev/serial/by-id/… 选择一个或输入其路径。
host-baud = 速率（波特）
host-data-bits = 数据位
host-parity = 校验
host-parity-none = 无
host-parity-odd = 奇
host-parity-even = 偶
host-stop-bits = 停止位
host-flow = 流控制
host-flow-none = 无
host-serial-none-linux = 未找到端口。USB 适配器显示为 /dev/ttyUSB*；你可能需要加入 dialout 组。
host-serial-hint = 大多数控制台使用 9600 8N1 或 115200 8N1，无流控制。
host-auth-not-ssh = Telnet 和串口连接在终端内登录。可使用登录脚本（终端）自动完成。
host-key-files = 密钥文件
host-keys-hint = OpenSSH 或 PuTTY .ppk 密钥，以逗号分隔。密钥旁的证书（id_ed25519-cert.pub）会自动使用。安全密钥（sk-…）通过代理程序使用。
host-auth-agent = SSH 代理程序中的密钥（Pageant、ssh-agent、1Password…）
host-auth-password = 密码
host-auth-remember-hint = 询问密码时勾选“记住密码”，下次即可自动登录。
host-auth-kbd = 键盘交互（一次性验证码、MFA）
host-auth-kerberos = Kerberos（单点登录）
host-auth-kerberos-forward = 将我的 Kerberos 票据转发到服务器
host-auth-kerberos-hint-windows = 使用你的 Windows 域登录。
host-auth-kerberos-hint = 使用 kinit 获取的票据（参见 klist）。没有票据时会尝试下一种方式。
host-auth-order = 尝试顺序：Kerberos、代理程序、密钥文件、密码、键盘交互。
host-connection-serial = 串口是本地设备：没有代理、跳板机或转发。
host-proxy = 代理
host-proxy-none = 无
host-proxy-host-port = 代理主机 / 端口
host-proxy-port = 代理端口
host-proxy-user = 代理用户
host-proxy-password-hint = 连接时会询问代理密码（勾选“记住”即可保存）。
host-jump-hosts = 跳板机
host-jump-hint = 以逗号分隔，最外层的在前（同 ssh -J）。
host-forwards = 端口转发
host-forwards-hint = L = 本地 → 远程，R = 远程 → 本地，D = SOCKS 代理。随终端一起启动。
host-keepalive = 保活（秒）
host-agent-forwarding = 转发我的 SSH 代理程序（服务器上的 git 使用我的密钥）
host-agent-forwarding-hint = 仅用于可信服务器：在你连接期间，其管理员可以使用你的密钥。
host-x11 = 转发 X11（在此显示远程图形程序）
host-x11-display = X 显示
host-x11-hint-windows = 需要运行 VcXsrv 或 Xming 等 X 服务器。
host-x11-hint-macos = 需要 XQuartz。
host-x11-hint = 使用桌面的 X 服务器（或 XWayland）。
host-terminal-type = 终端类型
host-login-script = 登录脚本
host-login-script-hint = 每行一条命令，登录后输入。@wait 文本 等待该输出（10 秒），@wait 30000 文本 等待更久，@sleep 500 暂停；# 开头为注释。
host-shell-integration = Shell 集成（bash、zsh）
host-shell-integration-hint = 让文件浏览器跟随终端所在文件夹（右键 → 在此打开文件）。Shell 启动时静默添加提示符钩子；不会在服务器上安装任何东西。
host-snippets-title = 此主机的代码片段
host-session-log = 会话日志
host-session-log-hint = 标记：&S 会话、&H 主机、&P 端口、&U 用户、&Y&M&D 日期、&T 时间。相对路径保存到日志文件夹。
host-raw-log = 记录原始输出（包含转义序列）
host-files-not-ssh = 文件浏览器需要 SSH。
host-file-protocol = 文件协议
host-file-protocol-auto = 自动
host-file-protocol-hint = “自动”使用 SFTP，服务器不支持时（路由器、ESXi、busybox）改用 SCP。
host-sudo = 以 root 身份打开文件浏览器（sudo）
host-sudo-user = 以用户
host-sudo-hint = 支持免密 sudo 或 sudo 密码（询问或已保存）。浏览器中的“以 root 身份重新打开”可随时切换。
host-legacy = 为旧设备允许旧算法（ssh-rsa/SHA-1、旧密钥交换）
host-legacy-hint = 仅用于需要这些算法的旧交换机、路由器和设备。
host-id = 主机 ID：{ $id }
host-title-edit = 编辑主机
host-title-new = 新建主机
loading = 正在加载…

## Settings

settings-title = 设置
settings-section-appearance = 外观
settings-section-terminal = 终端
settings-section-snippets = 代码片段
settings-section-connections = 连接
settings-section-files = 文件
settings-section-logging = 会话日志
settings-section-passwords = 已保存的密码
settings-section-sync = 设置同步
settings-section-diagnostics = 诊断
settings-language = 语言
settings-language-system = 跟随系统（{ $language }）
settings-language-hint = 保存后立即生效。已打开的对话框在再次打开前保持原语言。
settings-theme = 主题
settings-theme-dark = 深色
settings-theme-light = 浅色
settings-terminal-font = 终端字体
settings-font-size = 字号
settings-font-placeholder = 留空 = 系统等宽字体
settings-font-hint = 保存时字体更改也会应用到已打开的终端。
settings-editor-placeholder = 留空 = 系统默认应用（例如 code -w { "{file}" }）
settings-auto-lock-placeholder = 0 = 保持解锁直到退出 Hostwick
settings-sync-folder-placeholder = OneDrive、Dropbox、iCloud Drive 或 git 文件夹
settings-sync-pass-saved = 已保存在密码库中 — 输入以更改
settings-sync-pass-placeholder = 每台电脑使用相同的密码短语
settings-snippets-empty = 还没有代码片段。代码片段以按钮形式显示在每个终端上方，也在“右键 → 代码片段”中。
settings-remove-failed = 无法删除：{ $error }
settings-error-font-size = 终端 → 字号必须是 6 到 48 之间的数字
settings-error-scrollback = 终端 → 回滚行数必须是 100 到 1000000 之间的整数
settings-error-auto-lock = 已保存的密码 → 锁定时间必须以分钟为单位（0 到 1440）
settings-error-rules = 终端 → { $error }
settings-error-sync-folder = 设置同步 → 文件夹 { $folder } 不存在
settings-folder-missing = 文件夹 { $folder } 不存在。
settings-sync-pass-short = 同步密码短语至少需要 8 个字符。
settings-sync-pass-save-failed = 无法保存密码短语：{ $error }
vault-reason-sync-passphrase = 同步密码短语保存在你的密码库中。
settings-save-failed = 无法保存设置：{ $error }
settings-scrollback = 回滚行数
settings-right-click = 右键单击
settings-right-click-menu = 显示菜单
settings-right-click-paste = 粘贴（PuTTY）
settings-copy-on-select = 自动复制所选文本
settings-mouse = 将鼠标点击和滚动发送给程序（htop、mc、vim）
settings-mouse-hint = 程序使用鼠标时，按住 Shift 可选择文本。
settings-button-bar = 在终端上方显示代码片段按钮栏
settings-highlighting = 关键字高亮
settings-highlighting-hint = 为屏幕上匹配的文本着色。规则重叠时，靠前的规则优先。
settings-snippets-hint = 在所有终端中可用。主机可在“编辑主机 → 终端”中添加自己的代码片段。
settings-auto-reconnect = 连接断开时自动重新连接
settings-auto-reconnect-hint = SSH 和 Telnet：在 1、2、5、10 秒后，之后每 30 秒重试一次，保留回滚内容。输入 exit 后或登录、主机密钥失败后不会重连。
settings-restore-tabs = 启动 Hostwick 时重新打开我的标签页
settings-restore-tabs-hint = 包括拆分的窗格及其大小。标签页在你首次切换到它时连接。
settings-editor = 外部编辑器
settings-editor-pick = 编辑器程序
settings-editor-hint = 由“编辑”（F4）使用。{ "{file}" } 会替换为文件路径，否则路径追加在末尾。如编辑器有等待选项（code -w、subl -w），请使用。
settings-log-all = 将每个会话记录到文件
settings-log-folder = 日志文件夹
settings-log-content = 日志内容
settings-log-printable = 可打印文本
settings-log-raw = 原始（包含转义序列）
settings-log-hint = 主机自己的会话日志设置（编辑主机 → 终端）优先。
settings-log-level = 应用日志级别
settings-open-log-folder = 打开日志文件夹
settings-data-folder = 数据文件夹：{ $path }
settings-vault-none = 还没有密码库。在 Hostwick 询问密码时勾选“记住”，或立即创建一个。
settings-vault-locked = 已锁定。解锁后才会使用已保存的密码。
settings-vault-unlocked = 本次会话已解锁。
settings-vault-unlock = 解锁…
settings-vault-create = 创建密码库…
settings-vault-lock = 立即锁定
settings-vault-change = 更改主密码…
settings-vault-empty = 尚未保存任何内容。
settings-vault-remove = 删除
settings-auto-lock = 锁定时间（分钟）
settings-auto-lock-hint = 空闲多少分钟后再次锁定已保存的密码；0 表示保持解锁直到退出 Hostwick。
settings-vault-file = 使用 Argon2id + XChaCha20-Poly1305 加密，位于 { $path } — 与 `hostwick vault` 使用同一个密码库。从不同步。
settings-sync-never = 此电脑从未同步过。
settings-sync-intro = 主机和设置保存在一个加密文件中，放在由云盘或 git 保持同步的文件夹里。已保存的密码绝不会离开此电脑。
settings-sync-folder = 文件夹
settings-sync-folder-pick = 同步文件夹
settings-sync-passphrase = 密码短语
settings-sync-passphrase-hint = 保存在你的密码库中。每台电脑都需要使用同一个。
settings-sync-auto = 启动时、更改后以及每 5 分钟同步一次
settings-sync-now = 立即同步
settings-sync-push = 替换同步文件
settings-sync-push-tooltip = 使同步文件与此电脑一致（同时更改其密码短语）
settings-sync-pull = 替换此电脑的设置
settings-sync-pull-tooltip = 使此电脑的主机和设置与同步文件一致
settings-sync-last = 上次同步

## Snippet and highlight-rule editors

snippet-label-placeholder = 按钮标签
snippet-text-placeholder = 要发送的文本（多行将作为一个整体粘贴）
snippet-press-enter = 按 Enter
snippet-move-up = 上移
snippet-move-down = 下移
editor-remove = 删除
snippet-add = 添加代码片段
rule-error-empty = 模式为空
rule-error-color = 颜色格式应类似 #ff8800，而不是 `{ $color }`
rule-error-regex = 正则表达式无效
rule-pattern-placeholder = 单词或正则表达式
rule-error = 高亮 `{ $pattern }`：{ $error }
rule-enabled = 已启用
rule-regex-on = 正则表达式（点击改为普通单词）
rule-regex-off = 普通单词，不区分大小写（点击改为正则表达式）
rule-add = 添加规则
rule-defaults = 恢复默认

## Terminal

script-timeout = 等待“{ $text }”{ $after }，但它始终没有出现
script-session-ended = 会话先结束了
search-too-complex = 模式过于复杂
term-exit-status = 已退出，状态 { $code }
term-exit-signal = 被信号 { $signal } 终止
term-connection-lost = 连接已断开
term-connection-closed = 连接已关闭
term-reconnected-marker = 已于 { $time } 重新连接
term-reconnect-failed = 重新连接失败：{ $error }
term-break-failed = 无法发送 Break
term-no-break = { $protocol } 会话没有 Break 信号
term-script-not-run = 未运行登录脚本：{ $error }
term-script-stopped = 脚本已停止：{ $error }
search-placeholder = 在回滚内容中查找
search-no-results = 无结果
search-position = 第 { $index } 个，共 { $total } 个
search-matches = { $count } 个匹配
search-match-case = 区分大小写
search-regex = 正则表达式
search-older = 上一个（较早）匹配 — Enter、F3
search-newer = 下一个（较新）匹配 — Shift+Enter、Shift+F3
search-close = 关闭（Esc）
term-reconnect-in = 连接已断开{ $reason }。{ $seconds } 秒后重新连接…
term-reconnect-attempt = 正在重新连接（第 { $attempt } 次尝试）…
term-log-badge = 日志
term-reconnect-now = 立即
term-session-ended = 会话已结束 — { $reason }。按 Enter 重新连接。
term-menu-copy = 复制
term-menu-paste = 粘贴
term-menu-select-all = 全选
term-menu-find = 查找…
term-menu-snippets = 代码片段
term-menu-copy-folder = 复制当前文件夹
term-menu-stop-script = 停止脚本
term-menu-clear = 清除回滚内容
term-menu-stop-log = 停止会话日志
term-menu-start-log = 将会话记录到文件…
term-menu-files-here = 在此打开文件
term-menu-duplicate = 复制会话
term-menu-reconnect-ended = 重新连接（会话已结束）

## File browser

files-move = 移动
files-upload = 上传
files-download = 下载
files-move-confirm = { $count ->
    [1] 要将“{ $name }”移动到 { $dir } 吗？
   *[other] 要将 { $count } 个项目移动到 { $dir } 吗？
}
files-upload-confirm = { $count ->
    [1] 要将“{ $name }”上传到 { $dir } 吗？
   *[other] 要将 { $count } 个项目上传到 { $dir } 吗？
}
files-download-confirm = { $count ->
    [1] 要将“{ $name }”下载到 { $dir } 吗？
   *[other] 要将 { $count } 个项目下载到 { $dir } 吗？
}
files-same-place = { $name }：源和目标相同
files-skipped-unsafe = 未显示，因为它们的名称在此计算机上不安全：{ $names }
files-not-removed = { $name }：已复制，但未删除：{ $error }
files-pick-download = 下载到此处
files-exists = “{ $name }”已存在
files-delete-title = 删除
files-delete-folder-confirm = 要永久删除文件夹“{ $name }”及其中的所有内容吗？
files-delete-file-confirm = 要永久删除文件“{ $name }”吗？
files-delete-many-confirm = 要永久删除 { $count } 个项目（文件夹连同其中所有内容）吗？
files-name-invalid = 请输入不含“/”的名称
files-new-folder = 新建文件夹
files-create = 创建
files-folder-name = 文件夹名称
files-rename = 重命名
files-new-name = 新名称
files-new-symlink = 新建符号链接
files-symlink-placeholder = 目标 -> 链接名称
files-symlink-format = 请写成“目标 -> 链接名称”
files-symlink-empty = 目标为空
edit-conflict-title = 服务器上已更改
edit-conflict-body = 自你打开“{ $name }”以来，它已在服务器上被更改。你保存的版本应如何处理？
edit-conflict-later = 稍后决定
edit-conflict-theirs = 下载服务器版本
edit-conflict-copy = 另存为副本
edit-conflict-overwrite = 覆盖
edit-editor-failed = 无法启动编辑器“{ $program }”：{ $error }
keep-up-to-date = 已是最新
keep-synced = 已同步 { $count } 个文件，{ $size }
keep-failed = { $count } 个失败
keep-error = 错误（正在重试）：{ $error }
keep-starting = 正在启动…
keep-status = 正在保持 { $local } → { $remote } 最新：{ $status }
edits = 编辑
edits-busy = 编辑（{ $count }）：{ $status }
edits-tooltip = 在编辑器中打开的文件；保存后自动上传
edits-stop = 停止监视
edits-stop-all = 全部停止监视
files-hide-local = 隐藏本地
files-show-local = 显示本地
files-local-tooltip = 显示或隐藏本地电脑的文件
files-upload-pick = 上传…
files-synchronize = 同步…
files-synchronize-tooltip = 比较文件夹，查看更改，然后执行
files-keep-stop = 停止保持最新
files-keep = 保持最新…
files-keep-tooltip = 自动将本地文件夹中的更改上传到远程文件夹
files-following = 正在跟随终端
files-follow = 跟随终端
files-follow-tooltip = 显示终端“{ $terminal }”当前所在的文件夹，并随其变化
files-root-switching = 正在切换…
files-root-on = Root：开
files-root-reopen = 以 root 身份重新打开
files-root-tooltip = 通过 sudo 以 root（或主机的 sudo 用户）身份浏览服务器
files-path-placeholder = /路径
files-this-computer = 此电脑
files-selected-of = 已选择 { $selected } / { $count } 个（{ $size }）
files-item-count = { $count } 个项目
files-selected = { $count } 个项目
files-root-badge = ROOT
files-up-tooltip = 向上（Backspace）
files-refresh-tooltip = 刷新（Ctrl+R）
files-mkdir-tooltip = 新建文件夹（F7）
files-col-name = 名称
files-col-size = 大小
files-col-modified = 修改时间
files-col-mode = 权限
files-download-pick = 下载…
files-menu-open = 打开
files-menu-edit = 编辑
files-menu-move = 移动到另一侧
files-menu-rename = 重命名…
files-menu-new-folder = 新建文件夹…
files-menu-new-symlink = 新建符号链接…
files-menu-copy-path = 复制路径
files-menu-refresh = 刷新
files-menu-properties = 属性
edit-status-downloading = 正在下载…
edit-status-watching = 正在监视
edit-status-uploading = 正在上传…
edit-status-uploaded = 已于 { $time } 上传
edit-status-uploaded-in-place = 已于 { $time } 上传（原地重写）
edit-status-conflict = 服务器上已更改 — 等待你处理
edit-status-error = 错误：{ $error }
edit-gone = { $path }：已不存在
edit-kept-folder = 未上传的已编辑文件保存在 { $path }
edit-is-folder = { $path } 是文件夹
edit-missing-after-upload = { $path }：上传后不存在
edit-size-mismatch = { $path }：上传 { $uploaded } 字节后，服务器上为 { $server } 字节
edit-no-copy-name = 没有可用于副本的名称

## File properties

props-title = 属性
props-applying = 正在应用…
props-apply = 应用
props-calculating = 正在计算…
props-error = 错误：{ $error }
props-usage = { $size }（{ $bytes } 字节），{ $files } 个文件，{ $folders } 个文件夹
props-usage-partial = （提前停止：条目过多）
props-bad-id = “{ $id }”不是数字 ID
props-bad-octal = 权限必须为八进制，例如 0644
props-kind-folder = 文件夹
props-kind-file = 文件
props-kind-symlink = 符号链接 → { $target }
props-kind-special = 特殊文件
props-unknown-name = （未知名称）
props-not-reported = 服务器未提供
props-read = 读取
props-write = 写入
props-execute = 执行
props-owner = 所有者
props-group = 组
props-others = 其他人
props-special = 特殊
props-octal = 八进制
props-calculate = 计算
props-size = { $size }（{ $bytes } 字节）
props-location = 位置
props-type = 类型
props-checksum = 校验和
props-unknown = 未知
props-permissions = 权限
props-recursive = 应用到其中的所有内容（文件保留其执行位）

## Synchronize dialog

files-synchronize-title = 同步
sync-keep = 保持远程最新
sync-keep-tooltip = 监视本地文件夹并上传每一处更改（仅本地 → 远程）
sync-comparing = 正在比较…
sync-preview = 预览
sync-run-checked = 同步所选项
sync-exclude-placeholder = 例如 .git、*.tmp、node_modules
sync-choose-folders = 请选择两个文件夹
sync-use-local = 使用本地
sync-use-remote = 使用远程
sync-total-uploads = 上传 { $count } 个，{ $size }
sync-total-downloads = 下载 { $count } 个，{ $size }
sync-total-deletions = 删除 { $count } 个
sync-total-conflicts = { $count } 个冲突 — 选择一侧或保持不变
sync-in-sync = 两个文件夹已经同步。
sync-checked = 已选：{ $items }
sync-local-folder = 本地文件夹
sync-remote-folder = 远程文件夹
sync-direction = 方向
sync-dir-up = 本地 → 远程
sync-dir-down = 远程 → 本地
sync-dir-both = 双向（较新者优先）
sync-mirror = 删除源中不存在的文件（镜像）
sync-exclude = 排除
sync-comparing-folders = 正在比较文件夹…
sync-result = 同步：{ $summary }
sync-running = 正在同步…
sync-run-failed = 同步失败：{ $error }
sync-kind-upload = 上传
sync-kind-download = 下载
sync-kind-delete-local = 删除本地
sync-kind-delete-remote = 删除远程
sync-kind-conflict = 冲突
sync-kind-skip = 跳过
sync-reason-new = 新的
sync-reason-newer = 较新
sync-reason-size = 大小不同
sync-reason-differs = 不同
sync-reason-not-in-source = 源中不存在
sync-reason-both-changed = 两侧都已更改（时间相同，大小不同）
sync-reason-unknown = 无法比较（没有时间戳）
sync-reason-destination-newer = 目标较新
sync-reason-new-folder = 新文件夹
sync-reason-case-conflict = 另一个文件仅大小写不同
sync-review-deletions = 此次同步会删除文件。请检查下面的列表，然后再次点击“同步”。
sync-report-uploaded = 已上传 { $count } 个
sync-report-downloaded = 已下载 { $count } 个
sync-report-deleted = 已删除 { $count } 个
sync-report-folders = 已创建 { $count } 个文件夹
sync-report-failed = { $count } 个失败
sync-report-nothing = 无需操作

## Key tools

keyformat-openssh-hint = 适用于 OpenSSH、Hostwick、Git 和大多数工具（同时写入 .pub 文件）。
keyformat-ppk3-hint = 适用于 PuTTY 0.75+ 和 WinSCP（Argon2 保护）。
keyformat-ppk2-hint = 适用于旧版 PuTTY、WinSCP 和 FileZilla。
key-fingerprint = 指纹：{ $fingerprint }
keygen-title = 生成密钥
keygen-generate = 生成
keygen-passphrase-placeholder = 密码短语（推荐；留空 = 无）
keygen-repeat = 再次输入
keygen-mismatch = 两次输入的密码短语不一致。
keygen-no-path = 请选择密钥的保存位置。
keygen-exists = { $path } 已存在。请换一个名称，或勾选“覆盖”。
keygen-used-in-host = { $name } 现在使用 { $path } 登录。请将公钥添加到服务器的 ~/.ssh/authorized_keys。
keygen-ed25519-recommended = Ed25519（推荐）
keygen-saved = 已将 { $file }（私钥）和 { $file }.pub 保存到 { $dir }
keygen-public-key = 公钥（用于 ~/.ssh/authorized_keys）：
keygen-copy-public = 复制公钥
keygen-use-in-host = 用于主机…
keygen-type = 类型
keygen-comment = 注释
keygen-format = 格式
keygen-save-as = 另存为
keygen-overwrite = 覆盖现有文件
keygen-generating-rsa = 正在生成… RSA 4096 可能需要几秒钟。
keygen-generating = 正在生成…
keyconv-title = 查看或转换密钥
keyconv-path-placeholder = ~/.ssh/id_ed25519 或 C:\keys\work.ppk
keyconv-current-placeholder = 当前密码短语（如有）
keyconv-new-placeholder = 新密码短语（留空 = 无）
keyconv-repeat-placeholder = 再次输入新密码短语
keyconv-output = 输出文件
keyconv-mismatch = 两次输入的新密码短语不一致。
keyconv-pass-changed = 密码短语已更改（{ $format }）。
keyconv-pass-removed = 密码短语已删除（{ $format }）。
keyconv-wrote = 已写入 { $path }（{ $format }）。
keyconv-needs-passphrase = 此密钥有密码短语：请在“当前密码短语”中输入。
keyconv-wrong-passphrase = 密码短语错误。
keyconv-exists = { $path } 已存在；请选择其他输出文件。
keyconv-key-file = 密钥文件
keyconv-type-bits = { $kind } { $bits } 位（{ $algorithm }）
keyconv-protected = 受保护
keyconv-public-only = 仅公钥
keyconv-encrypted = 是，有密码短语
keyconv-no-passphrase = 无密码短语
keyconv-fingerprint = 指纹
keyconv-inspect = 查看
keyconv-current = 当前
keyconv-new = 新密码短语
keyconv-change = 更改密码短语
keyconv-convert-to = 转换为
keyconv-convert = 转换
keyconv-keeps-passphrase = 除非输入新的密码短语，否则保留当前密码短语。
keyconv-working = 正在处理…
kh-title = 已知主机
kh-search-placeholder = 搜索主机、密钥类型或指纹
kh-forget-placeholder = 主机 或 主机:端口
kh-nothing-matched = 没有匹配项；文件未更改。
kh-removed = 已删除 { $count } 个密钥。
kh-type-host = 请输入主机名或 主机:端口。
kh-col-line = 行
kh-col-hosts = 主机
kh-empty = 还没有已知主机。信任服务器时会添加密钥。
kh-no-match = 没有与搜索匹配的内容。
kh-reload = 重新加载
kh-remove-selected = 删除所选项（{ $count }）
kh-forget-host = 忘记主机
kh-forget-button = 删除其所有密钥
kh-footer = { $path } — 显示 { $shown } / { $total } 条。按名称忘记主机时，也会匹配经过哈希的名称。
files-opening-root = 以 root 身份打开文件
files-opening = 打开文件浏览器

## Keyboard and screen readers

sidebar-vault-lock = 锁定已保存的密码
sidebar-quick-connect = 快速连接（user@host:port，Enter）
sidebar-hosts-label = 已保存的主机
sidebar-hosts-keys = 方向键选择主机。Enter 打开终端，Shift+Enter 打开文件，F2 编辑，Delete 删除，Shift+F10 打开菜单。
tabs-label = 已打开的会话
tabs-keys = 左右方向键切换标签页，Enter 进入标签页，Delete 关闭标签页，Shift+F10 打开其菜单。Ctrl+F6 在主机列表、标签页和会话之间切换。
tab-state-not-connected = 尚未连接
tab-state-connected = 已连接
tab-state-disconnected = 已断开
tab-spoken = { $title }，{ $state }，第 { $index } 个标签页，共 { $count } 个
tab-close = 关闭 { $title }
pane-divider = 窗格分隔条

## Proxy test

vault-reason-proxy-test = 请解锁已保存的密码以使用已保存的代理密码。
proxy-test-closed = 代理已连接，但 { $target } 立即关闭了连接。
proxy-test-ok-ssh = 已通过代理连接到 { $target }。服务器返回：{ $server }
proxy-test-ok = 已通过代理连接到 { $target }（该服务器不是 SSH 服务器）。
proxy-test-ok-quiet = 已通过代理连接到 { $target }（服务器尚未发送任何内容）。
proxy-test-read-failed = 已通过代理连接到 { $target }，随后：{ $error }
host-proxy-test = 测试连接
host-proxy-testing = 正在测试…
host-proxy-test-tooltip = 通过代理连接到此主机的端口（如有跳板机，则为第一个跳板机）并报告结果
host-baud-common = 常用速率
dismiss = 关闭
term-no-output = 还没有输出。
term-text-view-title = 终端输出：{ $title }
term-text-view-hint = 共 { $lines } 行。用方向键阅读；按 Esc 返回终端。
term-a11y-label = 终端：{ $title }
term-a11y-keys = 新输出稳定后会被朗读。{ $read } 重新朗读最新输出，{ $view } 以文本显示全部输出。Ctrl+F6 离开终端。
term-menu-read-output = 朗读最新输出
term-menu-text-view = 以文本显示输出…
files-path-label = 文件夹：{ $side }
files-list-label = { $side }：{ $dir }
files-list-keys = 方向键移动，Shift+方向键扩展选择，空格键添加或移除一行，Enter 打开，Backspace 返回上级，F5 复制到另一侧，F6 移动，F2 重命名，Delete 删除，Alt+Enter 显示属性，Tab 切换左右，Shift+F10 打开菜单。
files-kind-link = 链接
a11y-sections = 分区
a11y-section-problem = { $section }（需要注意）
kh-row-label = 第 { $line } 行：{ $hosts }
rule-color = 颜色（#rrggbb）
toast-info = 信息
toast-success = 完成
toast-warning = 警告
toast-error = 错误
toast-close = 关闭通知
menu-close-pane = 关闭窗格
pane-files-title = 文件：{ $host }
