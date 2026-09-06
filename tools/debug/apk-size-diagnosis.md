# 体积差异诊断

## 数字

| 制品 | 大小 | 说明 |
| --- | ---: | --- |
| `Release/Android/app-arm64-debug.apk` | 164.66 MB | Tauri **debug** APK（`scripts/build-android.ps1` 默认 `--debug`） |
| `Release/Android/app-universal-debug.apk` | 326.45 MB | universal ABI（含全部 4 个 .so） |
| `Release/Windows/RemoteEnvCollector.exe` | 12.19 MB | Tauri **release** 单文件 |
| `Release/Windows/RemoteEnvCollector-Setup.exe` | 3.06 MB | NSIS 安装包 |

## APK 里到底装了什么

`app-arm64-debug.apk` 里就一个 `lib/arm64-v8a/libremote_env_desktop_lib.so`，**157.73 MB 未压缩**（APK 内 165,388,912 B），剩余部分是 classes*.dex + 资源，可忽略。

## `.so` 内部 ELF 段拆分（readelf -S）

| 段 | 字节 | MB |
| --- | ---: | ---: |
| `.debug_info` | 0x386f6bf = 59,236,543 | **56.49** |
| `.debug_str`  | 0x2f19fd9 = 49,404,377 | **47.11** |
| `.debug_ranges` | 0x750340 = 7,683,904 | 7.33 |
| `.debug_abbrev` | 0xa47e6 = 673,254 | 0.64 |
| `.debug_loc`   | 0x1901c8 = 1,638,856 | 1.56 |
| `.debug_line`  | 0xc594ae = 12,948,398 | 12.35 |
| `.debug_aranges` | 0x1a73b0 = 1,737,648 | 1.66 |
| `.text`        | 0xb804d4 = 12,054,228 | 11.50 |
| `.rodata`      | 0x1176f4 = 1,143,028 | 1.09 |
| `.eh_frame` + `.eh_frame_hdr` | — | ~3.7 |
| `.data.rel.ro` | — | ~0.62 |
| `.gcc_except_table` | — | ~0.84 |
| `.symtab` + `.strtab` | 0x93fa9f + 0x4433b8 | **131.7 MB 调试符号表** |

合计 `.debug_*` ≈ **127.91 MB**（占 .so 的 81%）；`.symtab`+`.strtab` 也大约 11 MB。剥离这两块后：

| 动作 | 大小 | 节省 |
| --- | ---: | ---: |
| 原 `libremote_env_desktop_lib.so` | 157.73 MB | — |
| `llvm-strip --strip-debug` | 29.82 MB | -127.91 MB |
| 再 `--strip-unneeded` | 17.16 MB | -12.66 MB |

## 结论

体积差异 99% 不在 Android 端 ARM 编译产物，而在 **Rust debug 构建未剥离调试信息**：

1. `scripts/build-android.ps1` 默认走 `--debug`，而 Tauri Android 插件会保留 DWARF（.debug_info / .line / .ranges / .str / .aranges 等）以及全量 `.symtab`/`.strtab`。
2. 而 `scripts/build-windows.ps1` 走 `tauri build --bundles nsis`，是 release profile，本身就 strip + LTO，最终 EXE 12 MB。
3. 你看到 Windows 端 11 MB vs Android 端 164 MB，差异本质上等于 130 MB DWARF。

## 立即验证

如果是 debug 设备调试包，这是预期；如果要发布版本，跑：

```powershell
pwsh -NoProfile -File scripts\build-android.ps1 -ReleaseApk
```

预期 release APK 在 **20–30 MB** 区间（单 ABI），跟 Windows 同量级。

## 进一步收紧体积

Tauri 2 官方建议：
1. `Cargo.toml` workspace 配 `[profile.release] strip = "symbols" panic = "abort" lto = "thin" codegen-units = 1`。
2. `tauri.conf.json` 里只打包一个 ABI（`productName` 同级 `bundle.android.targets: ["apk"]` 不够，还要在 Gradle `splits.abi` 配置，只保留 `arm64-v8a` + `universal: false`）。
3. 如果只是真机调试，可直接给 Gradle 传 `-PkeepDebugSymbols=false`，避免 `jniLibs.keepDebugSymbols` 把 DWARF 灌进 APK。