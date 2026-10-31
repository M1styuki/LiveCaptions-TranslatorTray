# LiveCaptions TranslatorTray · Rust

基于 Windows 实时字幕的轻量桌面翻译工具，现已使用 Rust 重构。支持 Windows 11 x64，单个 EXE 运行，无需安装 .NET 或 Rust。

[下载最新版](https://github.com/M1styuki/LiveCaptions-TranslatorTray/releases/latest) · [使用说明](docs/USER-GUIDE.md) · [构建说明](docs/BUILDING.md) · [验证范围](docs/VALIDATION.md)

## 使用

1. 先在 Windows 中完成“实时字幕”的首次设置和所需语言下载。识别语言在 Windows 实时字幕中选择。
2. 下载并运行 `LiveCaptionsTray.exe`，程序驻留在系统托盘。
3. 右键托盘图标打开设置。首次直接显示设置页并创建默认配置，无需额外确认。
4. Google 可直接使用；DeepSeek 等兼容服务在 OpenAI 接口中填写地址、模型和密钥。
5. 左击托盘图标开启或关闭字幕翻译；完全退出请使用右键菜单的“退出”。

## 功能

- Google、Google2、OpenAI 兼容接口、OpenRouter、Ollama、LMStudio、DeepL、有道、百度、MTranServer、LibreTranslate。
- 每个服务支持独立配置；提供上下文翻译、调用间隔和显示句数设置。
- 可置顶的字幕浮窗，可调整字体、描边、背景和透明度。
- 字幕与译文历史、搜索、分页及全量 CSV 导出。
- 分组设置页、单服务 API 编辑页、短促交互动画。
- 托盘右键菜单使用 Windows 原生样式，跟随系统深浅色。
- 托盘与按需启动的工作进程分离；关闭引擎和所有窗口后仅保留托盘进程。

## 数据与升级

配置和历史保存在 EXE 旁的 `data` 文件夹。升级 Rust 版时退出程序、替换 EXE，保留 `data` 即可。配置可能包含服务密钥，历史包含字幕内容，不应上传或公开分享。

本版不提供旧版配置导入，也不会自动扫描 EXE 旁的旧配置。首次从 C# 版切换时，请单独放置 Rust 版并在设置中配置服务。已有 Rust 数据可以继续使用，损坏数据仍提供恢复操作。

## 开发

Rust 工程位于 [rust-app](rust-app/)，固定 Rust 1.99.1 和 Windows x64 MSVC 目标。在已启用 MSVC / Windows SDK 的 PowerShell 中：

```powershell
cd rust-app
cargo build --release --locked
```

输出：`rust-app/target/x86_64-pc-windows-msvc/release/LiveCaptionsTray.exe`。完整命令与测试边界见[构建说明](docs/BUILDING.md)。

## 来源与许可

本仓库原有 C# 版本及历史提交保留在 Git 历史中。Rust 重构以 [7c0dfa3](https://github.com/M1styuki/LiveCaptions-TranslatorTray/tree/7c0dfa3e8b37db9c941d884f8de7642f915b9a0a) 为行为基线；上游项目为 [SakiRinn/LiveCaptions-Translator](https://github.com/SakiRinn/LiveCaptions-Translator)。

Rust 应用及组合程序采用 [GPL-3.0-only](LICENSE)，Slint 使用 GPLv3 许可分支。原版材料及第三方资源保留各自许可，详见 [NOTICE](NOTICE) 和 [licenses](licenses/)。
