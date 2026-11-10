# 命令与快捷键

所有写操作需要 root：`sudo rsztui ...`。`rsztui doctor` 可以在普通用户下运行。

## 全局

```bash
sudo rsztui                 # 启动 TUI
sudo rsztui status          # 状态汇总
sudo rsztui --json status   # JSON 输出
rsztui doctor               # 依赖/硬件检查（无需 sudo）
rsztui --json doctor
```

## 性能与功耗

```bash
sudo rsztui profile get
sudo rsztui profile set performance
sudo rsztui profile default ac performance
sudo rsztui profile default battery quiet

sudo rsztui power preset performance
sudo rsztui power apply --pl1 45 --pl2 55 --pl3 65
sudo rsztui power apply --gpu-tgp 70 --gpu-temp 80
```

写功耗前会先读 `/sys/class/firmware-attributes/asus-armoury/attributes/<项>/` 的 min/max/current：越界值会被夹到固件范围，超出 `min == max` 的项（例如核显模式下 `nv_tgp`、`nv_dynamic_boost`）直接跳过。每项写完会回读 `current_value` 校验，`power apply`/`power preset` 逐项输出 `已写入`/`跳过`/`失败`，不再因为某一项写不进而整条命令失败——全部失败时才返回非零，并在 stderr 提示「没有任何一项写进固件」。

## 电池

```bash
sudo rsztui battery info
sudo rsztui battery limit 80
sudo rsztui battery oneshot
```

## GPU

```bash
sudo rsztui gpu get
sudo rsztui gpu set hybrid --yes    # 需要重启
```

`integrated` / `hybrid` / `ultimate` 三档。

## 网络

```bash
sudo rsztui net status
sudo rsztui net wifi list
sudo rsztui net wifi connect "MyWiFi" --password 'secret'
sudo rsztui net wifi connect "MyWiFi" --password 'secret' --no-up
sudo rsztui net radio off
sudo rsztui net ip show wired
sudo rsztui net ip set wired --method manual \
  --address 192.0.2.10/24 --gateway 192.0.2.1 --dns 1.1.1.1,8.8.8.8
sudo rsztui net ip set wired --method auto --dns 1.1.1.1

sudo rsztui net vpn list
sudo rsztui net vpn up myvpn
sudo rsztui net conn up wired
sudo rsztui net conn delete old-connection
sudo rsztui net hotspot on --password 'hotspot123' --ssid rsztui-hotspot
sudo rsztui net hotspot off

sudo rsztui net ping 1.1.1.1
sudo rsztui net dns example.com
sudo rsztui net cache status
sudo rsztui net cache flush
sudo rsztui net cache flush-neighbors
sudo rsztui net cache size 4096
```

## 风扇

```bash
sudo rsztui fan list
sudo rsztui fan save 我的配置
sudo rsztui fan apply 我的配置
sudo rsztui fan delete 我的配置
sudo rsztui fan default
sudo rsztui fan enable
sudo rsztui fan disable
```

## 灯光

```bash
sudo rsztui aura status
sudo rsztui aura next
sudo rsztui aura prev
sudo rsztui aura static ff8800
```

`aura status` 显示当前模式、亮度、颜色、速度、区域和支持的模式。

## 服务

```bash
sudo rsztui service status
sudo rsztui service status sshd
sudo rsztui service start asusd
sudo rsztui service stop asusd
sudo rsztui service restart asusd
sudo rsztui service restart NetworkManager
sudo rsztui service enable asus-shutdown --now
sudo rsztui service disable asus-shutdown --now
```

服务名可以省略 `.service`。停止/重启 NetworkManager 会断网，停止 asusd 会失去 ASUS 控制。

## TUI 快捷键

### 全局

| 键 | 作用 |
|---|---|
| `Tab` / `Shift+Tab` | 切换页面 |
| `1`–`9` | 直接跳页 |
| `r` | 刷新 |
| `?` | 帮助 |
| `q` | 退出（无未应用更改直接退出；有则先列出并确认） |
| `Ctrl+C` | 立即退出 |
| `Home` / `End` | 跳到列表首/尾 |
| `PageUp` / `PageDown` | 列表翻 5 项 |

鼠标：点击导航/列表/操作栏/性能页各面板，点击列表行选中，双击条目执行默认操作（切换性能模式/应用预设/进入连接/应用 GPU/应用颜色），滚轮上下移动当前列表。滚轮停在可调数值上时直接调值：功耗 ±1 W、充电上限 ±5%、风扇 PWM ±8；取色圆盘、风扇作图区、充电上限滑条支持按住拖动。弹窗确认按 `Enter` 或 `y`，取消按 `Esc` 或 `n`，底部按钮也可点；性能/电池/服务/灯光/风扇页底部有可点操作栏。小键盘：NumLock 关闭时直接发方向键；NumLock 开启且终端支持 Kitty 键盘协议时，`8/2/4/6/5` 映射为上/下/左/右/确认。长文本弹窗（帮助/Ping/DNS）用 `j/k` 或 `PageUp/PageDown` 滚动，`Enter`/`Esc` 关闭。

### 显卡

只读监控页，按 PCI 设备分别展示集显与独显：型号、驱动版本、PCIe 链路、核心/热点/显存温度、负载、显存占用、功耗/频率与 runtime 状态。NVIDIA 卡用 `nvidia-smi` 读取（runtime 为 `suspended` 时不调用，避免唤醒），AMD/Intel 走 DRM + hwmon。该页没有可修改项。

### 性能

性能页包含「性能模式 / 功耗预设 / 功耗与 TGP / GPU 模式」四个可点击聚焦的面板；「默认模式」区域可直接点击 AC/电池的静音/均衡/性能芯片设置默认模式。

| 键 | 作用 |
|---|---|
| `f` | 在性能模式 / 功耗预设 / 功耗与 TGP / GPU 模式之间切换焦点 |
| `j/k` | 选择 |
| `Enter` | 应用模式或功耗 |
| `a` / `b` | 设为 AC / 电池默认 |
| `h/l` | 功耗微调；`H/L` 每次 5；滚轮停在数值上同样按 1 调 |
| `[` / `]` | 选择功耗预设 |
| `p` | 套用预设 |
| `g` / `G` | 选择 / 应用 GPU 模式 |

功耗面板显示的是**固件真值**：每行右侧 `[最小..最大]` 来自 `asus-armoury` 的 sysfs 文件，与 asusctl 报的 asusd 存盘配置可能不一致；存盘值落在固件范围外时该行会标黄「存盘值越界，应用会被固件拒绝」，`min == max` 的项显示「固件不可调」，滚轮/`h/l` 都不会改动它。应用时逐项写入并回读，状态栏按项列出「已写入 N 项 / 跳过 / 失败原因」，一项写坏不影响其它项。

第二行是 `asusd` 与 `amd-pstate` 的联动实况：`platform_profile`、`scaling_governor`、`energy_performance_preference`（EPP）与驱动。切换性能模式时 asusd 会同时写 ACPI `platform_profile` 和 cpu0 的 EPP（见 `/etc/asusd/asusd.ron` 的 `platform_profile_linked_epp` 与 `profile_*_epp`），`scaling_governor` 由内核保持 `powersave`；EPP 候选值里含 `performance`/`balance_performance`/`power` 时显示「联动正常」，否则提示「未检测到 EPP 联动」。

### 电池

`h/l` 调上限，`Enter` 应用，`o` 单次充满。上限进度条支持鼠标：点击按列直接设百分比、按住左右拖动连续调、滚轮 ±5。

### 风扇

| 键 | 作用 |
|---|---|
| `i` / `I` | 编辑 CPU / GPU 曲线 |
| `j/k` | 编辑中选点；非编辑时选预设 |
| `h/l` | PWM ±1；`H/L` ±10 |
| `[` / `]` | 温度 ±1 |
| `c` | 编辑中切换 CPU/GPU（有未写入修改时先确认） |
| `a` | 写入并启用曲线（写入后回读校验） |
| 鼠标 | 作图区任意位置按下即拖动最近的点，纵坐标换算 PWM；按住拖出面板仍继续 |
| `p` / `s` / `x` | 应用 / 保存 / 删除预设 |
| `d` / `e` | 恢复默认（并重新启用）/ 启用停用 |

编辑时 PWM 会被限制在相邻点之间（asusd 要求逐点不降），无法调出会被固件拒绝的曲线。`a` 写入后会用 `asusctl fan-curve` 回读比对，值不一致或未启用会直接报错，不再出现「提示写入成功但硬件没变」。大写 `I` 在开启 Kitty 键盘协议的终端里同样可用（Shift+i 会被归一化为 `I`）。

### 灯光

`b` 亮度，`e/E` 下一个/上一个灯效，`c` 选颜色，`S` 应用静态色，`C` 应用圆盘自定义色。

「自定义颜色」圆盘支持鼠标：点击或按住拖动取色（角度=色相，半径=饱和度），双击直接应用；底部显示十六进制值与角度/饱和度，右侧「应用自定义色」「静态预设」也可点。

### 网络

| 键 | 作用 |
|---|---|
| `v` | 切换 连接/Wi-Fi/VPN |
| `j/k` | 选择 |
| `Enter` | 连接或编辑 IPv4 |
| `u` / `d` | 激活 / 断开 |
| `x` | 删除连接 |
| `o` | Wi-Fi 开关 |
| `t` | 热点（可编辑 SSID/密码） |
| `P` | 修改已保存 Wi-Fi 连接的密码 |
| `s` | 重新扫描 Wi-Fi |
| `p` | Ping 测试 |
| `n` | DNS 查询 |
| `c` | DNS/邻居缓存面板 |

IPv4 编辑：`j/k` 选字段，`Enter` 切换方法/下一项，直接输入，`Backspace` 删除，`Ctrl+U` 清空，`a` 应用，`Esc` 取消。

热点：`t` 打开表单，`Tab`/`↑↓` 切换 SSID 与密码，`Enter` 开启（连接 id 固定为 `rsztui-hotspot`）；热点已开启时再按 `t` 会确认关闭。`P` 修改当前选中（或当前已连接）Wi-Fi 的密码，改完自动重连。

详情页的 IPv6 地址、网关和 DNS 为只读展示，配置仍由 NetworkManager 的默认策略管理。

缓存面板：`f` 清空 DNS 缓存，`F` 清空邻居缓存，`e` 修改 DNS 缓存上限（写入 `/etc/systemd/resolved.conf.d/rsztui-cache.conf` 并重启 resolved），`r` 刷新统计，`Esc` 关闭。

### 服务

| 键 | 作用 |
|---|---|
| `j/k` | 选择服务 |
| `s` | 启动 |
| `t` | 停止 |
| `R` | 重启（`r` 是全局刷新） |
| `e` / `d` | 启用 / 禁用自启 |
| `E` / `D` | 启用并立即启动 / 禁用并停止 |

## 开发

```bash
# 非 root 调试只读功能
RSZTUI_SKIP_ROOT_CHECK=1 ./target/debug/rsztui status

cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt

# 构建 pacman 包
make pkg
```

### 设置 / 外观

| 键 | 作用 |
|---|---|
| `t` | 切换配色主题（海洋/森林/日落/紫罗兰/灰阶） |
| `y` | 切换速率格式（二进制/十进制/比特） |

设置页的主题和速率芯片可以直接点击。
