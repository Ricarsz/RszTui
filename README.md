# rogtui

ASUS TUF 的终端控制台。Rust + [ratatui](https://ratatui.rs/)，底层调用 `asusctl` 和 NetworkManager 的 `nmcli`。

目标是替代 `rog-control-center` 的日常功能：切性能模式、限电池、调功耗、看风扇/温度、管网络。

## 当前状态（v0.1 MVP）

已实现：

- **仪表盘**：CPU/内存/网络实时曲线、温度列表、电池状态、ASUS 状态汇总
- **性能**：Quiet/Balanced/Performance 切换、AC/电池默认模式、GPU 三档（集显/混合/独显直连）
- **功耗**：PL1/PL2/PL3、dGPU TGP、Dynamic Boost、温度墙，预设 + 手动微调
- **电池**：充电上限 20–100%、单次充满、容量/健康度/循环/功率
- **风扇**：只读展示当前曲线与启用状态
- **灯光**：键盘亮度切换（off/low/med/high）
- **网络**：只读展示设备、连接状态、Wi-Fi 开关与 SSID
- **CLI**：与 TUI 同能力的子命令，支持 `--json`

下一批：

- 网络写入：Wi-Fi 扫描/连接（密码走临时 keyfile，不落 argv）、DHCP/静态 IP、网关、DNS/DoT、连接增删改、VPN 启停、热点
- 风扇：套用/保存预设、导入导出曲线
- 灯光：Aura 灯效切换（static/breathe/rainbow/…）
- 温度和功耗的更多传感器、图表缩放

## 依赖

- `asusctl` / `asusd`（Arch：`asusctl` 包），并已启用 `asusd.service`
- NetworkManager（`nmcli`）
- 本工具**必须以 root 运行**：`sudo rogtui`

`asusctl` 的普通用户操作走 asusd 的 D-Bus；但本工具还会写 `/var/backups/rogtui`、调 `nmcli`，所以统一要求 sudo。启动时如果不是 root 会直接提示。

## 构建与安装

```bash
cd rogtui
make            # cargo build --release
sudo make install   # 安装到 /usr/local/bin/rogtui
```

或者：

```bash
cargo build --release
sudo install -Dm755 target/release/rogtui /usr/local/bin/rogtui
```

卸载：

```bash
sudo make uninstall
```

## 使用

```bash
sudo rogtui                 # 启动 TUI
sudo rogtui status          # 状态汇总
sudo rogtui --json status   # 给脚本/waybar 用
```

常用子命令：

```bash
sudo rogtui profile get
sudo rogtui profile set performance
sudo rogtui profile default ac performance
sudo rogtui profile default battery quiet

sudo rogtui battery info
sudo rogtui battery limit 80
sudo rogtui battery oneshot

sudo rogtui power preset performance
sudo rogtui power apply --pl1 45 --pl2 55 --pl3 65
sudo rogtui power apply --gpu-tgp 70 --gpu-temp 80

sudo rogtui gpu get
sudo rogtui gpu set hybrid --yes   # 需要重启

sudo rogtui config          # 查看配置路径与预设
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

灯光页：`b` 切换键盘亮度。

## 配置

首次运行会生成：

```
~/.config/rogtui/config.toml
```

包含三套功耗预设（静音/均衡/性能）和界面偏好。即使以 sudo 运行，也会通过 `SUDO_USER` 写入真实用户的家目录，而不是 `/root`。

修改预设后重启 TUI 生效。`rogtui config` 可以查看当前预设。

## 持久化与备份

- 性能模式、电池上限、PPT/TGP 都写入 `/etc/asusd/asusd.ron`，由 **asusd** 在开机和 AC/电池切换时自动恢复；不需要额外的 systemd 服务。
- 功耗值写入**当前供电模式**（AC 或电池）下、**当前性能模式**的 tuning。想改电池那一套，就拔掉电源再调。
- 每次写入前自动备份到 `/var/backups/rogtui/<时间戳>/`，包含 `asusd.ron`、`fan_curves.ron`、`aura_tuf.ron`。
- 恢复备份：停掉 asusd，把文件拷回 `/etc/asusd/`，再启动 asusd。

## 安全说明

- TUI 以 root 运行，请只在本机终端使用。
- 下一批的 Wi-Fi 连接不会把密码放进命令行参数（会出现在 `/proc/*/cmdline`），而是写 0600 临时 keyfile 后 `nmcli connection load`，加载完立即删除。
- 所有修改都有二次确认，并在写入前备份 asusd 配置。

## 开发

```bash
# 非 root 调试只读功能（仅开发用）
ROGTUI_SKIP_ROOT_CHECK=1 ./target/debug/rogtui status

# 把备份写到可写目录，方便测试写入
ROGTUI_SKIP_ROOT_CHECK=1 ROGTUI_BACKUP_DIR=/tmp/rogtui-backup \
  ./target/debug/rogtui power apply --pl1 55

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
  sys.rs        /proc、/sys 采样
  config.rs     TOML 配置
  util.rs       命令执行、备份、家目录解析
```

## 已知限制

- 功耗/TGP 属性因机型而异，缺少的项会自动隐藏；`asusctl armoury list` 是能力来源。
- GPU 模式修改必须重启，TUI 不会自动重启。
- 风扇曲线目前只读；应用预设下一批实现。
- 网络页目前只读。
- 温度列表依赖 `hwmon` 暴露的标签，不同机型差异较大。
