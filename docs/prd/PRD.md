# 产品需求文档 (PRD) — headroom

## 0. 文档信息

| 项 | 内容 |
|---|---|
| 标题 | headroom：按模型/按规则的 AI 订阅额度余量监控器 |
| 作者 | Main (Claude, requirement-orchestrator) |
| 状态 | Draft · 待评审 |
| 版本 | v0.1 |
| 创建日期 | 2026-09-03 |
| 关联技术设计 | `docs/tech-design/tech-design.md` |
| 关联台账 | `.ai-work/ledger.md`（内部工件，不发布） |

## 1. 背景与问题

Claude、Codex 等订阅制编码额度按「滚动窗口」计量:5 小时窗口 + 每周上限,且**同账号多个模型共享部分窗口、部分窗口按模型档位单独设帽**（如 Claude 顶配档 `fable` 有专属周帽，Codex `spark` 有专属副窗口）。

现有工具（LimitDeck 等）只把账号拍平成几个百分比展示，**丢失了「哪个模型、受哪些规则约束、还剩多少」这一层**，且明确「不分类、不建议」。开发者因此无法在「某个模型档位快耗尽」时提前得知并改用同账号里更省的模型。

**本项目要回答的问题**：对每个模型档位，按它各自的约束规则（共享窗口 ∪ 该档专属窗口），算出真实可用余量、判断是否不可用、预测撞墙时间，并在越过阈值时提醒 + 给出切换建议。

## 2. 目标与非目标

**目标（v0）**
- G1 采集：从 `omp usage --json` 读取每账号全部限额窗口（含 scope、status、remaining、resetsAt）。
- G2 领域计算：按窗口 `scope`（shared / tier）推导每个模型档位的**约束窗口集**与**有效余量** = 约束窗口 remaining 的最小值。
- G3 可用性判定：任一约束窗口 `status != ok` 或有效余量为 0 → 该档不可用。
- G4 预测：基于本地采样历史估算燃烧率与撞墙 ETA（样本不足时优雅降级）。
- G5 展示：渲染「模型档位 × 约束窗口」矩阵 + 有效余量 + 重置时刻 + ETA + 告警。
- G6 提醒：越过 warn/critical 阈值或 ETA 逼近时，输出告警并给出「改用哪个更省的档位」建议。
- G7 只读、隐私优先：不改任何 omp 配置、不触发切换；只消费脱敏输出，本地仅存余量快照。

**非目标（v0，留待后续版本）**
- N1 自动切换 / 改写 omp 配置 / 触发 fallback（v1）。
- N2 全屏 TUI 交互（v0 用 CLI 矩阵 + watch 轮询；delivery 层可后续替换为 TUI，领域不变）。
- N3 型号级细分归因（Sonnet vs Haiku 在共享池内各烧多少，需接 omp 会话记账，v2）。
- N4 团队多账号池化调度（capacity 聚合视图，v2）。
- N5 直连各厂官方接口（v0 只经 omp，单一可信源）。

## 3. 需求（Requirements）

| 编号 | 需求 | 优先级 |
|---|---|---|
| R1 | 支持 provider 过滤（默认全部已认证账号，`--provider` 限定其一） | 必须 |
| R2 | 解析 omp usage 的真实契约：`reports[].limits[]{id,label,scope{shared\|tier},window{durationMs,resetsAt},amount{remaining},status}` | 必须 |
| R3 | 按 scope 推导每个模型档位的约束窗口集；`shared` 约束全部档位，`tier(t)` 只约束档位 t | 必须 |
| R4 | 有效余量 = 约束窗口 remaining 最小值；记录「哪个窗口是瓶颈」 | 必须 |
| R5 | 不可用判定（G3）与三级告警（Info/Warn/Critical），阈值可配 | 必须 |
| R6 | 本地历史采样（≤30 天），据此算燃烧率与撞墙 ETA；不足两点则不预测 | 必须 |
| R7 | 一次性渲染矩阵（默认）与 `watch` 轮询模式（可配间隔） | 必须 |
| R8 | 告警附切换建议：在同 provider 内指出仍有余量的更省档位 | 必须 |
| R9 | 隐私：不落盘凭证/邮箱/组织；历史只存 provider/account 前缀 + 窗口 id + 余量 + 时间戳 | 必须 |
| R10 | omp 缺失/未认证/超时 → 明确的可操作错误，不崩 | 必须 |

## 4. 验收场景（Acceptance Criteria）

- **AC1（采集+映射）**：对真实 `omp usage --json --redact` 输出（见 `tests/fixtures/`），解析出正确数量的窗口，`anthropic:7d:fable` 映射为 `LimitScope::Tier("fable")`，`anthropic:5h` 映射为 `LimitScope::Shared`。
- **AC2（约束规则）**：给定含 `5h(shared)/7d(shared)/7d:fable(tier)` 的快照，`fable` 档约束窗口 = 三者；`base` 档（无专属窗口的模型）约束窗口 = 仅两个 shared。
- **AC3（有效余量）**：`5h=72% / 7d=92% / 7d:fable=100%` ⇒ `fable` 有效余量=72%（瓶颈=5h）；若 `7d:fable=4%` ⇒ `fable` 有效余量=4%（瓶颈=7d:fable），而 `base` 仍=72%。
- **AC4（不可用）**：任一约束窗口 `status="unavailable"` ⇒ 该档 `available=false` 且产生 Critical 告警。
- **AC5（预测）**：注入两条时间递减样本 ⇒ 输出正的燃烧率与有限 ETA；单样本或非递减 ⇒ ETA=None，不误报。
- **AC6（告警+建议）**：`fable` 有效余量 ≤ critical 且 `base` 余量高 ⇒ Critical 告警且建议切到 `base`（Sonnet 档）。
- **AC7（只读）**：整个运行期不写任何 omp 配置文件、不调用切换；可用文件系统只读回验证（无写入 `~/.omp`）。
- **AC8（健壮）**：omp 不存在时报 `omp not found`；未认证报 `not authenticated`；均以非零退出码但不 panic。
- **AC9（发布）**：`cargo build --release` 通过；README 的安装与运行命令原样可跑；`cargo test` 全绿。

## 5. 度量

- 一次采集+计算+渲染 < 2s（omp 调用本身 ~0.5s）。
- 依赖数最小化（serde/serde_json/chrono/anyhow）。
- 领域层零 IO、可脱网单测覆盖核心算法（BindingRule/Headroom/Forecast/Alert）。
