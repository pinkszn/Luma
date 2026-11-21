#![allow(non_snake_case, non_camel_case_types, dead_code)]
//! Minimal, dependency-free Win32 bindings used by Luma.

use std::ffi::c_void;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

pub type HWND = *mut c_void;
pub type HINSTANCE = *mut c_void;
pub type HICON = *mut c_void;
pub type HCURSOR = *mut c_void;
pub type HMENU = *mut c_void;
pub type HBRUSH = *mut c_void;
pub type HPEN = *mut c_void;
pub type HFONT = *mut c_void;
pub type HDC = *mut c_void;
pub type HMONITOR = *mut c_void;
pub type HANDLE = *mut c_void;
pub type HKEY = *mut c_void;
pub type BOOL = i32;
pub type UINT = u32;
pub type WPARAM = usize;
pub type LPARAM = isize;
pub type LRESULT = isize;
pub type ATOM = u16;
pub type DWORD = u32;

pub unsafe fn wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

pub unsafe fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

#[repr(C)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
#[repr(C)]
pub struct PAINTSTRUCT {
    pub hdc: HDC,
    pub fErase: BOOL,
    pub rcPaint: RECT,
    pub fRestore: BOOL,
    pub fIncUpdate: BOOL,
    pub rgbReserved: [u8; 32],
}
#[repr(C)]
pub struct DRAWITEMSTRUCT {
    pub CtlType: u32,
    pub CtlID: u32,
    pub itemID: u32,
    pub itemAction: u32,
    pub itemState: u32,
    pub hwndItem: HWND,
    pub hDC: HDC,
    pub rcItem: RECT,
    pub itemData: usize,
}
#[repr(C)]
pub struct CHARFORMATW {
    pub cbSize: u32,
    pub dwMask: u32,
    pub dwEffects: u32,
    pub yHeight: i32,
    pub yOffset: i32,
    pub crTextColor: u32,
    pub bCharSet: u8,
    pub bPitchAndFamily: u8,
    pub szFaceName: [u16; 32],
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SYSTEMTIME {
    pub wYear: u16,
    pub wMonth: u16,
    pub wDayOfWeek: u16,
    pub wDay: u16,
    pub wHour: u16,
    pub wMinute: u16,
    pub wSecond: u16,
    pub wMilliseconds: u16,
}
#[repr(C)]
pub struct MONITORINFOEXW {
    pub cbSize: u32,
    pub rcMonitor: RECT,
    pub rcWork: RECT,
    pub dwFlags: u32,
    pub szDevice: [u16; 32],
}
#[repr(C)]
pub struct DISPLAY_DEVICEW {
    pub cb: u32,
    pub DeviceName: [u16; 32],
    pub DeviceString: [u16; 128],
    pub StateFlags: u32,
    pub DeviceID: [u16; 128],
    pub DeviceKey: [u16; 128],
}
pub const DISPLAY_DEVICE_ACTIVE: u32 = 0x1;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PHYSICAL_MONITOR {
    pub hPhysicalMonitor: HANDLE,
    pub szPhysicalMonitorDescription: [u16; 128],
}

#[repr(C)]
pub struct WNDCLASSEXW {
    pub cbSize: u32,
    pub style: u32,
    pub lpfnWndProc: Option<unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT>,
    pub cbClsExtra: i32,
    pub cbWndExtra: i32,
    pub hInstance: HINSTANCE,
    pub hIcon: HICON,
    pub hCursor: HCURSOR,
    pub hbrBackground: HBRUSH,
    pub lpszMenuName: *const u16,
    pub lpszClassName: *const u16,
    pub hIconSm: HICON,
}
#[repr(C)]
pub struct CREATESTRUCTW {
    pub lpCreateParams: *mut c_void,
    pub hInstance: HINSTANCE,
    pub hMenu: HMENU,
    pub hwndParent: HWND,
    pub cy: i32,
    pub cx: i32,
    pub y: i32,
    pub x: i32,
    pub style: i32,
    pub lpszName: *const u16,
    pub lpszClass: *const u16,
    pub dwExStyle: u32,
}
#[repr(C)]
pub struct MSG {
    pub hwnd: HWND,
    pub message: u32,
    pub wParam: WPARAM,
    pub lParam: LPARAM,
    pub time: u32,
    pub pt: POINT,
    pub lPrivate: u32,
}
#[repr(C)]
pub struct INITCOMMONCONTROLSEX {
    pub dwSize: u32,
    pub dwICC: u32,
}
#[repr(C)]
pub struct NMHDR {
    pub hwndFrom: HWND,
    pub idFrom: usize,
    pub code: u32,
}
#[repr(C)]
pub struct GUID {
    pub Data1: u32,
    pub Data2: u16,
    pub Data3: u16,
    pub Data4: [u8; 8],
}
#[repr(C)]
pub struct NOTIFYICONDATAW {
    pub cbSize: u32,
    pub hWnd: HWND,
    pub uID: u32,
    pub uFlags: u32,
    pub uCallbackMessage: u32,
    pub hIcon: HICON,
    pub szTip: [u16; 128],
    pub dwState: u32,
    pub dwStateMask: u32,
    pub szInfo: [u16; 256],
    pub uVersion: u32,
    pub szInfoTitle: [u16; 64],
    pub dwInfoFlags: u32,
    pub guidItem: GUID,
    pub hBalloonIcon: HICON,
}

pub const CW_USEDEFAULT: i32 = 0x8000_0000u32 as i32;
pub const WS_OVERLAPPEDWINDOW: u32 = 0x00CF_0000;
pub const WS_CAPTION: u32 = 0x00C0_0000;
pub const WS_SYSMENU: u32 = 0x0008_0000;
pub const WS_MINIMIZEBOX: u32 = 0x0002_0000;
pub const WS_CHILD: u32 = 0x4000_0000;
pub const WS_CLIPSIBLINGS: u32 = 0x0400_0000;
pub const WS_VISIBLE: u32 = 0x1000_0000;
pub const WS_TABSTOP: u32 = 0x0001_0000;
pub const WS_VSCROLL: u32 = 0x0020_0000;
pub const WS_BORDER: u32 = 0x0080_0000;
pub const WS_EX_CLIENTEDGE: u32 = 0x0000_0200;

pub const SW_HIDE: i32 = 0;
pub const SW_SHOW: i32 = 5;
pub const SW_MINIMIZE: i32 = 6;
pub const SW_RESTORE: i32 = 9;

pub const BS_AUTOCHECKBOX: u32 = 0x3;
pub const BS_GROUPBOX: u32 = 0x7;
pub const BS_PUSHBUTTON: u32 = 0x0;
pub const BS_OWNERDRAW: u32 = 0xB;
pub const CBS_DROPDOWNLIST: u32 = 0x3;
pub const CB_GETLBTEXT: u32 = 0x0148;
pub const CBS_OWNERDRAWFIXED: u32 = 0x0010;
pub const CBS_HASSTRINGS: u32 = 0x0200;
pub const ODT_COMBOBOX: u32 = 3;
pub const ODS_COMBOBOXEDIT: u32 = 0x1000;
pub const ES_AUTOHSCROLL: u32 = 0x80;
pub const LBS_NOTIFY: u32 = 0x1;
pub const LBS_NOINTEGRALHEIGHT: u32 = 0x100;
pub const SS_LEFT: u32 = 0x0;
pub const SS_RIGHT: u32 = 0x2;
pub const SS_NOPREFIX: u32 = 0x80;
pub const SS_OWNERDRAW: u32 = 0xD;
pub const TCS_HOTTRACK: u32 = 0x40;
pub const TBS_AUTOTICKS: u32 = 0x1;

pub const ODT_BUTTON: u32 = 4;
pub const ODS_SELECTED: u32 = 1;
pub const ODS_FOCUS: u32 = 0x10;
pub const ODS_DISABLED: u32 = 4;

pub const DT_LEFT: u32 = 0x0000;
pub const DT_CENTER: u32 = 0x0001;
pub const DT_RIGHT: u32 = 0x0002;
pub const DT_VCENTER: u32 = 0x0004;
pub const DT_SINGLELINE: u32 = 0x0020;
pub const DT_WORDBREAK: u32 = 0x0010;
pub const DT_NOPREFIX: u32 = 0x0800;
pub const DT_END_ELLIPSIS: u32 = 0x8000;

pub const PS_SOLID: i32 = 0;
pub const PS_DOT: i32 = 2;
pub const TRANSPARENT: i32 = 1;
pub const NULL_BRUSH: i32 = 5;
pub const NULL_PEN: i32 = 8;
pub const GWLP_USERDATA: i32 = -21;
pub const GWLP_WNDPROC: i32 = -4;
pub const WHITE_BRUSH: i32 = 0;

pub const WM_NCCREATE: u32 = 0x0081;
pub const WM_CREATE: u32 = 0x0001;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_SIZE: u32 = 0x0005;
pub const WM_PAINT: u32 = 0x000F;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const RDW_INVALIDATE: u32 = 0x0001;
pub const RDW_ERASE: u32 = 0x0004;
pub const RDW_ALLCHILDREN: u32 = 0x0080;
pub const WM_GETMINMAXINFO: u32 = 0x0024;
pub const WM_DRAWITEM: u32 = 0x002B;
pub const WM_NOTIFY: u32 = 0x004E;
pub const WM_SETICON: u32 = 0x0080;
pub const WM_COMMAND: u32 = 0x0111;
pub const WM_TIMER: u32 = 0x0113;
pub const WM_HSCROLL: u32 = 0x0114;
pub const WM_CTLCOLORBTN: u32 = 0x0135;
pub const WM_CTLCOLOREDIT: u32 = 0x0133;
pub const WM_CTLCOLORLISTBOX: u32 = 0x0134;
pub const EM_SETBKGNDCOLOR: u32 = 0x0443;
pub const EM_SETCHARFORMAT: u32 = 0x0444;
pub const SCF_DEFAULT: usize = 0x0001;
pub const SCF_ALL: usize = 0x0004;
pub const CFM_COLOR: u32 = 0x4000_0000;
pub const WM_CTLCOLORSTATIC: u32 = 0x0138;
pub const WM_POWERBROADCAST: u32 = 0x0218;
pub const WM_DISPLAYCHANGE: u32 = 0x007E;
pub const WM_HOTKEY: u32 = 0x0312;
pub const WM_APP: u32 = 0x8000;
pub const WM_SETTEXT: u32 = 0x000C;
pub const WM_GETTEXT: u32 = 0x000D;
pub const WM_SETFONT: u32 = 0x0030;

pub const COLOR_BTNFACE: i32 = 15;
pub const IDC_ARROW: *const u16 = 32512 as *const u16;

pub const BN_CLICKED: u16 = 0;
pub const LBN_SELCHANGE: u16 = 1;
pub const CBN_SELCHANGE: u16 = 1;
pub const EN_KILLFOCUS: u16 = 0x0200;

pub const TBM_GETPOS: u32 = 0x0400;
pub const TBM_SETPOS: u32 = 0x0405;
pub const TBM_SETRANGE: u32 = 0x0406;
pub const TBM_SETRANGEMIN: u32 = 0x0407;
pub const TBM_SETRANGEMAX: u32 = 0x0408;
pub const TBM_SETTICFREQ: u32 = 0x0414;
pub const TBM_SETPAGESIZE: u32 = 0x0413;
pub const TBM_GETRANGEMAX: u32 = 0x0402;
pub const TBM_GETRANGEMIN: u32 = 0x0401;

pub const TB_LINEUP: u32 = 0;
pub const TB_LINEDOWN: u32 = 1;
pub const TB_PAGEUP: u32 = 2;
pub const TB_PAGEDOWN: u32 = 3;
pub const TB_THUMBPOSITION: u32 = 4;
pub const TB_THUMBTRACK: u32 = 5;
pub const TB_TOP: u32 = 6;
pub const TB_BOTTOM: u32 = 7;
pub const TB_ENDTRACK: u32 = 8;

pub const CB_ADDSTRING: u32 = 0x0143;
pub const CB_GETCURSEL: u32 = 0x0147;
pub const CB_SETCURSEL: u32 = 0x014E;
pub const CB_RESETCONTENT: u32 = 0x014B;
pub const CB_SETDROPPEDWIDTH: u32 = 0x0160;

pub const LB_ADDSTRING: u32 = 0x0180;
pub const LB_RESETCONTENT: u32 = 0x0184;
pub const LB_SETCURSEL: u32 = 0x0186;
pub const LB_GETCURSEL: u32 = 0x0188;

pub const BM_SETCHECK: u32 = 0x00F1;
pub const BM_GETCHECK: u32 = 0x00F0;
pub const BST_CHECKED: usize = 1;

pub const TCM_FIRST: u32 = 0x1300;
pub const TCM_GETCURSEL: u32 = TCM_FIRST + 4;
pub const TCM_SETCURSEL: u32 = TCM_FIRST + 12;
pub const TCN_FIRST: i32 = -550;
pub const TCN_SELCHANGE: i32 = TCN_FIRST - 1;

pub const PBT_APMRESUMESUSPEND: u32 = 0x0007;
pub const PBT_APMRESUMEAUTOMATIC: u32 = 0x0012;

pub const NIM_ADD: u32 = 0;
pub const NIM_MODIFY: u32 = 1;
pub const NIM_DELETE: u32 = 2;
pub const NIM_SETVERSION: u32 = 4;
pub const NIF_MESSAGE: u32 = 0x1;
pub const NIF_ICON: u32 = 0x2;
pub const NIF_TIP: u32 = 0x4;
pub const NIF_SHOWTIP: u32 = 0x80;
pub const NOTIFYICON_VERSION_4: u32 = 4;

pub const WM_TRAY: u32 = WM_APP + 1;
pub const WM_RBUTTONUP: u32 = 0x0205;
pub const WM_LBUTTONDBLCLK: u32 = 0x0203;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_GETDLGCODE: u32 = 0x0087;
pub const WM_NCDESTROY: u32 = 0x0082;
pub const WM_ENABLE: u32 = 0x000A;
pub const VK_LEFT: u32 = 0x25;
pub const VK_RIGHT: u32 = 0x27;
pub const VK_SHIFT: u32 = 0x10;
pub const VK_RETURN: u32 = 0x0D;
pub const DLGC_WANTARROWS: isize = 0x0001;

pub const MF_STRING: u32 = 0;
pub const MF_SEPARATOR: u32 = 0x800;
pub const MF_CHECKED: u32 = 0x8;
pub const TPM_LEFTALIGN: u32 = 0;
pub const TPM_RIGHTBUTTON: u32 = 0x2;
pub const TPM_BOTTOMALIGN: u32 = 0x20;
pub const TPM_RETURNCMD: u32 = 0x100;
pub const TPM_NONOTIFY: u32 = 0x80;

pub const MOD_ALT: u32 = 1;
pub const MOD_CONTROL: u32 = 2;
pub const VK_N: u32 = 0x4E;
pub const VK_UP: u32 = 0x26;
pub const VK_DOWN: u32 = 0x28;

pub const ERROR_ALREADY_EXISTS: u32 = 183;
pub const ERROR_ACCESS_DENIED: u32 = 5;
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub const KEY_SET_VALUE: u32 = 0x0002;
pub const REG_SZ: u32 = 1;
pub const HKEY_CURRENT_USER: HKEY = 0x8000_0001u32 as usize as HKEY;

pub const ICC_WIN95_CLASSES: u32 = 0xFF;

pub const IMAGE_ICON: u32 = 1;
pub const LR_DEFAULTCOLOR: u32 = 0;

#[link(name = "user32")]
extern "system" {
    pub fn RegisterClassExW(lpWndClass: *const WNDCLASSEXW) -> ATOM;
    pub fn CreateWindowExW(
        dwExStyle: u32,
        lpClassName: *const u16,
        lpWindowName: *const u16,
        dwStyle: u32,
        X: i32,
        Y: i32,
        nWidth: i32,
        nHeight: i32,
        hWndParent: HWND,
        hMenu: HMENU,
        hInstance: HINSTANCE,
        lpParam: *mut c_void,
    ) -> HWND;
    pub fn DefWindowProcW(hWnd: HWND, Msg: u32, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
    pub fn DestroyWindow(hWnd: HWND) -> BOOL;
    pub fn PostQuitMessage(nExitCode: i32);
    pub fn ShowWindow(hWnd: HWND, nCmdShow: i32) -> BOOL;
    pub fn UpdateWindow(hWnd: HWND) -> BOOL;
    pub fn RedrawWindow(
        hWnd: HWND,
        lprcUpdate: *const RECT,
        hrgnUpdate: HANDLE,
        flags: u32,
    ) -> BOOL;
    pub fn GetMessageW(
        lpMsg: *mut MSG,
        hWnd: HWND,
        wMsgFilterMin: u32,
        wMsgFilterMax: u32,
    ) -> BOOL;
    pub fn TranslateMessage(lpMsg: *const MSG) -> BOOL;
    pub fn DispatchMessageW(lpMsg: *const MSG) -> LRESULT;
    pub fn SetWindowLongPtrW(hWnd: HWND, nIndex: i32, dwNewLong: isize) -> isize;
    pub fn CallWindowProcW(
        lpPrevWndFunc: isize,
        hWnd: HWND,
        Msg: u32,
        wParam: WPARAM,
        lParam: LPARAM,
    ) -> LRESULT;
    pub fn GetWindowLongPtrW(hWnd: HWND, nIndex: i32) -> isize;
    pub fn SendMessageW(hWnd: HWND, Msg: u32, wParam: WPARAM, lParam: LPARAM) -> LRESULT;
    pub fn PostMessageW(hWnd: HWND, Msg: u32, wParam: WPARAM, lParam: LPARAM) -> BOOL;
    pub fn SetWindowTextW(hWnd: HWND, lpString: *const u16) -> BOOL;
    pub fn GetWindowTextW(hWnd: HWND, lpString: *mut u16, nMaxCount: i32) -> i32;
    pub fn GetWindowTextLengthW(hWnd: HWND) -> i32;
    pub fn EnableWindow(hWnd: HWND, bEnable: BOOL) -> BOOL;
    pub fn IsWindowEnabled(hWnd: HWND) -> BOOL;
    pub fn MoveWindow(
        hWnd: HWND,
        X: i32,
        Y: i32,
        nWidth: i32,
        nHeight: i32,
        bRepaint: BOOL,
    ) -> BOOL;
    pub fn GetClientRect(hWnd: HWND, lpRect: *mut RECT) -> BOOL;
    pub fn SetTimer(hWnd: HWND, nIDEvent: usize, uElapse: u32, lpTimerFunc: *const c_void) -> usize;
    pub fn KillTimer(hWnd: HWND, uIDEvent: usize) -> BOOL;
    pub fn MessageBoxW(
        hWnd: HWND,
        lpText: *const u16,
        lpCaption: *const u16,
        uType: u32,
    ) -> i32;
    pub fn CreatePopupMenu() -> HMENU;
    pub fn AppendMenuW(hMenu: HMENU, uFlags: u32, uIDNewItem: usize, lpNewItem: *const u16)
        -> BOOL;
    pub fn DestroyMenu(hMenu: HMENU) -> BOOL;
    pub fn TrackPopupMenu(
        hMenu: HMENU,
        uFlags: u32,
        x: i32,
        y: i32,
        nReserved: i32,
        hWnd: HWND,
        prcRect: *const RECT,
    ) -> BOOL;
    pub fn GetCursorPos(lpPoint: *mut POINT) -> BOOL;
    pub fn SetForegroundWindow(hWnd: HWND) -> BOOL;
    pub fn RegisterHotKey(hWnd: HWND, id: i32, fsModifiers: u32, vk: u32) -> BOOL;
    pub fn UnregisterHotKey(hWnd: HWND, id: i32) -> BOOL;
    pub fn GetLocalTime(lpSystemTime: *mut SYSTEMTIME);
    pub fn GetSystemTime(lpSystemTime: *mut SYSTEMTIME);
    pub fn EnumDisplayMonitors(
        hdc: HDC,
        lprcClip: *const RECT,
        lpfnEnum: unsafe extern "system" fn(HMONITOR, HDC, *mut RECT, LPARAM) -> BOOL,
        dwData: LPARAM,
    ) -> BOOL;
    pub fn GetMonitorInfoW(hMonitor: HMONITOR, lpmi: *mut MONITORINFOEXW) -> BOOL;
    pub fn EnumDisplayDevicesW(
        lpDevice: *const u16,
        iDevNum: u32,
        lpDisplayDevice: *mut DISPLAY_DEVICEW,
        dwFlags: u32,
    ) -> BOOL;
    pub fn LoadCursorW(hInstance: HINSTANCE, lpCursorName: *const u16) -> HCURSOR;
    pub fn LoadIconW(hInstance: HINSTANCE, lpIconName: *const u16) -> HICON;
    pub fn CreateIcon(
        hInstance: HINSTANCE,
        nWidth: i32,
        nHeight: i32,
        cPlanes: u8,
        cBitsPerPixel: u8,
        lpbANDbits: *const u8,
        lpbXORbits: *const u8,
    ) -> HICON;
    pub fn DestroyIcon(hIcon: HICON) -> BOOL;
    pub fn GetSystemMetrics(nIndex: i32) -> i32;
    pub fn AdjustWindowRectEx(
        lpRect: *mut RECT,
        dwStyle: u32,
        bMenu: BOOL,
        dwExStyle: u32,
    ) -> BOOL;
    pub fn FindWindowW(lpClassName: *const u16, lpWindowName: *const u16) -> HWND;
    pub fn GetDlgItem(hDlg: HWND, nIDDlgItem: i32) -> HWND;
    pub fn GetDlgCtrlID(hWnd: HWND) -> i32;
    pub fn GetClassNameW(hWnd: HWND, lpClassName: *mut u16, nMaxCount: i32) -> i32;
    pub fn SetFocus(hWnd: HWND) -> HWND;
    pub fn GetFocus() -> HWND;
    pub fn GetParent(hWnd: HWND) -> HWND;
    pub fn SetCapture(hWnd: HWND) -> HWND;
    pub fn ReleaseCapture() -> BOOL;
    pub fn GetCapture() -> HWND;
    pub fn GetKeyState(nVirtKey: i32) -> i16;
    pub fn SetCursor(hCursor: HCURSOR) -> HCURSOR;
    pub fn IsWindow(hWnd: HWND) -> BOOL;
    pub fn InvalidateRect(hWnd: HWND, lpRect: *const RECT, bErase: BOOL) -> BOOL;
    pub fn BeginPaint(hWnd: HWND, lpPaint: *mut PAINTSTRUCT) -> HDC;
    pub fn EndPaint(hWnd: HWND, lpPaint: *const PAINTSTRUCT) -> BOOL;
    pub fn GetWindowRect(hWnd: HWND, lpRect: *mut RECT) -> BOOL;
}

#[link(name = "gdi32")]
extern "system" {
    pub fn CreateFontW(
        cHeight: i32,
        cWidth: i32,
        cEscapement: i32,
        cOrientation: i32,
        cWeight: i32,
        bItalic: u32,
        bUnderline: u32,
        bStrikeOut: u32,
        iCharSet: u32,
        iOutPrecision: u32,
        iClipPrecision: u32,
        iQuality: u32,
        iPitchAndFamily: u32,
        pszFaceName: *const u16,
    ) -> HFONT;
    pub fn DeleteObject(ho: HANDLE) -> BOOL;
    pub fn CreateDCW(
        pwszDriver: *const u16,
        pwszDevice: *const u16,
        pszPort: *const u16,
        pdm: *const c_void,
    ) -> HDC;
    pub fn DeleteDC(hdc: HDC) -> BOOL;
    pub fn SetDeviceGammaRamp(hdc: HDC, lpRamp: *const c_void) -> BOOL;
    pub fn GetDeviceGammaRamp(hdc: HDC, lpRamp: *mut c_void) -> BOOL;
    pub fn CreateSolidBrush(color: u32) -> HBRUSH;
    pub fn CreatePen(style: i32, width: i32, color: u32) -> HPEN;
    pub fn SelectObject(hdc: HDC, h: HANDLE) -> HANDLE;
    pub fn GetStockObject(i: i32) -> HANDLE;
    pub fn SetBkMode(hdc: HDC, mode: i32) -> i32;
    pub fn SetBkColor(hdc: HDC, color: u32) -> u32;
    pub fn SetTextColor(hdc: HDC, color: u32) -> u32;
    pub fn FillRect(hdc: HDC, lprc: *const RECT, hbr: HBRUSH) -> i32;
    pub fn FrameRect(hdc: HDC, lprc: *const RECT, hbr: HBRUSH) -> i32;
    pub fn Rectangle(hdc: HDC, left: i32, top: i32, right: i32, bottom: i32) -> BOOL;
    pub fn RoundRect(
        hdc: HDC,
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
        width: i32,
        height: i32,
    ) -> BOOL;
    pub fn Ellipse(hdc: HDC, left: i32, top: i32, right: i32, bottom: i32) -> BOOL;
    pub fn DrawTextW(
        hdc: HDC,
        lpchText: *const u16,
        cchText: i32,
        lprc: *mut RECT,
        format: u32,
    ) -> i32;
}

#[link(name = "shell32")]
extern "system" {
    pub fn Shell_NotifyIconW(dwMessage: u32, lpData: *const NOTIFYICONDATAW) -> BOOL;
}

#[link(name = "comctl32")]
extern "system" {
    pub fn InitCommonControlsEx(lpInitCtrls: *const INITCOMMONCONTROLSEX) -> BOOL;
}

#[link(name = "uxtheme")]
extern "system" {
    pub fn SetWindowTheme(hwnd: HWND, pszSubAppName: *const u16, pszSubIdList: *const u16)
        -> i32;
}

#[link(name = "dxva2")]
extern "system" {
    pub fn GetNumberOfPhysicalMonitorsFromHMONITOR(hMonitor: HMONITOR, pdwNumber: *mut DWORD)
        -> BOOL;
    pub fn GetPhysicalMonitorsFromHMONITOR(
        hMonitor: HMONITOR,
        dwPhysicalMonitorArraySize: DWORD,
        pPhysicalMonitorArray: *mut PHYSICAL_MONITOR,
    ) -> BOOL;
    pub fn DestroyPhysicalMonitors(
        dwPhysicalMonitorArraySize: DWORD,
        pPhysicalMonitorArray: *mut PHYSICAL_MONITOR,
    ) -> BOOL;
    pub fn GetMonitorCapabilities(
        hMonitor: HANDLE,
        pdwMonitorCapabilities: *mut DWORD,
        pdwSupportedColorTemperatures: *mut DWORD,
    ) -> BOOL;
    pub fn GetMonitorBrightness(
        hMonitor: HANDLE,
        pdwMinimumBrightness: *mut DWORD,
        pdwCurrentBrightness: *mut DWORD,
        pdwMaximumBrightness: *mut DWORD,
    ) -> BOOL;
    pub fn SetMonitorBrightness(hMonitor: HANDLE, dwNewBrightness: DWORD) -> BOOL;
    pub fn GetVCPFeatureAndVCPFeatureReply(
        hMonitor: HANDLE,
        bVCPCode: u8,
        pvct: *mut i32,
        pdwCurrentValue: *mut DWORD,
        pdwMaximumValue: *mut DWORD,
    ) -> BOOL;
    pub fn SetVCPFeature(hMonitor: HANDLE, bVCPCode: u8, dwNewValue: DWORD) -> BOOL;
    pub fn SaveCurrentMonitorSettings(hMonitor: HANDLE) -> BOOL;
}

#[link(name = "kernel32")]
extern "system" {
    pub fn GetModuleHandleW(lpModuleName: *const u16) -> HINSTANCE;
    pub fn LoadLibraryW(lpLibFileName: *const u16) -> HINSTANCE;
    pub fn CreateMutexW(
        lpMutexAttributes: *mut c_void,
        bInitialOwner: BOOL,
        lpName: *const u16,
    ) -> HANDLE;
    pub fn GetLastError() -> u32;
    pub fn CloseHandle(hObject: HANDLE) -> BOOL;
    pub fn GetTickCount64() -> u64;
}

#[link(name = "advapi32")]
extern "system" {
    pub fn RegCreateKeyExW(
        hKey: HKEY,
        lpSubKey: *const u16,
        Reserved: u32,
        lpClass: *const u16,
        dwOptions: u32,
        samDesired: u32,
        lpSecurityAttributes: *const c_void,
        phkResult: *mut HKEY,
        lpdwDisposition: *mut u32,
    ) -> i32;
    pub fn RegSetValueExW(
        hKey: HKEY,
        lpValueName: *const u16,
        Reserved: u32,
        dwType: u32,
        lpData: *const u8,
        cbData: u32,
    ) -> i32;
    pub fn RegDeleteValueW(hKey: HKEY, lpValueName: *const u16) -> i32;
    pub fn RegCloseKey(hKey: HKEY) -> i32;
}

pub unsafe fn sys_to_unix(st: &SYSTEMTIME) -> i64 {
    // days_from_civil (Howard Hinnant)
    let y = st.wYear as i64;
    let m = st.wMonth as i64;
    let d = st.wDay as i64;
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = if y2 >= 0 { y2 } else { y2 - 399 } / 400;
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    days * 86_400
        + st.wHour as i64 * 3600
        + st.wMinute as i64 * 60
        + st.wSecond as i64
}

pub unsafe fn local_now() -> SYSTEMTIME {
    let mut st: SYSTEMTIME = std::mem::zeroed();
    GetLocalTime(&mut st);
    st
}

pub unsafe fn current_tz_offset_minutes() -> i32 {
    let mut lt: SYSTEMTIME = std::mem::zeroed();
    let mut ut: SYSTEMTIME = std::mem::zeroed();
    GetLocalTime(&mut lt);
    GetSystemTime(&mut ut);
    let diff = sys_to_unix(&lt) - sys_to_unix(&ut);
    (diff as f64 / 60.0).round() as i32
}

pub fn hwnd_text(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        from_wide(&buf)
    }
}

pub unsafe fn set_text(hwnd: HWND, s: &str) {
    let w = wide(s);
    SetWindowTextW(hwnd, w.as_ptr());
}

pub unsafe fn add_combo_string(combo: HWND, s: &str) {
    let w = wide(s);
    SendMessageW(combo, CB_ADDSTRING, 0, w.as_ptr() as isize);
}

pub unsafe fn combo_selection(combo: HWND) -> usize {
    let r = SendMessageW(combo, CB_GETCURSEL, 0, 0);
    if r < 0 {
        0
    } else {
        r as usize
    }
}

pub unsafe fn set_combo_selection(combo: HWND, index: usize) {
    SendMessageW(combo, CB_SETCURSEL, index, 0);
}

pub unsafe fn make_font(height: i32, weight: i32, face: &str) -> HFONT {
    let face_w = wide(face);
    CreateFontW(
        height,
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        1,
        0,
        0,
        5,
        0,
        face_w.as_ptr(),
    )
}
