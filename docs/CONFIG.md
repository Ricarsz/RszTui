# 配置与依赖

## 配置文件

首次运行生成：

```
~/.config/rsztui/config.toml
```

内容：

- 三套功耗预设（静音/均衡/性能）
- 保存的风扇预设（原始 PWM）
- 界面偏好（上次页面）

即使以 sudo 运行，也会通过 `SUDO_USER` 写入真实用户家目录，而不是 `/root`。

## 持久化

- 性能模式、电池上限、PPT/TGP 写入 `/etc/asusd/asusd.ron`，由 **asusd** 在开机和 AC/电池切换时自动恢复。
- 功耗值写入当前供电模式（AC 或电池）下、当前性能模式的 tuning。想改电池那一套，拔电源再调。
- 风扇曲线由 asusd 持久化在 `/etc/asusd/fan_curves.ron`。
- 网络配置由 NetworkManager 持久化在 `/etc/NetworkManager/system-connections/`。

**本工具不自动备份**。需要时手动：

```bash
sudo cp -a /etc/asusd /etc/asusd.bak
```

## 依赖与硬件检测

```bash
rsztui doctor
rsztui --json doctor
```

检测：

- 发行版（`/etc/os-release`）
- 主机厂商、型号、机箱类型（DMI）
- CPU 型号/核心数/线程数
- GPU 型号（lspci，NVIDIA 回退 nvidia-smi）
- 内存、内核、BIOS
- 是否 ASUS 笔记本
- `asusctl`、`asusd`
- `nmcli`、`NetworkManager`

缺项会按发行版给出安装/启动命令：

| 发行版 | asusctl | NetworkManager |
|---|---|---|
| Arch/CachyOS | `paru -S asusctl` | `sudo pacman -S networkmanager` |
| Fedora | `sudo dnf install asusctl`（Terra 仓库） | `sudo dnf install NetworkManager` |
| openSUSE | `sudo zypper install asusctl` | `sudo zypper install NetworkManager` |
| Debian/Ubuntu | 需从源码构建 | `sudo apt install networkmanager` |

服务：

```bash
sudo systemctl enable --now asusd
sudo systemctl enable --now NetworkManager
```

非 ASUS 机器会标记主机检查，但网络功能仍可使用；asusctl/asusd 在该情况下不算必需依赖。

TUI 启动时会弹出同样的检查结果；设置页常驻显示，`r` 重新检测。

## DNS / 邻居缓存

Network 页按 `c` 打开缓存面板，或使用 CLI：

```bash
sudo rsztui net cache status
sudo rsztui net cache flush
sudo rsztui net cache flush-neighbors
sudo rsztui net cache size 4096
```

- `flush` 清空 systemd-resolved 的 DNS 缓存
- `flush-neighbors` 清空内核 ARP/邻居缓存
- `size` 写入 `/etc/systemd/resolved.conf.d/rsztui-cache.conf` 的 `CacheSize=`，并重启 `systemd-resolved`（会短暂中断 DNS）；范围 0–65536

## 安全说明

- TUI 以 root 运行，只在本机终端使用。
- Wi-Fi/热点密码不放进命令行参数，写入 `/etc/NetworkManager/system-connections/`（0600 root），再 `nmcli connection reload`。
- 网络修改前二次确认；IP/网关/DNS 做基本格式校验。
- 服务停止/重启有影响提示：NetworkManager 会断网，asusd 会失去 ASUS 控制。

## 已知限制

- 已有同名 Wi-Fi 配置直接复用，暂不支持在工具内改密码；需先删除该连接再重连。
- 网络只处理 IPv4，IPv6 由 NetworkManager 默认策略管理。
- 热点固定连接 id 为 `rsztui-hotspot`，TUI 内 SSID 固定；CLI 可指定 SSID。
- DoT 依赖 NetworkManager + systemd-resolved；DoH 不原生支持。
- GPU 模式修改必须重启。
- 风扇曲线要求温度严格递增；PWM 0–255。
- 温度列表依赖 `hwmon` 标签，不同机型差异较大。
- 不自动备份，写坏了需要手动恢复。
