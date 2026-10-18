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
| `1`–`8` | 直接跳页 |
| `r` | 刷新 |
| `?` | 帮助 |
| `q` | 退出（有未应用更改时会列出并确认） |
| `Ctrl+C` | 立即退出 |
| `Home` / `End` | 跳到列表首/尾 |
| `PageUp` / `PageDown` | 列表翻 5 项 |

鼠标：点击导航/列表/操作栏/性能页各面板，点击列表行选中，双击条目执行默认操作（切换性能模式/应用预设/进入连接/应用 GPU/应用颜色），滚轮上下移动当前列表。弹窗底部有可点的确认/取消按钮；性能/电池/服务/灯光/风扇页底部有可点操作栏。小键盘：NumLock 关闭时直接发方向键；NumLock 开启且终端支持 Kitty 键盘协议时，`8/2/4/6/5` 映射为上/下/左/右/确认。

### 性能

| 键 | 作用 |
|---|---|
| `f` | 在性能模式 / 功耗预设 / 功耗与 TGP / GPU 模式之间切换焦点 |
| `j/k` | 选择 |
| `Enter` | 应用模式或功耗 |
| `a` / `b` | 设为 AC / 电池默认 |
| `h/l` | 功耗微调；`H/L` 每次 5 |
| `[` / `]` | 选择功耗预设 |
| `p` | 套用预设 |
| `g` / `G` | 选择 / 应用 GPU 模式 |

### 电池

`h/l` 调上限，`Enter` 应用，`o` 单次充满。

### 风扇

| 键 | 作用 |
|---|---|
| `i` / `I` | 编辑 CPU / GPU 曲线 |
| `j/k` | 编辑中选点；非编辑时选预设 |
| `h/l` | PWM ±1；`H/L` ±10 |
| `[` / `]` | 温度 ±1 |
| `c` | 编辑中切换 CPU/GPU |
| `a` | 写入曲线 |
| `p` / `s` / `x` | 应用 / 保存 / 删除预设 |
| `d` / `e` | 恢复默认 / 启用停用 |

### 灯光

`b` 亮度，`e/E` 下一个/上一个灯效，`c` 选颜色，`S` 应用静态色。

### 网络

| 键 | 作用 |
|---|---|
| `v` | 切换 连接/Wi-Fi/VPN |
| `j/k` | 选择 |
| `Enter` | 连接或编辑 IPv4 |
| `u` / `d` | 激活 / 断开 |
| `x` | 删除连接 |
| `o` | Wi-Fi 开关 |
| `t` | 热点 |
| `s` | 重新扫描 Wi-Fi |
| `p` | Ping 测试 |
| `n` | DNS 查询 |
| `c` | DNS/邻居缓存面板 |

IPv4 编辑：`j/k` 选字段，`Enter` 切换方法/下一项，直接输入，`Backspace` 删除，`Ctrl+U` 清空，`a` 应用，`Esc` 取消。

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
