# Quiet CLI 设计

## 目标

`quiet` 是 Quiet.md 的本地 CLI companion。它复用 `fdl_core` 的文档模型、DocxXML 读写、Markdown 转换和 Store 写入路径；不直接改 `index.json` 或拼接 XML 绕过原子写入/锁。

命令交互沿用 `lark-cli` 的 Agent-friendly 约定，但业务范围只覆盖 Quiet.md 的本地能力：

- 根命令和业务域 `--help`；
- `+shortcut` 人类/Agent 友好命令；
- 结构化 JSON 默认输出，另有 `pretty`、`table`、`ndjson`、`csv`；
- `schema` 自省参数、风险和示例；
- 写操作支持 `--dry-run`，删除/清空等高风险写操作要求 `--yes`；
- stdout 只放成功数据，stderr 只放错误；非零退出表示失败。

不复制飞书云端能力。消息、日历、邮箱、审批、OKR、云端权限和 OAuth 不属于 Quiet 的本地产品边界。

## 命令树

```text
quiet <domain> <command> [flags]

Global flags:
  --library <path>       使用指定 Quiet.md 文档库；默认 QUIET_MD_HOME 或 ~/Documents/Quiet.md
  --format <json|pretty|table|ndjson|csv>
  --jq <expr>            对 JSON 数据执行有限字段选择（不执行 shell）
  --dry-run              只预览写操作
  --yes                  确认 high-risk-write
  --help / --version

docs:
  +list                  列出文档，可按 query/parent/pinned/starred 过滤
  +get                   读取文档元数据或内容
  +create                 从标题、Markdown、DocxXML 或 stdin 创建文档
  +update                 用 Markdown 或 DocxXML 原子替换正文
  +append                 追加 Markdown 内容
  +metadata               设置图标和封面
  +rename                 重命名
  +duplicate              创建副本
  +move                   移动到文档树节点或根
  +delete                 移入回收站
  +search                 全文搜索
  +import                 导入 Markdown/TXT/DocxXML
  +export                 导出 Markdown/DocxXML/HTML/PDF
  +pin / +star             置顶/收藏
  +width                  设置 standard/wide/full
  +reading                设置阅读模式
  +catalogue              设置目录展开状态
  +info                   设置文档信息行
  +refs                   设置引用列表显示
  +links                  列出反向引用
  +stats                  文档统计

library:
  path / status / tree / doctor / validate / settings

trash:
  list / restore / purge / empty

assets:
  list                   列出文档引用的图片和附件

emoji:
  search                 使用 Quiet 与 GUI 相同的 emoji 数据和匹配规则

schema:
  列出或读取命令 schema，供 Agent 在执行前检查参数和风险

app:
  open                   打开 GUI 应用（不改库）
```

## 数据契约

默认成功响应：

```json
{
  "ok": true,
  "identity": "local",
  "data": {},
  "meta": {"library": "...", "count": 0}
}
```

默认失败响应写 stderr：

```json
{
  "ok": false,
  "identity": "local",
  "error": {
    "type": "usage|validation|io|conflict|unsupported",
    "message": "...",
    "hint": "..."
  }
}
```

列表数据使用稳定字段 `id`、`title`、`created`、`modified`、`opened`、`pinned`、`starred`、`parent`、`width`、`reading`。文档正文以 `markdown`、`xml` 或 `plain_text` 明确标识，不把大正文隐式塞进列表响应。

## 风险模型

| 操作 | 风险 | 规则 |
|---|---|---|
| list/get/search/export/doctor/validate | read | 直接执行 |
| create/update/append/rename/move/pin/star/width/settings | write | `--dry-run` 可预览 |
| delete/restore/purge/empty/import-replace | high-risk-write | 必须 `--yes`；`--dry-run` 不写盘 |

所有 Store 写入复用短 `.lock`、临时文件 + rename、index backup 和已有冲突检测。CLI 不碰真实默认库的测试数据；测试统一设置 `QUIET_MD_HOME` 到临时目录。

## 实现边界

- Markdown 和 XML 是完整内容入口。XML 用于所有 GUI 支持但 Markdown 没有表达的 block（公式、按钮、提醒、倒计时、封面、附件、分栏、合并表格和未知扩展）。
- HTML 导出由 CLI 提供自包含、无网络依赖的渲染；PDF 先生成同一 HTML，再使用系统已有转换器（macOS `cupsfilter` 或 PATH 中的 `pandoc`）并在工具不可用时返回明确 `unsupported`，不伪造成功。
- `app open` 不自动激活或修改文档；GUI 的快捷键、主题、窗口状态仍由 GUI 管理。
- CLI binary 名为 `quiet`，GUI binary 保持 `quiet_md`，所以安装后两者并存。

## 验收

1. `quiet --help`、每个域的 `--help`、`quiet schema` 可读。
2. 临时库中完成 create → list → get → update/append → search → export → rename/move → delete → trash restore/purge 的闭环。
3. XML round-trip 通过现有 core fixture；CLI 写入不破坏 GUI 可读性。
4. JSON 成功/错误 stdout/stderr、退出码、`--dry-run` 和 `--yes` 行为固定。
5. `cargo test --release -p fdl_core`、CLI 测试和 `scripts/check.sh` 通过。
