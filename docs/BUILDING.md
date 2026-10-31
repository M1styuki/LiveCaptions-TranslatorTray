# 构建与测试

## 环境

- Windows 11 x64（程序运行目标）。
- Rust 1.99.1，由 rust-app/rust-toolchain.toml 固定。
- Visual Studio 2022 Build Tools 的 MSVC x64 C++ 工具及 Windows SDK。

正常 Cargo 构建会按 Cargo.lock 获取依赖。不需要本项目作者的开发目录、验收记录或 .NET 环境。

## 构建

在仓库根目录的 PowerShell 执行：

```powershell
.ust-app	oolsuild.ps1 -Mode release
```

脚本使用现有 Rust，并在需要时通过 Visual Studio 自带的开发环境入口启用 MSVC / SDK；不安装软件，也不更改用户或系统环境变量。

也可在 Developer PowerShell 中直接执行：

```powershell
cd rust-app
cargo build --release --locked
```

成品位于 rust-app/target/x86_64-pc-windows-msvc/release/LiveCaptionsTray.exe。只需分发这个 EXE；运行数据在程序旁自动创建。EXE 已嵌入项目和第三方许可文本，不需要附带编译工具、PDB 或 Cargo 缓存。

## 检查

在 rust-app 目录：

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=1
```

测试会创建项目内的 scratch/ 和 evidence/ 夹具目录，不处理用户实际配置。需要真实外部服务、系统字幕或可见原生对话框的诊断标为 ignored，不随普通测试运行。不要将普通测试通过视为这些诊断通过。

GitHub Actions 在 Windows runner 上检查格式、构建发行版并运行全工作区发行模式测试；全部通过后提供 EXE 构建产物。本机无需安装编译环境。测试包括 CC/托盘启停状态和隔离注册表中的自启动读写，不会注册真实登录任务。网络服务的真实翻译质量、可见界面和实际登录自启动仍需要单独验证。
