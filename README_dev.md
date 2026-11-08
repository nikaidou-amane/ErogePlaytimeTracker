# 构建产物目录清理指南

本文说明 Rust 构建产物目录（`src-tauri/target`）里哪些内容属于可再生的中间产物、哪些是出安装包必需的产物，以及对应的清理方式与效果。

目标是让「清掉一大块空间」和「下次出包不用等全量重编」这两件事可以分开处理，而不是每次都在 `cargo clean` 和硬扛磁盘之间二选一。

> 本文只描述方法与效果，不记录具体路径与实测体积，避免随环境变化而失效。

---

## 1. 目录构成

`src-tauri/target` 下按 profile 分为 `debug`、`release` 两个构建目录，另有少量 cargo 状态文件和一个临时目录。

| 位置 | 内容 | 是否可再生 | 备注 |
| --- | --- | --- | --- |
| `debug/` 全目录 | `dev` / `test` profile 的全部产物 | 可再生 | 通常占据 target 的绝大部分空间 |
| `debug/incremental/` | rustc 增量编译缓存 | 可再生 | 随开发轮次持续累积，增长最快 |
| `debug/deps/` | dev profile 下各依赖 crate 的产物 | 可再生 | 删除后 dev 编译需重新编译依赖 |
| `debug/build/` | 本包 `build.rs`（tauri-build）的历史输出 | 可再生 | 每条历史记录一份，旧版本会累积 |
| `release/` 全目录 | `build` profile 的全部产物 | 可再生 | 出安装包所需，见第 2 节 |
| `release/bundle/` | 已生成的安装包 | 可再生 | 每次 build 重新产出，历史版本会累积 |
| `tmp/` | cargo 临时目录 | 可再生 | 一般为空，可删 |
| `.rustc_info.json`、`CACHEDIR.TAG` | cargo 状态标记 | 可再生 | 体积可忽略，删了会自动重建 |

---

## 2. 出安装包所必需的产物

`npm run tauri build` 使用 release profile，**不会读取 `debug/`**。因此：

- `debug/` 的清理对出包**没有影响**，只影响下一次 `npm run tauri dev`、`cargo test`、`cargo clippy` 的耗时。
- `release/` 中的依赖产物、库文件与可执行文件是链接与打包的直接输入，清理它们会把下一次出包变成一次全量重编。

本项目依赖了体积较大的原生推理相关 crate（ONNX / OCR），这类依赖的单个产物往往远大于普通 Rust 依赖，且同一 crate 会在 `deps/` 中存在多份历史副本。这使得「误清 release」和「全清 debug」的时间代价差别非常明显，值得按下面分档处理。

---

## 3. 分档清理方法

以下命令均在仓库根目录执行。执行前先确认没有正在运行的 `npm run tauri dev` 或 `cargo build`，否则文件被占用会导致部分删除失败。

已安装的桌面版程序运行时不占用构建目录，不需要先关闭。

### 方案 A：只清 debug，完全不影响出包

适用场景：只想回收空间，且近期不打算跑完整的 dev / test 流程。

```powershell
# 预览将被删除的内容，不实际执行
cargo clean --manifest-path src-tauri/Cargo.toml --profile dev --dry-run

# 实际执行
cargo clean --manifest-path src-tauri/Cargo.toml --profile dev

# 顺带清掉历史安装包
Remove-Item -LiteralPath src-tauri\target\release\bundle -Recurse -Force
```

效果：

- 回收 target 中的绝大部分占用（`debug` 通常是最大的一块）。
- release 侧的依赖产物与可链接产物原样保留，下一次 `npm run tauri build` 的耗时基本不变。
- 代价是下一次 `dev` 启动、`test`、`clippy` 需要重新编译依赖，属于一次性等待。

### 方案 B：保留 dev 依赖产物，只清缓存与废料

适用场景：仍在频繁开发或过 CI 验证，希望保留热编译基础，同时清掉纯累积型数据。

```powershell
# 增量编译缓存：只会被重建，不影响依赖产物
Remove-Item -LiteralPath src-tauri\target\debug\incremental -Recurse -Force

# 本包 build.rs 的历史输出目录
Get-ChildItem -LiteralPath src-tauri\target\debug\build -Directory -Filter 'eroge-playtime-tracker-*' |
  Remove-Item -Recurse -Force

# cargo 临时目录
Remove-Item -LiteralPath src-tauri\target\tmp -Recurse -Force -ErrorAction SilentlyContinue

# 历史安装包
Remove-Item -LiteralPath src-tauri\target\release\bundle -Recurse -Force
```

效果：

- 回收量小于方案 A，但能清掉增长最快的那一部分。
- `debug/deps` 中的依赖产物保留，下一次 dev 编译本包代码时不必重编全部依赖，只是失去增量加速。
- `build.rs` 会自动重跑，无副作用。

### 方案 C：全清

适用场景：切换工具链、怀疑产物损坏、需要一份干净基线。

```powershell
cargo clean --manifest-path src-tauri/Cargo.toml
```

效果：

- 回收 `target` 的全部占用，是三者中最彻底的。
- 下一次出包与开发都是全量重编，在本项目依赖规模下耗时明显，不建议作为日常清理手段。

---

## 4. 不建议的做法

- **不要用 `cargo clean --release` 来省空间。** 它会连 release 侧依赖产物一起删除，直接把下一次出包变成全量重编，收益与代价不成比例。
- **不要在出包前清理 `release/deps`、`release/build` 或 release 下的库文件。** 这些是链接与打包的输入，不是缓存。
- **不要在验证流程中途清理 `debug`。** 若手头还有待跑的 `cargo test` / `cargo clippy`，第一次执行会显著变慢，建议安排在验证完成之后。

---

## 5. 相关说明

- 清理只涉及生成物，`target` 本身已在版本控制忽略列表中，不产生 diff。
- 安装包输出仍固定为 release 侧的 bundle 目录，删除历史安装包不改变后续 build 的输出位置。
- 若要长期抑制 `debug` 的增长，可在 cargo 配置中关闭本项目的增量编译，或把 target 目录重定向到空间更充裕的磁盘，但两者都会改变本地开发习惯，需要单独评估。
