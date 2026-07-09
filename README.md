# 机器码获取工具 Native 版

这是一个独立工程，不依赖旧版 Tauri/WebView2 代码。

目标：

- Windows 原生 Win32 窗口，不依赖 WebView2、Edge、Chromium。
- Portable ZIP 发布，解压即用。
- 内置 `127.0.0.1` API，兼容网页调用机器码。
- 硬件信息多通道采集，失败时保留原因和来源。
- 运行日志写入 `%APPDATA%\machine-code-native\startup.log`。

界面元素与旧版机器码工具保持一致：

- 机器码信息
- 开启授权 / 取消授权
- 网卡MAC地址
- 主板序列号
- CPU序列号
- 硬盘序列号
- 软件信息 / 版本 / 检查更新
- 用户协议 / 隐私策略

开发命令：

```powershell
cargo check
cargo run
cargo build --release
```

打包：

```powershell
.\scripts\package.ps1 -Target x86_64-pc-windows-msvc
.\scripts\package.ps1 -Target i686-pc-windows-msvc
```

Gitee 托管：

```powershell
git remote set-url origin https://gitee.com/zhangxinak/machine-code-native.git
git push -u origin main
```

注意：

- `.github/workflows/build.yml` 仅在 GitHub Actions 中生效。
- 放到 Gitee 后，源码托管不受影响；构建打包先使用本地 `scripts/package.ps1`。
- 如需 Gitee 自动构建，可以后续接入 Gitee Go 或企业流水线。
