# -*- coding: utf-8 -*-
"""PyInstaller hook:把 curl_cffi 的 libcurl-impersonate 动态库打进 bundle。

为什么需要
----------
curl_cffi 的浏览器 TLS 指纹伪装(JA3/JA4 + HTTP/2)依赖 libcurl-impersonate
原生库,wheel 把它放在 curl_cffi/lib/ 下。PyInstaller 静态分析抓不到运行期
dlopen 的动态库,默认不会打包。漏打后 frozen exe 一发请求就 ImportError 或
SSL 连接失败(见 curl_cffi issue #5、#455),且这种失败是静默降级到 httpx ——
TLS 指纹又变回爬虫特征,Pixiv 风控重新触发。

此 hook 同时收集:
- 动态库(.dll/.dylib/.so):collect_dynamic_libs 自动找 curl_cffi/lib/ 下的
  libcurl-impersonate 及其依赖,放进 bundle 根目录(PyInstaller 运行期会把
  bundle 根目录加进 dlopen 搜索路径)。
- 数据文件:collect_data_files 兜底收集 .dat 等(如 curl 编译期的 ca-bundle)。

验证方式:打包后启动 frozen exe,访问 /api/auth/diag-version 确认 http_factory
报告 backend="curl_cffi"(而非降级到 httpx)。
"""

from PyInstaller.utils.hooks import collect_dynamic_libs, collect_data_files

binaries = collect_dynamic_libs("curl_cffi")
datas = collect_data_files("curl_cffi", include_py_files=False)
