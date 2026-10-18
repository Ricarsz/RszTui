# RszTui

ASUS TUF 的终端控制台。Rust + [ratatui](https://ratatui.rs/)，底层调用 `asusctl` 和 NetworkManager 的 `nmcli`。

目标：替代 `rog-control-center` 的日常功能——性能模式、电池、功耗、风扇曲线、灯光、网络和系统服务。

## 特性

- **性能/功耗**：三种性能模式、AC/电池默认模式、PL1/PL2/PL3、dGPU TGP、Dynamic Boost、温度墙
- **电池**：20–100% 充电上限、单次充满、健康度/循环/功率
- **GPU**：集显 / 混合 / 独显直连（`asusctl armoury`，需重启）
- **风扇**：图形化曲线、逐点编辑、原始 PWM 保存、预设
- **灯光**：当前灯效/亮度/颜色、Aura 模式切换、静态色
- **网络**：Wi-Fi 扫描连接、DHCP/静态 IP、网关、DNS/DoT、VPN、热点、连接管理、Ping/DNS 查询、DNS/邻居缓存
- **服务**：asusd、asus-shutdown、NetworkManager、systemd-resolved 的启停/重启/自启
- **外观**：5 套配色主题，网络速率单位可选二进制 / 十进制 / 比特
- **诊断**：发行版、ASUS 笔记本、CPU/GPU 型号、asusctl/asusd/nmcli/NetworkManager 检测
- **CLI**：与 TUI 同能力，支持 `--json`

## 快速开始

```bash
git clone https://github.com/Ricarsz/RszTui.git
cd RszTui
cd packaging/aur && paru -Bi
sudo rsztui
```

## 文档

| 文档 | 内容 |
|---|---|
| [安装](docs/INSTALL.md) | 本地 PKGBUILD、GitHub Release、手动构建、更新/卸载 |
| [命令与快捷键](docs/COMMANDS.md) | 所有 CLI 子命令、TUI 快捷键、鼠标操作、开发命令 |
| [配置与依赖](docs/CONFIG.md) | 配置文件、持久化、依赖检测、安全说明、已知限制 |

## 当前版本

`0.12.1` · Arch / CachyOS · 需要 `asusctl`、`NetworkManager`，必须 `sudo` 运行（`doctor` 除外）。

## 许可

MIT
