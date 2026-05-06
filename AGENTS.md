# Bevy TA

基于 Bevy 的技术美术作品集与实时渲染实验场。

项目重点是实时 NPR 渲染、着色器 lookdev、运行时动画实验、风格化 VFX，以及面向美术调参和调试的工具流程。

## 当前范围

- 使用 Bevy 0.18.1。
- `assets/private/` 用于本地第三方资源和临时资源，并被 Git 忽略。
- 私有参考、临时笔记和不可公开来源记录放在被忽略的本地目录里。

## 常用命令

```sh
cargo run
cargo run --features brp_tools
cargo run --release --features brp_tools
scripts/bevy_ta_brp
cargo run --features inspector
cargo run --example npr_toon_ramp
```

## 工作流程

### 项目构建工作流程

1. 阅读理解 prompt，有必要时上网检索资料，查询业界或其他人的办法，不要首先写代码，而是在明确示意写代码的时候，才开始动代码。
2. 写代码，注意模块划分，避免模块行数太大，模块使用 `mod_name.rs + mod_name/` 的形式，而不是用 `mod_name/mod.rs` 的形式，还需要仔细考虑扩展性和开放性，而不是应付单次任务。
3. 进行测试，可以启动 app (可能需要申请 GPU 权限)，使用 MCP 控制。

### 参数调整工作流程

1. 阅读理解 prompt，有必要时上网检索资料，查询业界或其他人的办法，查看参考图，指出目标效果的实现办法。
2. 接入 MCP，检查 app 是否启动，或启动 app，新建 debug cameras 并摆放到位。
3. 调整参数后抓图查看，若效果不拟合目标，进一步尝试或修改，若拟合目标，则保存参数并结束。

### 提交工作流程

1. 执行 `cargo fmt`。
2. 使用 git 查看变更，以 git 输出为参考，给用户一则 commit message，但不要实际执行提交，而是交给用户来提交。

## MCP/BRP 调试

启用 `brp_tools` feature 后，app 会在 `127.0.0.1:15702` 开启 Bevy Remote Protocol，并注册项目专用调试入口。

Codex 通过项目内 `.codex/config.toml` 自动加载 `bevy-brp` MCP server。当前 `bevy_brp_mcp` 的 `brp_execute` 只支持内置 BRP method；Codex 调项目专用能力时应使用标准 `world_trigger_event`：

- `bevy_ta::mcp::McpSetOrbitCamera`: 按 `name` 或 `entity` 设置 `target`、`distance`、`yaw`、`pitch`。
- `bevy_ta::mcp::McpCapturePrimaryWindow`: 保存主窗口截图，参数为 `{ "path": "assets/private/captures/capture.png" }`。
- `bevy_ta::mcp::McpSetMaterialParam`: 按 `entity`、`node_name`、`shader_key` 或 `apply_all` 修改运行时材质参数。参数包含 `field`、`apply_all`，并且在 `number`、`boolean`、`vec4` 中只传一个值；公共字段使用 `toon.*`、`character_material.*` 路径，shader 专属字段使用如 `face_sdf.*` 的路径。若不传任何筛选条件，必须显式传 `apply_all: true`。
- `bevy_ta::mcp::McpSaveToonProfile`: 把当前 Toon 材质参数保存到 `.toon-model.ron`。默认根据 `BEVY_TA_CHARACTER_SCENE` 对应的 scene asset 路径推导 profile 路径，也可以显式传 `path`。
- `bevy_ta::mcp::debug_camera::McpCreateDebugCamera`: 新建一个调试摄像机，渲染到独立 offscreen image，不影响主窗口和 UI。
- `bevy_ta::mcp::debug_camera::McpSetDebugCamera`: 按 `name` 或 `entity` 修改调试摄像机的位置、目标点、分辨率或启用状态。
- `bevy_ta::mcp::debug_camera::McpCaptureDebugCamera`: 保存指定调试摄像机的 offscreen 画面。
- `bevy_ta::mcp::debug_camera::McpDeleteDebugCamera`: 删除一个或全部调试摄像机；删除会延迟几帧清理，避免渲染线程仍引用 view。

调试摄像机查询使用标准 `world_query`，过滤 `bevy_ta::mcp::debug_camera::McpDebugCamera` 组件即可。

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
- `bevy_ta/list_material_params`: 列出场景中的材质参数快照，返回公共 `toon`、`character_material` 分组，以及按 `shader_key` 展开的 shader 专属分组。
- `bevy_ta/set_material_param`: 按 `entity`、`node_name`、`shader_key` 或 `apply_all` 修改材质参数。若不传任何筛选条件，必须显式传 `apply_all: true`。例如 `{ "field": "toon.rim_strength", "value": 1.2, "apply_all": true }`，或 `{ "node_name": "Face.Face", "shader_key": "character_face_sdf", "field": "face_sdf.threshold_bias", "value": 0.08 }`。

### MCP/BRP 调试注意事项

- 建议使用调试用摄像机 `debug_camera` 优先于主摄像机，除非需要查看主窗口 UI。
- 不要将角色名字泄漏在 git 可见范围内。
- 记忆 `~/.codex/memories/` 有调试用摄像机机位推荐。
- 调试时也可以动一动光源，也不一定限制一个摄像机，可以多个摄像机同时使用。

## 命令行可用工具

- `gltf-transform`
