//! Windows 菜单栏「默认隐藏，Alt 唤出」。
//!
//! 应用菜单由 `lib.rs::setup_app_menu` 通过 `app.set_menu()` 下发，Windows 上
//! 等价于 `SetMenu(hwnd, hmenu)`，菜单栏因此常驻窗口顶部一行。该行在 macOS
//! 渲染于系统顶栏（不受影响），在 Windows 上只承载「退出」与 Edit 两项——
//! 前者与 Alt+F4／标题栏关闭等价，后者（撤销/剪切/复制/粘贴/全选）WebView2
//! 原生支持——纯属视觉噪音。
//!
//! 行为：装配时立即隐藏菜单栏；前端按 Alt 松开调用 `app_menu_show`
//! （`commands/app_cmds.rs`）显示并进入菜单循环；系统退出菜单循环（选中菜单项、
//! Esc 取消、窗口失活）后由本模块的子类化过程重新隐藏。
//!
//! 与 muda（Tauri 的菜单后端）共存：muda 在同一 HWND 上装了子类化过程
//! （`MENU_SUBCLASS_ID = 200`）。子类化链按「后装先调用」执行，本模块用独立
//! ID 且未消费的消息一律交回 `DefSubclassProc`，muda 的 WM_COMMAND 分发与
//! 深色菜单绘制不受影响。

use tauri::{Manager, WebviewWindow};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    DrawMenuBar, PostMessageW, SC_KEYMENU, SetMenu, WM_EXITMENULOOP, WM_SYSCOMMAND,
};

/// 子类化过程标识；muda 占用 200 与 202，这里取 ASCII "PTMB" 避免相撞。
const SUBCLASS_ID: usize = 0x5054_4D42;

/// 隐藏菜单栏，并挂上「退出菜单循环即重新隐藏」的子类化过程。
///
/// 装配期调用一次。`SetWindowSubclass` 以 ID 去重，重复调用不会叠加过程。
pub fn install(window: &WebviewWindow) -> tauri::Result<()> {
    window.hide_menu()?;
    let raw = window.hwnd()?;
    // SAFETY: HWND 来自存活窗口；subclass_proc 是 'static 函数，执行期间窗口
    // 由 Tauri 持有，直到进程退出都不会销毁。
    unsafe {
        let _ = SetWindowSubclass(HWND(raw.0), Some(subclass_proc), SUBCLASS_ID, 0);
    }
    Ok(())
}

/// 显示菜单栏并进入菜单循环（首项高亮，方向键/回车可操作）。
///
/// 前端 Alt 松开时经 `app_menu_show` 调用。
pub fn show(window: &WebviewWindow) -> tauri::Result<()> {
    // 两个调用都投递到主线程事件队列，FIFO 保证 SC_KEYMENU 一定在
    // SetMenu 之后到达，否则 DefWindowProc 会因窗口尚无菜单而空转。
    window.show_menu()?;
    let raw = window.hwnd()?.0 as isize;
    window.app_handle().run_on_main_thread(move || {
        // SAFETY: hwnd 来自存活窗口；此时菜单已装回窗口，DefWindowProc 收到
        // SC_KEYMENU 会激活菜单栏（等价于用户按下 Alt 的系统行为）。
        unsafe {
            let _ = PostMessageW(
                Some(HWND(raw as _)),
                WM_SYSCOMMAND,
                WPARAM(SC_KEYMENU as usize),
                LPARAM(0),
            );
        }
    })?;
    Ok(())
}

/// 退出菜单循环即隐藏菜单栏：覆盖「选中菜单项 / Esc 取消 / 窗口失活」。
unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    uidsubclass: usize,
    _dwrefdata: usize,
) -> LRESULT {
    if msg == WM_EXITMENULOOP && uidsubclass == SUBCLASS_ID {
        // 等价于 `Window::hide_menu()`：子类化过程内拿不到窗口对象，直接操作 HWND。
        // SAFETY: hwnd 由系统传入，两个调用只作用于该窗口。
        unsafe {
            let _ = SetMenu(hwnd, None);
            let _ = DrawMenuBar(hwnd);
        }
    }
    // SAFETY: 交回子类化链下一层（muda 的过程）与原窗口过程。
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
