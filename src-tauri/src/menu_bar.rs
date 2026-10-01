//! Windows 菜单栏「默认隐藏，Alt 唤出」。
//!
//! 应用菜单由 `lib.rs::setup_app_menu` 通过 `app.set_menu()` 下发，Windows 上
//! 等价于 `SetMenu(hwnd, hmenu)`，菜单栏因此常驻窗口顶部一行。该行在 macOS
//! 渲染于系统顶栏（不受影响），在 Windows 上只承载「退出」与 Edit 两项——
//! 前者与 Alt+F4／标题栏关闭等价，后者（撤销/剪切/复制/粘贴/全选）WebView2
//! 原生支持——纯属视觉噪音。
//!
//! 行为：装配时隐藏菜单栏；按 Alt 松开唤起并进入菜单循环；系统退出菜单循环
//! （选中菜单项、Esc 取消、窗口失活）后自动收回。
//!
//! 触发为何不用更朴素的做法：
//! - **前端按键事件**：Windows 上 Chromium 把单独按下的 Alt 交给
//!   `DefWindowProc`（供系统菜单使用），不向页面派发 DOM 键盘事件。
//! - **线程级键盘钩子（`WH_KEYBOARD`）**：键盘消息落在 WebView2 自己的
//!   窗口线程，装配线程的钩子根本看不到（实测回调不触发）。
//!
//! 因此改用 `ICoreWebView2Controller::AcceleratorKeyPressed`——它位于
//! WebView2 的输入管线内，是与宿主无关的唯一稳定拦截点。
//!
//! 与 muda（Tauri 的菜单后端）共存：muda 在同一 HWND 上装了子类化过程
//! （`MENU_SUBCLASS_ID = 200`）。子类化链按「后装先调用」执行，本模块用独立
//! ID 且未消费的消息一律交回 `DefSubclassProc`，muda 的 WM_COMMAND 分发与
//! 深色菜单绘制不受影响。

use std::sync::atomic::{AtomicIsize, Ordering};

use tauri::{Manager, WebviewWindow};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_KEY_EVENT_KIND, COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_UP,
};
use webview2_com::AcceleratorKeyPressedEventHandler;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    DrawMenuBar, GetMenu, HMENU, PostMessageW, SC_KEYMENU, SetMenu, WM_EXITMENULOOP, WM_SYSCOMMAND,
};

/// 子类化过程标识；muda 占用 200 与 202，这里取 ASCII "PTMB" 避免相撞。
const SUBCLASS_ID: usize = 0x5054_4D42;
/// `VK_MENU`（Alt）。为少启用一个 win32 feature 直接取字面值。
const VK_MENU: u32 = 0x12;

/// 顶层窗口句柄。
static TOP_HWND: AtomicIsize = AtomicIsize::new(0);
/// 菜单句柄：隐藏后 `GetMenu` 返回空，唤出时需要它重新装回。
static MENU_HANDLE: AtomicIsize = AtomicIsize::new(0);

/// 隐藏菜单栏，并装配「Alt 唤出 / 退出菜单循环即收回」。
///
/// 装配期调用一次。
pub fn install(window: &WebviewWindow) -> tauri::Result<()> {
    // `app.set_menu` 把菜单经主线程队列异步装到窗口上，本装配必须排在它之后：
    // 否则 `GetMenu` 拿到空句柄，之后无从把菜单重新装回。
    let app = window.app_handle().clone();
    let window = window.clone();
    app.run_on_main_thread(move || {
        if let Err(e) = install_now(&window) {
            log::warn!("菜单栏装配失败（将保持常驻）: {e}");
        }
    })
}

/// 主线程执行的实际装配（此时菜单已装到窗口上）。
fn install_now(window: &WebviewWindow) -> tauri::Result<()> {
    let hwnd = HWND(window.hwnd()?.0);
    // SAFETY: HWND 来自存活窗口。
    let menu = unsafe { GetMenu(hwnd) };
    if menu.0.is_null() {
        return Err(anyhow::anyhow!("窗口菜单尚未初始化").into());
    }
    TOP_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
    MENU_HANDLE.store(menu.0 as isize, Ordering::SeqCst);

    // 隐藏同样走主线程队列（排在本次之后），顺序得到保证。
    window.hide_menu()?;

    // SAFETY: hwnd 来自存活窗口；subclass_proc 是 'static 函数，随进程存活，
    // 无需卸载。
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0);
    }

    // Alt 松开 → 唤出。注册成功后由 WebView2 持有处理器引用，闭包随之存活。
    window.with_webview(|pw| {
        let controller = pw.controller();
        let handler = AcceleratorKeyPressedEventHandler::create(Box::new(move |_sender, args| {
            if let Some(args) = args {
                let mut vk = 0u32;
                let mut kind = COREWEBVIEW2_KEY_EVENT_KIND(0);
                unsafe {
                    args.VirtualKey(&mut vk)?;
                    args.KeyEventKind(&mut kind)?;
                }
                if vk == VK_MENU && kind == COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_UP {
                    show();
                }
            }
            Ok(())
        }));
        let mut token = 0i64;
        if let Err(e) = unsafe { controller.add_AcceleratorKeyPressed(&handler, &mut token) } {
            log::warn!("WebView2 加速键事件注册失败（Alt 唤出菜单栏不可用）: {e}");
        }
    })
}

/// 装回菜单栏，并请求系统进入菜单循环（等价于用户按下 Alt 的系统行为）。
fn show() {
    let top = TOP_HWND.load(Ordering::SeqCst);
    let menu = MENU_HANDLE.load(Ordering::SeqCst);
    if top == 0 || menu == 0 {
        return;
    }
    // SAFETY: 两个句柄都取自本窗口；先装菜单再请求激活，同一线程队列保证顺序。
    unsafe {
        let hwnd = HWND(top as _);
        let _ = SetMenu(hwnd, Some(HMENU(menu as _)));
        let _ = DrawMenuBar(hwnd);
        let _ = PostMessageW(Some(hwnd), WM_SYSCOMMAND, WPARAM(SC_KEYMENU as usize), LPARAM(0));
    }
}

/// 顶层窗口过程：退出菜单循环后收回菜单栏。
unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    uidsubclass: usize,
    _dwrefdata: usize,
) -> LRESULT {
    if msg == WM_EXITMENULOOP && uidsubclass == SUBCLASS_ID {
        // 选中菜单项 / Esc 取消 / 窗口失活，等价于 `Window::hide_menu()`。
        // SAFETY: 只作用于本窗口。
        unsafe {
            let _ = SetMenu(hwnd, None);
            let _ = DrawMenuBar(hwnd);
        }
    }
    // SAFETY: 交回子类化链下一层（muda 的过程）与原窗口过程。
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
