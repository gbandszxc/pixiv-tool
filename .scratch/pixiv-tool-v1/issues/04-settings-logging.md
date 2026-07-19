# 04 — Settings JSON 配置 + 日志

**What to build:**

从用户视角：App 启动时读取 `config/settings.json` 加载用户配置（输出目录、输出格式、语言、主题等），其他模块通过 `Settings` 单例访问配置值。同时配置 Python logging，把 INFO 级以上日志写入 `data/logs/app.log`，dev 模式同时输出到 console，方便开发者调试和用户报错时提供日志。

**Blocked by:** None — 可立即开始

**Status:** ready-for-agent

**Acceptance criteria:**

- [ ] `backend/storage/settings.py` 提供 `Settings` 单例（dataclass + 工厂）
- [ ] 字段对应 SPEC §5.2：output_dir (默认 "downloads")、output_formats (默认 ["txt", "markdown"])、language (默认 "zh-CN")、theme (默认 "auto")、backend_port (默认 null)
- [ ] `config/settings.json` 不存在时用默认值创建
- [ ] 存在但字段缺失时用默认值填充缺失字段（向后兼容）
- [ ] 存在但 JSON 损坏时备份原文件为 `settings.json.corrupt-<timestamp>` 后用默认值
- [ ] 提供 `save()` 方法持久化当前配置到文件
- [ ] `backend/logging_config.py` 配置 root logger：
  - [ ] FileHandler 写 `data/logs/app.log`，UTF-8 编码，INFO 级
  - [ ] dev 模式额外加 StreamHandler 输出到 console
  - [ ] 格式：`%(asctime)s [%(levelname)s] %(name)s: %(message)s`
- [ ] 敏感字段（PHPSESSID 等）在日志中被 filter 自动掩码（仅前 8 位）
- [ ] V1 不做日志滚动（RotatingFileHandler 留 V2）
- [ ] 单元测试：settings 往返、默认值填充、损坏文件恢复
- [ ] `config/settings.json` 已加入 `.gitignore`，但保留 `config/settings.example.json` 模板（本 ticket 提供）
