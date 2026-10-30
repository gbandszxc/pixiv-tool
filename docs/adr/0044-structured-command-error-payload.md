# ADR 0044：设置保存结构化错误载荷

- 状态：已接受（首批仅 `settings_save` 一条命令采用，其余命令渐进迁移）
- 日期：2026-10-30
- 关联 ADR：[ADR 0027](0027-storage-maintenance-diagnostics.md)（错误信息脱敏口径）
- 规格：[SPEC](../SPEC.md) §5.2（错误码对照表）/ §7（命令表）

## 背景

`settings_save` 的校验错误一直是中文自由字符串（`Err(String)`）。前端
`SettingsPanel.save()` 的 catch 靠中文子串匹配把错误归位到字段：
`if (message.includes("输出目录")) outputDirError.value = message`。这是
$impeccable 全局审查的遗留条目 5：en-US / ja-JP 界面下后端文案仍是中文，
子串匹配静默失效，错误只剩 Snackbar 一条提示，不再停靠到字段下方；同时
「前端按子串猜后端文案」没有任何类型或测试保护，改一处文案就断。

规划见 [`docs/plans/settings-error-codes.md`](../plans/settings-error-codes.md)。

## 决策

### 1. 载荷形状：`{ code, field, message }`

`src-tauri/src/ipc_error.rs` 新增共享载荷类型，`settings_save` 的 `Err` 改为它：

```rust
#[derive(Debug, Clone, Serialize)]
pub struct SettingsError {
    pub code: &'static str,          // 稳定错误码，前端 i18n 键 settings.errors.<code> 的依据
    pub field: Option<&'static str>, // 触发错误的设置键名（SPEC §5.2），整体性错误为 None
    pub message: String,             // 中文兜底文案：日志与 i18n 缺失时的降级显示
}
```

- **IPC 序列化契约已按源码核实**（免真机实验）：Tauri 2 对 command 的 `Err`
  值走 `impl<T: Serialize> From<T> for InvokeError`（`InvokeError(pub serde_json::Value)`），
  回传时 `serde_json::to_vec(&e.0)` 以 `APPLICATION_JSON` + error header 交给前端
  error callback（tauri 2.12 `src/ipc/protocol.rs`、`scripts/core.js` 的 `reject(e)`）。
  即前端 `catch` 收到的 rejection 就是 `{ code, field, message }` 纯对象；
  `field: None` 序列化为 `null`。
- `message` 保留中文兜底（后端不做 i18n），服务两个场景：日志可读、前端 i18n
  键缺失时降级显示。脱敏口径沿用 ADR 0027：不得含 Cookie / 凭据原文；
  `output_dir_create_failed` 的 message 含系统 io 错误（可能带路径），与既有
  行为一致，仅进日志与 Snackbar，不入库。

### 2. 首批错误码对照表（SPEC §5.2 同步维护）

| code | field | 触发点 |
|---|---|---|
| `output_dir_empty` | `output_dir` | 目录为空 |
| `output_dir_invalid` | `output_dir` | 控制字符 / Windows 非法字符 / 非字符串 / 指向已有文件 |
| `output_dir_create_failed` | `output_dir` | 目录创建失败（message 含系统错误原因） |
| `max_wait_invalid` | `max_wait_seconds` | 越界 / bool / 非整数 |
| `translation_timeout_invalid` | `translation_timeout_seconds` | 同上 |
| `image_cache_max_invalid` | `image_cache_max_mib` | 同上 |
| `detail_image_scale_invalid` | `detail_image_scale` | 越界 / 非数字 / NaN/inf |
| `novel_font_scale_invalid` | `novel_font_scale` | 同上 |
| `novel_bg_color_invalid` | `novel_bg_color` | 白名单外 / 非字符串 |
| `palette_invalid` | `theme_color` | 色板白名单外 / 非字符串 |
| `startup_page_invalid` | `startup_page` | 启动页白名单外 |
| `thumb_tier_invalid` | `thumb_quality_grid` / `thumb_quality_detail` / `thumb_quality_fullscreen` | 档位白名单外 |
| `filename_template_invalid` | 三个模板键 | 模板校验失败（明细语义进 message，code 到字段粒度） |
| `translation_models_invalid` | `translation_models` | 非数组 |
| `agent_int_invalid` | 对应运行参数键 | 区间表越界 / 类型错误 |
| `agent_bool_invalid` | 对应运行参数键 | 非布尔 |
| `agent_settings_invalid` | `None` | 交叉约束冲突（目标≤上限等，冲突涉及多键） |
| `show_r18_invalid` | `show_r18` | 非布尔 |
| `saucenao_key_invalid` | `saucenao_api_key` | 非字符串 |
| `translation_key_invalid` | `translation_api_key` | 非字符串 / 超长 / 控制字符 |
| `translation_settings_invalid` | `None` | 翻译模块聚合校验（协议、高级 JSON 越权等）未通过 |
| `settings_invalid` | `None` | patch 非对象 / 合并反解失败 |
| `save_failed` | `None` | settings.json 落盘失败 |
| `credential_save_failed` | `translation_api_key`（回滚失败为 `None`） | 凭据库写入失败 |
| `internal` | `None` | 状态锁中毒等内部错误 |

### 3. 前端：字段停靠 + i18n 双通道

- `api/tauri.ts` 新增 `errorPayload(err)` 取形函数与 `StructuredError` 类型；
  `errorMessage()` 增加结构化载荷分支（取 `message` 兜底），字符串路径不变。
- `SettingsPanel.save()` 的 catch 删除 `includes("输出目录")` 子串匹配，改为
  `field → 错误 ref` 映射表（`output_dir` / `max_wait_seconds` /
  `image_cache_max_mib` / 三个模板键）；字段文案优先 `t("settings.errors." + code)`，
  键缺失（`te()` 为假）回落后端 `message`。未映射 field 与非结构化错误维持
  Snackbar 兜底。「字段停靠 + Snackbar 双通道」「保存失败阻止整表写入」语义不变。
- locales 三语同批新增 `settings.errors.*`（上表 code 全集）。

### 4. 采用策略：settings_save 先行，其余命令渐进

- 本批只改 `settings_save`（含 `apply_settings_patch` 与 settings.rs 校验函数的
  返回类型）及其测试；其他命令的 `Err(String)` 不动。
- 后续命令迁移时复用 `ipc_error.rs` 的构造模式与前端 `errorPayload()`，各自按
  硬约束同步 SPEC 命令表。
- 已否决：字符串前缀协议（脆、无类型检查）；校验全搬前端（后端仍是设置写入
  真相源，目录可创建性等只有文件系统能判定）。

## 后果

- en/ja 界面下错误停靠到字段且为对应语言文案，不再依赖中文子串；后端文案调整
  不再破坏前端归位逻辑。
- 后端单测全部断言 `code` / `field`；中文串仅允许出现在 `message` 兜底文案与其
  断言中（验收 grep 通过）。
- `settings.rs` 校验函数返回类型从 `Result<_, String>` 变为 `Result<_, SettingsError>`；
  加载期归一（`load_or_init`）不经过这些函数（自包含回落逻辑），不受影响。
- 残余风险：新增校验点若忘记给 code，会以 `settings_invalid` 之外的裸字符串路径
  编译失败暴露（Err 类型收窄为 SettingsError，字符串分支已不存在），不存在静默
  退回子串匹配的通道。
