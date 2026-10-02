# BoringSSL 在 Windows ARM64 上的 CMake 工具链文件（经 btls-sys 的入口注入）。
#
# 为什么需要它：
# BoringSSL 的 win-aarch64 汇编（gen/bcm/*-armv8-win.S）是 GNU 汇编器语法，上游
# 明确要求用 Clang 汇编（其 CMakeLists 对非 Clang 的 ASM 编译器会加 `-Wa,-g`）。
# 而在原生 arm64 构建下，CMake 的 ASM 语言会选到 MSVC 自带的 armasm64，语法不兼容，
# 汇编阶段必失败。btls-sys 自己只在「Windows 交叉编译」时刻意关闭汇编
# （build/main.rs：`OPENSSL_NO_ASM=YES`，注释写明 BoringSSL 的 CMakeLists 未适配
# Visual Studio 交叉编译），原生同架构构建不会关，所以这里补上。
#
# 代价：TLS 走 C 实现，失去 ARMv8 汇编加速。本应用以网络 I/O 为主（pixiv 接口 +
# 图片下载），握手吞吐不是瓶颈，故取稳定性。若将来确实需要汇编，把下面的
# OPENSSL_NO_ASM 换成指定 Clang 作 ASM 编译器（镜像带 LLVM）即可：
#   set(CMAKE_ASM_COMPILER clang CACHE FILEPATH "" FORCE)
#
# 注入方式（btls-sys 与 cmake-rs 共用这套环境变量命名，见 build/config.rs 的
# target_only_var 与 cmake-rs 的 getenv_target_os）：
#   CMAKE_TOOLCHAIN_FILE_aarch64_pc_windows_msvc=<本文件绝对路径>
# 设置了它以后 btls-sys 会跳过自己的一批 define（含 MSVC 运行时），故下面补齐。

# 用 CACHE 变量而非普通变量：BoringSSL 若以 option() 声明同名项，普通变量在旧
# CMP0077 行为下会被 cache 覆盖，CACHE + FORCE 才稳。
set(OPENSSL_NO_ASM YES CACHE BOOL "关闭 BoringSSL 汇编（win-arm64 走 C 实现）" FORCE)

# 等价于 btls-sys 在非 crt-static 情况下的设定，保持与本仓其它平台一致
set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreadedDLL" CACHE STRING "MSVC 运行时库" FORCE)
