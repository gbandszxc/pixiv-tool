# 设置保存结构化错误载荷实施计划

- 日期：2026-10-30
- 状态：已实施（2026-10-30，见 ADR 0044；实施时以 tauri 2.12 源码路径核实了 object
  Err 的回传形状，未走真机实验）
- 来源：$impeccable audit 全局审查遗留条目 5（设置保存失败按中文子串匹配归位，英文/日文界面下静默失效）。
- 规格入口：[`../SPEC.md`](../SPEC.md) §7（命令表）、§5.2（settings.json 键）；实施时**须新开 ADR 0044**（触发条件：AGENTS.md「新增外部契约——IPC 契约」）。
- 本轮范围：方案设计与文档落盘，不修改运行代码。

## 1. 目标与边界

`settings_save` 的校验错误从中文自由字符串改为结构化载荷（错误码 + 设置键 + 兜底文案），前端按 `field` 停靠字段错误、按 `code` 取 i18n 文案；顺带修复 en/ja 界面下错误无法归位字段的问题。

| 项目 | 决定 |
|---|---|
| 改动范围 | 仅 `settings_save` 一条命令及其测试；其他命令的 `Err(String)` 不动 |
| 错误类型归属 | `src-tauri/src/` 新共享模块（如 `ipc_error.rs`），供后续命令渐进采用 |
| 前端落点 | `api/tauri.ts` 的 `errorMessage()` 识别新形状；`SettingsPanel.save()` 的 catch 改字段映射表，删除子串匹配 |
| 用户可见文案 | 按 code 走 i18n（`settings.errors.<code>`，三语同批）；后端 `message` 字段仅作日志与兜底 |
| 兼容策略 | 前后端同仓同包发布、无外部消费者，单批切换；`errorMessage()` 保留字符串兜底 |
| 已否决 | 字符串前缀协议（脆、无类型检查）；校验全搬前端（后端仍是设置写入真相源，目录可创建性等只能后端判） |

## 2. 现状证据（实施前应再次核对行号）

- `frontend/src/components/settings/SettingsPanel.vue` `save()` 的 catch：`if (message.includes("输出目录")) outputDirError.value = message;`——注释自述「后端 validate_output_dir 的中文文案都含『输出目录』」；en/ja 界面下该停靠静默失效（错误仅剩 Snackbar）。
- `src-tauri/src/commands/settings_cmds.rs` 各校验点均返回中文 `Err(String)`：「输出目录不能为空」「阅读背景色无效」「主题色板无效」「应用启动页无效」「列表/详情/全屏缩略图档位无效」及文件名模板校验的中文消息（「未知占位符：…」「模板不能为空」等）；文件头注释（:11-12）与单测断言（:365 `assert_eq!(err, "输出目录不能为空")`）同样绑定中文串。
- `frontend/src/api/tauri.ts:48` `errorMessage()`：invoke 拒绝值为字符串时透传；对象分支仅兼容旧 axios 形状，无 code 概念。
- 前端已有结构化校验先例可对齐落点形态：`validateTranslationAgent()` 返回 `issues[]`（携带定位 key 与 i18n key），失败时「切二级分类 + 滚动 + 聚焦出错字段」链路成熟。
- 设置表单的保存预校验目前是「前端先拦 + 后端兜底」双层：前端拦截已覆盖空值/非法字符/数字区间/模板语法，后端独占的是「目录是否可创建」这类只有文件系统能判定的事实——结构化改造只影响后端兜底层。

## 3. 方案设计

### 3.1 后端：`SettingsError` 载荷

```rust
#[derive(Serialize)]
pub struct SettingsError {
    pub code: &'static str,          // 稳定错误码，前端 i18n 键的依据
    pub field: Option<&'static str>, // 对应 SPEC §5.2 的设置键名，前端字段停靠依据
    pub message: String,             // 中文兜底文案（日志 / i18n 缺失时的降级显示）
}
```

- Tauri 2 command 的 `Err` 类型需可序列化：object 拒绝值会原样到达前端 `catch` 的 rejection（需一次真机验证形状，见 §6）。
- 首批 code / field 对照表：

| code | field | 触发点 |
|---|---|---|
| `output_dir_empty` | `output_dir` | 目录为空 |
| `output_dir_invalid` | `output_dir` | 含非法字符 |
| `output_dir_create_failed` | `output_dir` | 目录创建失败（含失败原因于 message） |
| `novel_bg_color_invalid` | `novel_bg_color` | 阅读背景色键非法 |
| `palette_invalid` | `theme_color` | 主题色板非法 |
| `startup_page_invalid` | `startup_page` | 启动页非法 |
| `thumb_tier_invalid` | `thumb_tier_list/grid/detail/fullscreen` | 三档缩略图档位非法 |
| `filename_template_invalid` | `illust/novel/novel_series_filename_template` | 模板校验失败（非法占位符等） |

- 值域校验的其余键（语言、输出格式、图片缓存上限等）按同模式顺带补齐；文件名模板的「须包含 {order}」等明细语义放 message，code 粒度到字段即可。

### 3.2 前端：识别、停靠与 i18n

- `api/tauri.ts`：`errorMessage()` 增加对象分支——识别 `{ code, field, message }` 形状时返回 `message`（兜底不变）；新增 `errorPayload(err)` 之类的取形函数供设置面板取 `code/field`。
- `SettingsPanel.save()` catch：删除 `includes("输出目录")`；改为 `field → 错误 ref` 映射表（`output_dir → outputDirError`、模板三键 → `templateErrors.*`、其余数字/枚举键 → 对应字段 error 态），未映射的 code 走 Snackbar 兜底；字段文案 `t("settings.errors." + code)`，缺失键回落后端 `message`。
- locales 三语新增 `settings.errors.*` 文案（首批 8 个 code × 3 语言）。
- 保留现行「字段停靠 + Snackbar 双通道」与「保存失败阻止整表写入、留在弹窗」语义。

### 3.3 ADR 0044 与文档同步（硬约束）

- **ADR 0044**（编号接现行 0043）：记录「结构化命令错误载荷」决策——载荷形状、首批 code 表、「仅 settings_save 先行、其余命令渐进采用」的策略、错误信息脱敏口径（沿用 ADR 0027：message 不得包含 Cookie / 凭据；输出目录路径按现行日志脱敏规则核对）。
- SPEC §7 `settings_save` 行补错误语义说明；若 §5.2 对各键的校验规则有列示，同步 code 对照。
- DESIGN.md 不涉及（无视觉变化）。

## 4. 实施拆解

| 步骤 | 内容 | 预估 |
|---|---|---|
| 1 | Rust：`ipc_error.rs`（或并入 settings 模块）定义 `SettingsError`；`settings_save` 各校验点替换；单测断言从中文串改为 `code`/`field` | 0.5 天 |
| 2 | 前端：api 取形 + `errorMessage()` 扩展 + SettingsPanel 映射表 + locales ×3 | 0.5 天 |
| 3 | ADR 0044 + SPEC §7 同步 + 本文状态行更新 | 0.5 小时 |
| 4 | 回归：zh/en/ja 三界面各触发一次输出目录错误验证字段停靠与文案；`cargo test`（settings 相关）+ `pnpm build`；日志脱敏抽查 | 0.5 天 |

提交信息：`feat(settings): 设置保存错误改结构化载荷并按字段停靠`（前后端一批一个提交，符合「代码与文档同批改」）。

## 5. 验收标准

- en-US / ja-JP 界面下触发「输出目录为空 / 不可创建」：错误停靠到字段下方且为对应语言文案，不再依赖中文子串。
- 后端单测全部断言 `code`/`field`，不再出现中文串断言；`grep "输出目录" src-tauri/src/commands/settings_cmds.rs` 仅命中 message 兜底文案。
- 未映射错误仍可经 Snackbar 可读（字符串兜底路径不回归）。
- ADR 0044 落盘且 SPEC §7 与实现对齐。

## 6. 风险

- **Tauri 拒绝值序列化形状需先验证**：实施第 1 步先做一个最小 invoke 实验（返回 object Err），确认前端收到的 rejection 形状与文档一致，再铺开替换；若形状不合预期，回退方案是 `code` 序列化为 `"<code>|<field>|<message>"` 单串（前端拆分），契约仍写进 ADR。
- 双层校验语义漂移：前端预校验与后端 code 表须覆盖同一集合，实施时以本文 §3.1 表为对账清单。
- i18n 键遗漏：`settings.errors.*` 三语同批添加，代码侧以「缺失回落 message」兜底，不阻塞发布。
