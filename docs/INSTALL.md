# 安装

包名：`rsztui`  
安装后的命令：`rsztui`

## AUR

AUR 发布后：

```bash
paru -S rsztui
```

AUR 账号/SSH key/首次发布步骤见 [AUR.md](AUR.md)。AUR 新账号注册偶尔会临时关闭，期间请用下面的本地方式。

## 本地 PKGBUILD

```bash
cd RszTui/packaging/aur
paru -Bi
```

等价于 `makepkg -si`。只构建不安装：

```bash
makepkg -f
```

## 生成 pacman 包

```bash
cd RszTui
make pkg
```

产物：

```
packaging/aur/rsztui-<版本>-1-x86_64.pkg.tar.zst
```

安装：

```bash
paru -U packaging/aur/rsztui-0.5.0-1-x86_64.pkg.tar.zst
# 或 sudo pacman -U ...
```

## GitHub Release

从 [Releases](https://github.com/Ricarsz/RszTui/releases) 下载 `.pkg.tar.zst` 后：

```bash
paru -U rsztui-<版本>-1-x86_64.pkg.tar.zst
```

推送 `v*` tag 时，GitHub Actions 会自动在 Arch 容器里构建并附加包到 Release。

## 手动构建

```bash
cd RszTui
make                 # cargo build --release
sudo make install    # 安装到 /usr/local/bin/rsztui
```

卸载：

```bash
sudo make uninstall
# AUR / pacman 安装则：
sudo pacman -R rsztui
```

## 依赖

运行检查：

```bash
rsztui doctor
```

- `asusctl` / `asusd`：ASUS 硬件控制
- `NetworkManager` / `nmcli`：网络和热点
- 系统服务：`systemd`

缺项时 `doctor` 会按发行版给出安装/启动命令。详见 [配置与依赖](CONFIG.md)。
