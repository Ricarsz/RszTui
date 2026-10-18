#!/bin/sh
# 移除 rsztui 的免密启动设置。
set -eu

MARKER="# managed by rsztui"
WRAPPER=/usr/local/bin/rsztui
SUDOERS=/etc/sudoers.d/rsztui

if [ "$(id -u)" -ne 0 ]; then
	exec sudo "$0" "$@"
fi

if [ -e "$WRAPPER" ]; then
	if grep -q "$MARKER" "$WRAPPER"; then
		rm -f "$WRAPPER"
		echo "已删除 $WRAPPER"
	else
		echo "$WRAPPER 不是 rsztui 管理的文件，跳过"
	fi
fi

if [ -e "$SUDOERS" ]; then
	rm -f "$SUDOERS"
	echo "已删除 $SUDOERS"
fi

echo "免密启动已移除，之后需要 sudo rsztui。"
