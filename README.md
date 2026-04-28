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
cargo run --features inspector
cargo run --example npr_toon_ramp
```
