# RszTui

ASUS TUF 的终端控制台。Rust + [ratatui](https://ratatui.rs/)，底层调用 `asusctl` 和 NetworkManager 的 `nmcli`。

目标：替代 `rog-control-center` 的日常功能——切性能模式、限电池、调功耗、看风扇/温度、管网络。

## 已实现

- **仪表盘**：CPU/内存/网络实时曲线、温度列表、电池状态、ASUS 状态汇总
- **性能**：Quiet/Balanced/Performance 切换、AC/电池默认模式
- **功耗**：PL1/PL2/PL3、dGPU TGP、Dynamic Boost、温度墙；预设 + 手动微调
- **电池**：充电上限 20–100%、单次充满、容量/健康度/循环/功率
- **GPU**：集显/混合/独显直连（`asusctl armoury` 的 `dgpu_disable` + `gpu_mux_mode`，需重启）
- **风扇**：查看/启用/停用曲线、恢复默认、保存/套用/删除预设（原始 PWM，无舍入误差）
- **灯光**：键盘亮度、Aura 下一个/上一个灯效、静态颜色
- **网络**：连接列表、Wi-Fi 扫描/连接/断开、DHCP/静态 IP、网关、DNS、DoT、VPN 启停、热点、无线开关、连接删除
- **CLI**：与 TUI 同能力，支持 `--json`

## 依赖

- `asusctl` / `asusd`（Arch：`asusctl` 包），并已启用 `asusd.service`
- NetworkManager（`nmcli`）
- 本工具**必须以 root 运行**：`sudo rsztui`

启动时如果不是 root 会直接提示退出。

依赖与硬件检查：

```bash
rsztui doctor          # 普通用户即可运行
rsztui --json doctor
```

会检测发行版、是否 ASUS 笔记本（DMI 厂商/机箱类型）、`asusctl`、`asusd`、`nmcli`、`NetworkManager`。缺项会给出对应发行版的安装/启动命令；TUI 启动时也会弹出同样的提示，设置页可随时查看。

## 安装

### AUR（推荐）

已发布后：

```bash
paru -S rsztui
```

AUR 包名是 `rsztui`，安装后的命令也是 `rsztui`。

### 从本地 PKGBUILD 安装

还没发布到 AUR 时，可以直接用 paru 构建当前目录：

```bash
cd RszTui/packaging/aur
paru -Bi
```

或者：

```bash
cd RszTui/packaging/aur
makepkg -si
```

### 手动构建

```bash
cd RszTui
make                 # cargo build --release
sudo make install    # 安装到 /usr/local/bin/rsztui
```

卸载：`sudo make uninstall`（AUR 安装则 `sudo pacman -R rsztui`）。

AUR 发布/更新步骤见 [`packaging/aur/README.md`](packaging/aur/README.md)。

## 使用

```bash
sudo rsztui                 # 启动 TUI
sudo rsztui status          # 状态汇总
sudo rsztui --json status   # 给脚本/waybar 用
```

### 性能与功耗

```bash
sudo rsztui profile get
sudo rsztui profile set performance
sudo rsztui profile default ac performance
sudo rsztui profile default battery quiet

sudo rsztui power preset performance
sudo rsztui power apply --pl1 45 --pl2 55 --pl3 65
sudo rsztui power apply --gpu-tgp 70 --gpu-temp 80
```

### 电池

```bash
sudo rsztui battery info
sudo rsztui battery limit 80
sudo rsztui battery oneshot
```

### GPU

```bash
sudo rsztui gpu get
sudo rsztui gpu set hybrid --yes    # 需要重启
```

### 网络

```bash
sudo rsztui net status
sudo rsztui net wifi list
sudo rsztui net wifi connect "MyWiFi" --password 'secret'
sudo rsztui net wifi connect "MyWiFi" --password 'secret' --no-up  # 只建配置
sudo rsztui net radio off
sudo rsztui net ip show mt3600
sudo rsztui net ip set mt3600 --method manual \
  --address 192.168.31.50/24 --gateway 192.168.31.1 --dns 1.1.1.1,8.8.8.8
sudo rsztui net ip set mt3600 --method auto --dns 1.1.1.1

sudo rsztui net vpn list
sudo rsztui net vpn up myvpn
sudo rsztui net conn up mt3600
sudo rsztui net conn delete old-connection
sudo rsztui net hotspot on --password 'hotspot123' --ssid rsztui-hotspot
sudo rsztui net hotspot off
```

### 风扇与灯光

```bash
sudo rsztui fan list
sudo rsztui fan save 我的配置
sudo rsztui fan apply 我的配置
sudo rsztui fan delete 我的配置
sudo rsztui fan default
sudo rsztui fan enable
sudo rsztui fan disable

sudo rsztui aura next
sudo rsztui aura prev
sudo rsztui aura static ff8800
```

## 快捷键

全局：

| 键 | 作用 |
|---|---|
| `Tab` / `Shift+Tab` | 切换页面 |
| `1`–`7` | 直接跳页 |
| `r` | 刷新 |
| `?` | 帮助 |
| `q` | 退出（确认） |
| `Ctrl+C` | 立即退出 |

性能页：

| 键 | 作用 |
|---|---|
| `f` | 在“性能模式”和“功耗”之间切换焦点 |
| `j` / `k` | 选择 |
| `Enter` | 应用性能模式 / 功耗 |
| `a` / `b` | 设为 AC / 电池默认模式 |
| `h` / `l` | 功耗微调（`H` / `L` 每次 5） |
| `[` / `]` | 选择功耗预设 |
| `p` | 套用选中的功耗预设 |
| `g` / `G` | 选择 / 应用 GPU 模式（需重启） |

电池页：`h` / `l` 调上限，`Enter` 应用，`o` 单次充满。

风扇页：

- 上方是曲线图（横轴温度、纵轴 PWM），CPU 青色、GPU 紫色
- `i` / `I` 进入 CPU / GPU 曲线编辑
- 编辑中：`j/k` 选点，`h/l` 调 PWM ±1，`H/L` ±10，`[`/`]` 调温度 ±1，`c` 切 CPU/GPU，`a` 写入，`Esc` 取消
- 非编辑：`j/k` 选预设，`p` 应用，`s` 保存当前，`x` 删除，`d` 恢复默认，`e` 启用/停用

小键盘：NumLock 关闭时终端直接发方向键；NumLock 开启时，如果终端支持 Kitty 键盘协议（kitty / foot / WezTerm / Alacritty），会识别为 KEYPAD 并把 `8/2/4/6/5` 映射为 上/下/左/右/确认。不支持该协议的终端请关闭 NumLock。

灯光页：`b` 亮度，`e/E` 下一个/上一个灯效，`c` 选颜色，`S` 应用静态色。

网络页：`v` 切换 连接/Wi-Fi/VPN，`j/k` 选择，`Enter` 连接或编辑 IPv4，`u` 激活，`d` 断开，`x` 删除，`o` 无线开关，`t` 热点，`s` 重新扫描 Wi-Fi。

IPv4 编辑：`j/k` 选择字段，`Enter` 切换方法/下一项，直接输入地址/网关/DNS，`Backspace` 删除，`Ctrl+U` 清空，`a` 应用，`Esc` 取消。

## 配置

首次运行生成 `~/.config/rsztui/config.toml`，包含：

- 三套功耗预设（静音/均衡/性能）
- 保存的风扇预设（原始 PWM 值）
- 界面偏好

即使以 sudo 运行，也会通过 `SUDO_USER` 写入真实用户的家目录，而不是 `/root`。

## 持久化

- 性能模式、电池上限、PPT/TGP 都写入 `/etc/asusd/asusd.ron`，由 **asusd** 在开机和 AC/电池切换时自动恢复；不需要额外的 systemd 服务。
- 功耗值写入**当前供电模式**（AC 或电池）下、**当前性能模式**的 tuning。想改电池那一套，就拔掉电源再调。
- 风扇曲线由 asusd 持久化在 `/etc/asusd/fan_curves.ron`。
- 网络配置由 NetworkManager 持久化在 `/etc/NetworkManager/system-connections/`。

**本工具不自动备份**（按你的要求）。需要留后路时手动：

```bash
sudo cp -a /etc/asusd /etc/asusd.bak
```

## 安全说明

- TUI 以 root 运行，请只在本机终端使用。
- Wi-Fi/热点密码不会放进命令行参数（会出现在 `/proc/*/cmdline`），而是写入临时内容后落到 `/etc/NetworkManager/system-connections/`（权限 0600、root），再 `nmcli connection reload`；不会保存到 rsztui 配置里。
- 网络修改前有二次确认；IP/网关/DNS 会做基本格式校验。

## 开发

```bash
# 非 root 调试只读功能（仅开发用）
RSZTUI_SKIP_ROOT_CHECK=1 ./target/debug/rsztui status

cargo test
cargo clippy --all-targets -- -D warnings
```

项目结构：

```
src/
  main.rs       入口与 sudo 检查
  cli.rs        clap 子命令
  app.rs        TUI 状态、事件、刷新
  ui.rs         ratatui 渲染
  asus.rs       asusctl 封装与解析
  nm.rs         NetworkManager 封装
  sys.rs        /proc、/sys 采样
  config.rs     TOML 配置
  util.rs       命令执行、nmcli 解析、家目录
```

## 已知限制

- 功耗/TGP 属性因机型而异，缺少的项自动隐藏；能力来源是 `asusctl armoury list`。
- GPU 模式修改必须重启，TUI 不会自动重启。
- 已有同名 Wi-Fi 配置时直接复用，暂不支持在工具内改密码；需要时先删除该连接再重连。
- 热点固定连接 id 为 `rsztui-hotspot`，TUI 内 SSID 固定；CLI 可指定 SSID。
- 网络只处理 IPv4；IPv6 仍由 NetworkManager 默认策略管理。
- DoT 依赖 NetworkManager + systemd-resolved；DoH 不原生支持。
- 温度列表依赖 `hwmon` 标签，不同机型差异较大。
- 风扇曲线编辑要求温度严格递增，相邻点会互相约束；PWM 范围 0-255。
- 不自动备份，写坏了需要手动恢复。
