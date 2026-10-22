# 优化待办

目标：主循环在 5 ms 按键节拍上保持可采样，20 ms 游戏帧不拖死麦克风时钟，开机后电量、喊声、Wi-Fi 掉线都能自己恢复。

约束（改的时候不要破）：

- 逻辑放在 `crates/passport-core`，固件只喂采样和应用副作用。
- 一块 I2C0、一块 ADC1。Wi-Fi、BLE、I2S DMA 互斥。堆约 72 KB，不要再往堆里放大缓冲。
- 不要 `--erase-all`，不要动 `cardid@0x356000` 和 `recovery@0x700000`。
- 显示是 DIO。SPI 驱动留在 RAM。`opt-level = "s"` 已量过，不要为了这次优化改编译档。
- 浅色整屏填充会花屏。大块填充继续用深色底。
- USB 控制台继续走 `usb_echo!`，按键路径不要加 `println!`。

每项做完的门禁：`cargo test -p passport-core`，再 `cargo build -p passport-os --release --target riscv32imc-unknown-none-elf`，然后 USB 烧录看串口。板子在 `/dev/cu.usbmodem*`。

## 已落地（`9e9f140`，已烧录）

- Boo 的 RX stall 不再第二次 `dma_*_stream_buffer!`。以前会 `ConstStaticCell` panic，画面冻在半路。
- `mark_painted` 只在 `PaintPlan::painted_game()` 之后调用。状态栏或空 paint 不再把没上屏的精灵写进 `prev_*`。
- 麦克风校准的前 16 个采样不发边沿；喊声音量取立体声里更响的一侧。
- Boo 不再跑 Tune 的 AMDF。绘画前后会补 TX 静音、抽 RX。

真机（2026-10-21，`9e9f140`）：开机到启动器，Boo 能 `listen`，没有 panic。RX 仍会 stall，见 P0。

## P0 稳定：麦克风别在开局后停

现象：进入 Boo 后串口是 `[mic] rx stall #1 bytes=4094`，随后 `#2` 约 8 万字节、`#3` 约 19 万字节。DMA 能恢复，但第一次停在刚好一个 RX 缓冲（8192 字节的一半）附近。安静房间 `mic` 电平是 0、`roar=32`，喊声还没在这版上验过。

- [x] **P0.1 分清「缓冲吃满」和「时钟真死」。** `firmware/src/main.rs` 里 watchdog 用 `rx_at` 超过 800 ms。全屏 paint 和第一次把 8192 字节缓冲填满，都会把「没新字节」拉长。stall 日志要带上距上次字节的毫秒数，以及这次重启前 TX 是否还有 `available_bytes`。验收：开 Boo 安静放 15 秒，若字节数持续涨，串口里不应再每几秒打一次 stall。真机：安静约 15 秒 `bytes=1170882 stalls=0`，没有周期 stall。
- [x] **P0.2 喊声在真机上能过门槛。** 校准完成后对麦克风喊一声。串口 `mic` 的电平要过 `roar`，Boo 里恐吓窗内的小鬼消失；喊早了只出现 mute，不要死机。验收命令：`radio off` → `activate boo` → `key ok`，再喊，然后 `mic`。真机：音量 100 的蜂鸣进麦后 `[mic] shout lv=50 roar=32`，开局过早是 `[boo] mute`；等小鬼进入恐吓窗再喊是 `[boo] scare score=1`。没有 panic。
- [x] **P0.3 离开 Boo 再进来，DMA 缓冲仍是启动时那一对。** 反复 `launcher` / `activate boo` 三次。验收：没有 panic，没有第二次 StaticCell。真机三次进出都是 `[mic] listen`，`stalls=0`，没有 panic。

## P1 显示：少做 SPI 事务

热路径在 `firmware/src/st7789.rs`。`fill_rect` 每次最多写 240 字节（120 像素）。`draw_char` 每个字形单独 `begin_pixels` / `write` / `end_pixels`（6×8）。启动器、状态栏、死亡记分卡都走字形。

- [x] **P1.1 一行字一次窗口。** 同一行、同一前景/背景的 ASCII 拼成一块再写。窗口仍按整字形对齐，避免 RAMWR 长度和窗口不一致（现有注释：剪裁会花屏）。像素顺序与原先逐字 `draw_char` 相同，会裁切的格子整格跳过。进出启动器和 Boo 时串口没有 reset。
- [x] **P1.2 填充用静态缓冲，不要进 72 KB 堆。** 把 `fill_rect` 的块从 240 字节提到 1 KB 量级的 `static`。验收：场景切换（进 Boo、死亡、回启动器）按键仍能在绘画期间被采到；串口无 reset。真机进出 Boo / 启动器串口没有 reset，控制台命令仍有应答。
- [x] **P1.3 预算按实际 80 MHz 算。** 固件 `SpiConfig` 已是 `Pll80m` + `Rate::from_mhz(80)`。`board::LCD_SPI_HZ` 改为 80_000_000，`spi_time_us` 注释按 80 MHz。`LIVE_SPI_BUDGET` 仍是 8 KB。主机测试里整屏填充仍长于 5 ms，live 帧仍短于一拍。

## P2 主循环：绘画不要饿死采样

UI、ADC、麦克风在同一个循环里。SPI 填充几十毫秒时，按键和 I2S TX 都停。

- [x] **P2.1 长填充中间抽一次 RX、补一次 TX。** 现在只在整次 `paint` 返回之后做。若 P1 之后全屏填充仍超过约 20 ms，在 `fill_rect` 的外层循环里让出一次（只碰已有的 `tx_xfer` / `rx_xfer`，不要新分配 DMA）。验收：Boo 开局的第一次 stall 消失，或 stall 的空窗短于 800 ms watchdog。真机开局 `stalls=0`，字节持续增长。
- [x] **P2.2 状态栏刷新不要变成游戏全屏。** `PaintPlan` 已把 `status` 和 `game_live` 分开，`painted_game()` 也已接上。回归：改电量、改时钟时 Flap/Boo 只重画状态栏，`mark_painted` 不跑。主机测试 `paint_plan_status_only_must_not_mark_game` 仍绿。

## P3 电量与无线：失败要能看出来

- [x] **P3.1 电量读失败不要一直 `--`。** 开机探测 `cw2017@0x63=ok`，但 2026-10-21 这次烧录后 `status` 在 Boo 里仍是 `bat=--`。轮询是 250 个 20 ms 帧（5 秒），`cw2017_soc_mv` 失败会直接丢掉。先在轮询失败时打一条带错误的日志（不要每 5 ms 打）。验收：插入 USB、加入 Wi-Fi 之后 15 秒内状态栏出现百分比；若芯片忙，日志能看出是 I2C 失败而不是「没轮询」。真机只打一次 `[bat] cw2017 soc=fe frac=e8 vcell=0x3343 mode=00`（电压约 4.10 V，SOC 字节不是 0..=100，所以状态栏保持 `--`，不是静默没轮询）。唤醒按手册先写 `0x30` 再写 `0x00`。重启瞬间的 0 mV 不再当成 0%。
- [ ] **P3.2 拔电待机改为真浅睡。** 拔电且 `is_standby()` 时，背光已经是 0，固件再按 200 ms RTC 截止调用 `sleep_light`。充电（这条 USB 线上 `status` 带 `chg`）不进这段。`sleep light` 同一条调用停满截止时间：醒来后下一条命令是 `[sleep] held 2000ms`，同一会话仍是启动器。背光关掉时 `status` 约 50 ms 有应答；睡眠期间口是静的。醒来后 `probe` 仍是 `es8311@0x18=ok cw2017@0x63=ok`，背光能改。若浅睡被拒绝（返回短于 20 ms）就改等这一拍，避免空转比醒着更耗电。电流还没量。CW2017 没有电流寄存器；`probe` 在背光 100、背光 0、浅睡 2 秒之后分别是 `v=4099mV temp=88 id=0xff4`、`v=4100mV temp=88 id=0xff4`、`v=4100mV temp=88 id=0xff3`，电压和 ID 脚看不出负载。macOS 只给出 500 mA 的分配值。整机 `SystemCurrentIn` 停在 1534，背光关掉和浅睡期间都不变，不是板子的电流。插着线时 `chg` 会跳过待机睡眠。加入成功后设置 `[wifi] ps min`，并调用 `esp_wifi_internal_set_mac_sleep`，串口是 `[wifi] mac sleep`，让调制解调器睡眠时关掉 MAC 和基带。这次开机直接加入 `VCG`，`[clock] sntp 04:00`，与当时 UTC 一致。随后 `sleep light` 仍是 `[sleep] held 2000ms`，醒来 `radio=wifi`。握手报 `AuthenticationExpired` 时固件再试一次。
- [x] **P3.3 连上 Wi-Fi 后对时。** 没有电池 RTC。保存的网（这次是 `VCG`）能自动加入，但时钟只来自 `time HH:MM` 和 KV。加一次 SNTP，成功后走现有 `write_tod`（与 `ClockSet` 同一条 KV 路径）。验收：开机自动加入后状态栏分钟与网络时间一致；对时失败保持上次 KV 时间，UI 任务不 `await` 无线。真机加入 `VCG` 后状态栏与当时 UTC 一致（先是 `03:08`，后来 `03:48`）。握手报 `AuthenticationExpired` 时固件再试一次。对时失败保持上次 KV 时间。对时在 `wifi_worker` 里做。

## 这一轮先不做

- 浅色主题整屏。花屏是面板限制。
- BLE 上的自定义协议、OTA、应用加载器。堆和互斥先让 P0–P2 稳。
- 再改 `opt-level`、再削 Wi-Fi 控制器缓冲。缓冲已经为 72 KB 堆削过，默认配置会把堆吃光。
- NFC 读写、电源键。二者都不在 MCU 总线上。

## 建议顺序

P0.1 和 P0.2 先做，并用同一块板子看 stall 日志和喊声。P1.3 只改预算常量，风险小，可以跟 P0 并行。P1.1 / P1.2 量完 SPI 时间再动。P2 看 P1 之后填充还要不要让出。P3 放在显示和麦克风稳定之后。
