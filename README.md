# Bevy TA

基于 Bevy 的技术美术作品集与实时渲染实验场。

项目重点是实时 NPR 渲染、着色器 lookdev、运行时动画实验、风格化 VFX，以及面向美术调参和调试的工具流程。

## 当前范围

- 使用 Bevy 0.18.1。
- 当前是单 crate 项目，暂时不使用 Cargo workspace。
- `assets/private/` 用于本地第三方资源和临时资源，并被 Git 忽略。
- 私有参考、临时笔记和不可公开来源记录放在被忽略的本地目录里。
- 早期优先做 NPR 卡通材质、描边、lookdev 场景和后处理。

## 常用命令

```sh
cargo run
cargo run --features brp_tools
cargo run --release --features brp_tools
scripts/bevy_ta_brp
cargo run --features inspector
cargo run --example npr_toon_ramp
```

## MCP/BRP 调试

启用 `brp_tools` feature 后，app 会在 `127.0.0.1:15702` 开启 Bevy Remote Protocol，并注册项目专用调试入口。

Codex 通过项目内 `.codex/config.toml` 自动加载 `bevy-brp` MCP server。当前 `bevy_brp_mcp` 的 `brp_execute` 只支持内置 BRP method；Codex 调项目专用能力时应使用标准 `world_trigger_event`：

- `bevy_ta::mcp::McpSetOrbitCamera`: 按 `name` 或 `entity` 设置 `target`、`distance`、`yaw`、`pitch`。
- `bevy_ta::mcp::McpCapturePrimaryWindow`: 保存主窗口截图，参数为 `{ "path": "assets/private/captures/capture.png" }`。
- `bevy_ta::mcp::McpSetToonParam`: 按 `entity`、`node_name` 或 `apply_all` 修改 Toon shader 参数。参数包含 `field`、`apply_all`，并且在 `number`、`boolean`、`vec4` 中只传一个值。
- `bevy_ta::mcp::McpSaveToonProfile`: 把当前 Toon 材质参数保存到 `.toon-model.ron`。默认根据 `BEVY_TA_CHARACTER_SCENE` 对应的 scene asset 路径推导 profile 路径，也可以显式传 `path`。

真实资产调试建议在有桌面/GPU 的终端中启动 app，Codex 只通过 BRP 连接已运行进程。`scripts/bevy_ta_brp` 默认使用 release 构建，并透传 app 使用的环境变量：

```sh
BEVY_TA_CHARACTER_SCENE='private/character_source/角色/角色.glb#Scene0' \
BEVY_TA_CHARACTER_SCALE=5 \
BRP_EXTRAS_PORT=15702 \
scripts/bevy_ta_brp
```

直接用 JSON-RPC/curl 调试时，也可以调用项目自定义 BRP method：

- `bevy_ta/list_cameras`: 列出相机和 orbit 参数。
- `bevy_ta/set_orbit_camera`: 按 `name` 或 `entity` 设置 `target`、`distance`、`yaw`、`pitch`。
- `bevy_ta/capture_primary_window`: 保存主窗口截图，参数为 `{ "path": "assets/private/captures/capture.png" }`。
- `bevy_ta/list_toon_materials`: 列出场景中的 `ToonMaterial` 参数。
- `bevy_ta/set_toon_param`: 按 `entity`、`node_name` 或 `apply_all` 修改 Toon shader 参数，例如 `{ "field": "rim_strength", "value": 1.2, "apply_all": true }`。
