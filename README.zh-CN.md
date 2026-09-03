# headroom

[English](README.md) · [简体中文](README.zh-CN.md)

**按模型、按规则的 AI 编码订阅额度余量监控。**

大多数额度工具只把订阅拍平成几个「账号级百分比」。`headroom` 回答你写代码时真正关心的问题:

> *对**每一个模型**、按**它自己**的规则,我还能用多少 —— 如果某个模型快用完了,我该切到哪个?*

它通过 [`omp`](https://github.com/can1357/oh-my-pi) 读取用量,按窗口的 **scope**(所有模型共享 vs. 某档专属)建模,算出每个模型档位的**约束最小值(binding minimum)**——于是 Opus/Fable 的周额度见底时,你不会误以为整个账号没了,而 Sonnet 其实还能用好几个小时。

`headroom` 是**只读**的:它绝不修改你的 `omp` 配置、也不做任何切换。它只负责观测、预测、告警。(自动切换是 v0 刻意的非目标,见[局限](#局限)。)

![headroom — 交互式 TUI(中文;按 `l` 切换 中/EN)](docs/assets/tui-zh.png)

## 解决的痛点

如果你用的是 **Claude**(或 **Codex**)订阅,你的额度**不是一个数字**。它是一个滚动的 **5 小时窗口** 加上**每周上限**,按 token 计量——而且关键在于:**所有模型共享一个池子,外加对高档位单独设的帽**(Claude 的 Opus / `fable`;Codex 的 `spark`)。

所以同一个账号、同一时刻,你选不同模型时**真实可用余量是不同的**。经典翻车场景:你把 Opus 的周额度烧光,Claude 开始拒绝服务,感觉像「我的 Claude 没额度了」——但其实 Sonnet 还能跑好几个小时。现有工具只显示一个平铺的账号级百分比,恰恰把你最需要的信息藏了起来:**现在我还能用哪个模型,还能用多久?**

## 效果

`headroom` 为每个模型算出**约束最小值**(它受约束的所有窗口里剩余量的最小者),标出**瓶颈窗口**,显示重置倒计时和基于燃烧率的**撞墙 ETA**;当某个模型快用完时,**在你撞墙之前**告诉你该切到哪个更省的模型:

```
Claude · f6* · max
  base   [██████████████░░░░░░]  72%  Claude 5 Hour           ↺4h12m  ~14h
  fable  [█░░░░░░░░░░░░░░░░░░░]   3%  Claude 7 Day (Fable)    ↺2d8h   ~36m

告警
  严重  Claude/f6* — fable 仅剩 3%(瓶颈:Claude 7 Day (Fable)) · 可切到 base(还剩 72%)
```

彩色条形 gauge、critical 时**整行标红**、交互式 TUI,支持中文或 English —— 全部只读(见[局限](#局限))。

## 前置要求

平台:macOS 或 Linux。桌面通知仅 macOS 支持。

需要装两样东西:

1. **omp**([Oh My Pi](https://github.com/can1357/oh-my-pi))—— 在 `PATH` 上且已登录。`headroom` 会调用 `omp usage --json --redact`,所以至少要登录一个 provider:
   ```bash
   omp            # 然后执行 /login,登录 Anthropic 和/或 OpenAI Codex
   omp usage      # 自检:应能打印出你的额度窗口
   ```
2. **Rust 工具链 1.88+**(从源码构建)。用 rustup 安装:
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
   . "$HOME/.cargo/env"     # 或直接新开一个终端
   cargo --version          # 期望 1.88 或更高
   ```

不需要其它系统库 —— 依赖(serde、serde_json、anyhow、ratatui、crossterm)都是纯 Rust,由 cargo 构建。

## 安装

### 预编译二进制 —— 推荐(免 clone、免 Rust)

macOS / Linux,一行搞定:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/tomtdhzz/headroom/releases/latest/download/headroom-installer.sh | sh
```

或从[最新 release](https://github.com/tomtdhzz/headroom/releases/latest)下载对应平台的 `.tar.xz`。

> macOS 首次运行未签名二进制可能被 Gatekeeper 隔离。若被拦:
> `xattr -dr com.apple.quarantine "$(command -v headroom)"`。

### 用 cargo(需要 Rust 工具链)

```bash
cargo install --git https://github.com/tomtdhzz/headroom
```

### 从源码(兜底)

```bash
git clone https://github.com/tomtdhzz/headroom
cd headroom
cargo install --path .   # 或:cargo run --release -- …
```

## 使用

```bash
# 一次性:渲染「模型 × 窗口」矩阵与告警,然后退出
headroom

# 限定单个 provider
headroom --provider anthropic
headroom --provider openai-codex

# 调整阈值(有效余量 %)
headroom --warn 25 --critical 8

# watch:按间隔轮询,只在新出现/升级的情况才提醒
headroom watch --interval 60

# 交互式 TUI:条形 gauge、↑↓/jk 选择、r 刷新、l 中/EN、q 退出
headroom tui --interval 60

# 中文显示(默认按系统 locale 自动;也可用 --lang 强制)
headroom --lang zh
```

选项:

| 参数 | 默认 | 含义 |
|---|---|---|
| `--provider <id>` | 全部 | 限定单个 provider(`anthropic`、`openai-codex`…) |
| `--warn <pct>` | 20 | 有效余量 ≤ 此值时告警(警告) |
| `--critical <pct>` | 5 | 有效余量 ≤ 此值时告警(严重) |
| `--interval <secs>` | 60 | `watch` / `tui` 自动刷新的轮询间隔 |
| `--no-desktop` | 关 | 关闭 macOS 桌面通知 |
| `--no-color` | 自动 | 关闭 ANSI 颜色(同时尊重 `NO_COLOR` 与非 TTY) |
| `--lang <zh\|en>` | 自动 | 显示语言;默认从 `LANG`/`LC_*` 自动识别 |

## 它如何读取额度

`omp usage --json` 会按账号返回一组额度窗口,每个窗口带一个 `scope`:

- `scope.shared = true` → 约束**所有**模型(5h 池、周上限)。
- `scope.tier = "fable"` → 只约束**该档位**(Opus 的周帽)。

`headroom` 为每个模型档位推导出它的**约束窗口集** = 所有共享窗口 ∪ 该档位的专属窗口,取其中剩余量最小者作为有效余量,并标出瓶颈窗口。任一约束窗口 `status != "ok"` 就把该档位标为不可用。**不硬编码任何 provider/档位名**——映射完全由 `scope` 驱动。

## 架构

轻量六边形架构;领域层纯净、无 IO。

```
delivery/{cli,tui,i18n} ─┐
main ─────────┤→ app(Evaluator 用例 + 端口)
              │        │
              │        └→ domain(LimitScope、LimitWindow、QuotaSnapshot、
              │                    BindingRule、Headroom、Forecast、Alert)
              └ adapters: omp_usage · history_file · clock · notify  ─┘(实现端口)
```

- `domain/` —— 统一语言与规则;零 IO;可脱网单测。
- `app/` —— `Evaluator`(轮询 → 记录历史 → 算余量 → 预测 → 告警)与 `UsageSource` / `HistoryStore` / `Notifier` / `Clock` 端口。
- `adapters/` —— `omp usage` 防腐层映射、XDG 缓存历史、系统时钟、stderr + macOS 桌面通知。
- `delivery/` —— CLI 条形渲染器与交互式 `tui`(ratatui),经 `i18n` 本地化(`--lang`,自动识别)。替换交付层不动核心。

另见 [`docs/prd/PRD.md`](docs/prd/PRD.md) 与 [`docs/tech-design/tech-design.md`](docs/tech-design/tech-design.md)。

## 隐私

只有「额度形状」的数据会落盘。本地历史位于 `$XDG_CACHE_HOME/headroom/history.json`(或 `~/.cache/headroom/history.json`),仅存 `provider|账号前缀|窗口id → (时间戳, 剩余%)`——**绝不**存凭证、邮箱、组织身份或 provider 原始响应。账号 id 用的是 `omp --redact` 已脱敏的前缀。

## 测试

```bash
cargo test          # 单元 + 契约 + 端到端(使用脱敏 fixture)
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## 局限

- **只读。** v0 只观测与告警,不自动切换模型/账号。把建议变成动作(生成 `omp` `modelRoles` / `retry.fallbackChains`,或开启 `usageAwareFallback`)计划放在 v1。
- **档位粒度。** `omp` 暴露到档位(tier)而非具体型号,所以余量按档位计算。共享池内的型号级归因(如 Sonnet vs. Haiku)需要会话记账,计划 v2。
- **预测需要历史。** ETA 要在 ≥2 个不同采样后才出现;单次读数显示 `—`。
- **窗口可能变化。** provider 的限额结构经常调整;`scope` 驱动的映射避免了硬编码档位,但新形态可能需要更新解析器。

## 路线图

刻意搁置的项,记录在案以保持只读内核的诚实。

- **v1 —— 让告警可执行(搁置中)。** 把建议变成动作:生成/修补 omp `modelRoles` 与 `retry.fallbackChains`,可选开启 `retry.usageAwareFallback` 做请求前降级。opt-in,绝不静默。
- **v2 —— 型号级归因。** 用 omp 会话记账把共享池消耗拆到具体型号(如 Sonnet vs. Haiku)。
- **v2 —— 多账号池化容量视图**,使用 omp 的 `capacity` 块。
- **也许 —— 绝对值 `≈Nk` 估算**,基于用户提供的每窗口预算(omp 目前只给百分比,任何绝对值都是显式估算)。

## 许可

MIT —— 见 [LICENSE](LICENSE)。

## 免责声明

独立项目,与 Anthropic、OpenAI 或 `omp` / Oh My Pi 作者无隶属或背书关系。provider 接口与订阅额度可能随时变化。
