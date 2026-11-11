# 安装

包名：`rogtui`  
安装后的命令：`rogtui`

## 本地 PKGBUILD

```bash
git clone https://github.com/Ricarsz/rogtui.git
cd rogtui/packaging/aur
paru -Bi
```

等价于 `makepkg -si`。只构建不安装：

```bash
makepkg -f
```

## 生成 pacman 包

```bash
cd rogtui
make pkg
```

产物：

```
packaging/aur/rogtui-<版本>-1-x86_64.pkg.tar.zst
```

安装：

```bash
paru -U packaging/aur/rogtui-<版本>-1-x86_64.pkg.tar.zst
# 或 sudo pacman -U ...
```

## GitHub Release

从 [Releases](https://github.com/Ricarsz/rogtui/releases) 下载 `.pkg.tar.zst` 后：

```bash
paru -U rogtui-<版本>-1-x86_64.pkg.tar.zst
```

推送 `v*` tag 时，GitHub Actions 会自动在 Arch 容器里构建并附加包到 Release。

## 手动构建

```bash
cd rogtui
make                 # cargo build --release
sudo make install    # 安装到 /usr/local/bin/rogtui
```

卸载：

```bash
sudo make uninstall
# pacman 安装则：
sudo pacman -R rogtui
```

## 可选：免密启动

默认需要 `sudo rogtui`。如果不想每次输入密码，可以显式运行：

源码目录：

```bash
cd rogtui
./scripts/setup-nopasswd.sh
# 或 make setup-nopasswd
```

pacman 安装后：

```bash
sudo /usr/share/rogtui/setup-nopasswd.sh
```

它会：

- 写入 `/etc/sudoers.d/rogtui`，只允许当前用户无密码运行 `/usr/bin/rogtui`
- 安装 `/usr/local/bin/rogtui` 包装脚本，之后直接输入 `rogtui`
- `doctor`、`--help`、`--version` 仍然以普通用户运行

**安全代价**：这相当于给当前账号一个受限 root 控制台（不能直接开 shell，但可以管理网络、DNS 缓存、系统服务和功耗）。单人笔记本可接受，共享机器不建议。

卸载：

```bash
sudo ./scripts/remove-nopasswd.sh
# pacman 安装：
sudo /usr/share/rogtui/remove-nopasswd.sh
```

包管理器不会自动安装或修改 sudoers，必须手动执行上述脚本。

## 依赖

运行检查：

```bash
rogtui doctor
```

- `asusctl` / `asusd`：ASUS 硬件控制
- `NetworkManager` / `nmcli`：网络和热点
- 系统服务：`systemd`

缺项时 `doctor` 会按发行版给出安装/启动命令。详见 [配置与依赖](CONFIG.md)。
