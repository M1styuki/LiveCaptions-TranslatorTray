# LiveCaptions TranslatorTray

基于 Windows 实时字幕的轻量桌面实时翻译工具。Windows 11 原生 Mica 外观，单文件直接运行，无需安装 .NET 或其他运行时。

[下载最新版](https://github.com/M1styuki/LiveCaptions-TranslatorTray/releases/latest) · [使用说明](docs/USER-GUIDE.md) · [构建说明](docs/BUILDING.md) · [验证范围](docs/VALIDATION.md)

## 使用方式

1. 先在 Windows 中完成「实时字幕」的首次设置并下载所需语音识别语言（识别语言在 Windows 实时字幕中选择）。
2. 下载并运行 `LiveCaptionsTray.exe`，程序常驻系统托盘。
3. 右键点击托盘图标即可打开设置页；首次启动会自动生成默认配置并直接进入设置。
4. Google 翻译可直接使用；DeepSeek 等 OpenAI 兼容服务可在对应接口页面填写地址、模型名称和密钥。
5. 左键点击托盘图标开启或关闭字幕翻译；需要完全退出时在右键菜单选择「退出」。

## 主要功能

- **多翻译引擎支持**：内置 Google、Google2、OpenAI 兼容接口、OpenRouter、Ollama、LMStudio、DeepL、有道、百度、MTranServer、LibreTranslate。
- **独立接口与上下文控制**：每个翻译服务均可保存独立配置，支持上下文翻译、调用间隔与显示句数调节。
- **可置顶字幕浮窗**：自由调整字体、描边、背景色与透明度，适应视频或会议叠加场景。
- **历史记录管理**：支持字幕与译文检索、分页浏览以及全量 CSV 导出。
- **原生界面体验**：Windows 11 Fluent 2 分组设置与原生样式托盘菜单，自动跟随系统浅/深色切换；关闭窗口与引擎后仅保留极轻量的托盘进程。

## 数据保存与升级

配置和翻译历史统一保存在 `LiveCaptionsTray.exe` 同级的 `data` 文件夹中。升级时只需退出程序并替换 `LiveCaptionsTray.exe`，保留 `data` 文件夹即可完整延续配置与历史记录（配置文件可能含接口密钥，历史记录包含字幕文本，请勿公开分享）。

## 从源码构建

主工程位于 [rust-app](rust-app/)，固定使用 Rust 1.99.1 与 `x86_64-pc-windows-msvc` 目标。在已配置 MSVC 与 Windows SDK 的 PowerShell 中执行：

```powershell
cd rust-app
cargo build --release --locked
```

构建产物位于 `rust-app/target/x86_64-pc-windows-msvc/release/LiveCaptionsTray.exe`。完整测试与构建细则请参见[构建说明](docs/BUILDING.md)。

## 来源与许可

本项目行为基线参考 [SakiRinn/LiveCaptions-Translator](https://github.com/SakiRinn/LiveCaptions-Translator)。本体代码及组合程序采用 [GPL-3.0-only](LICENSE) 协议，相关原始材料与第三方组件许可请见 [NOTICE](NOTICE) 与 [licenses](licenses/)。
