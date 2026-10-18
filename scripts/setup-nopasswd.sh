#!/bin/sh
# rsztui 可选的免密启动设置。
# 只会给当前用户添加一条 NOPASSWD 规则，并安装 /usr/local/bin/rsztui 包装脚本。
# 不修改系统默认 sudo 策略，不会自动运行。
set -eu

MARKER="# managed by rsztui"
BIN=/usr/bin/rsztui
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
WRAPPER=/usr/local/bin/rsztui
SUDOERS=/etc/sudoers.d/rsztui

if [ "$(id -u)" -ne 0 ]; then
	exec sudo "$0" "$@"
fi

USER_NAME=${SUDO_USER:-}
if [ -z "$USER_NAME" ] || [ "$USER_NAME" = "root" ]; then
	echo "请从目标用户通过 sudo 运行，例如：sudo $0" >&2
	exit 1
fi

if [ ! -x "$BIN" ]; then
	echo "找不到 $BIN，请先安装 rsztui" >&2
	exit 1
fi

tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

cat >"$tmpdir/sudoers" <<EOF
$USER_NAME ALL=(root) NOPASSWD: $BIN
EOF
chmod 644 "$tmpdir/sudoers"

if ! visudo -cf "$tmpdir/sudoers" >/dev/null; then
	echo "sudoers 校验失败，未做任何修改" >&2
	exit 1
fi

cat >"$tmpdir/wrapper" <<EOF
#!/bin/sh
$MARKER
BIN=$BIN

if [ "\$(id -u)" -eq 0 ]; then
    exec "\$BIN" "\$@"
fi

case "\${1:-}" in
    doctor|--help|-h|--version|-V|help)
        exec "\$BIN" "\$@"
        ;;
esac

for arg in "\$@"; do
    if [ "\$arg" = "doctor" ]; then
        exec "\$BIN" "\$@"
    fi
done

exec sudo -n "\$BIN" "\$@"
EOF
chmod 755 "$tmpdir/wrapper"

if [ -e "$WRAPPER" ] && ! grep -q "$MARKER" "$WRAPPER"; then
	echo "$WRAPPER 已存在且不是 rsztui 管理的文件，拒绝覆盖" >&2
	exit 1
fi

install -Dm0440 -o root -g root "$tmpdir/sudoers" "$SUDOERS"
install -Dm755 -o root -g root "$tmpdir/wrapper" "$WRAPPER"

echo "已安装："
echo "  $SUDOERS"
echo "  $WRAPPER"
echo
echo "现在直接运行 rsztui 即可，不需要 sudo/密码。"
echo "回滚：sudo $SCRIPT_DIR/remove-nopasswd.sh"
echo
echo "注意：这等于给 $USER_NAME 一个受限 root 控制台（不能直接开 shell，但可管理网络、服务和功耗）。"
