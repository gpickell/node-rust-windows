use neon::types::Finalize;

use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::System::IO::*;

use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;

pub fn wide(data: &str) -> Vec<u16> {
    let mut str = OsString::from(data);
    str.push("\0");

    return str.encode_wide().collect::<Vec<u16>>();
}

pub fn wide_ptr(data: &Vec<u16>) -> PCWSTR {
    return PCWSTR(Vec::as_ptr(data));
}

struct OverlappedEx(OVERLAPPED, *mut (dyn FnOnce(*mut OVERLAPPED, u32) + Send + 'static));

pub fn into_async<F>(f: F) -> *mut OVERLAPPED where F: FnOnce(*mut OVERLAPPED, u32) + Send + 'static {
    let o = OverlappedEx(OVERLAPPED::default(), Box::into_raw(Box::new(f)));
    Box::into_raw(Box::new(o)) as *mut OVERLAPPED
}

pub fn call_async(ptr: *mut OVERLAPPED, err: u32) {
    unsafe {
        let ex = Box::from_raw(ptr as *mut OverlappedEx);
        let cb = Box::from_raw(ex.1);
        cb(ptr, err);
        drop(ex);
    }
}

unsafe extern "system" fn recv(_: u32, _: u32, ptr: *mut OVERLAPPED) {
    call_async(ptr, ERROR_IO_PENDING.0);
}

pub unsafe fn bind_io(h: HANDLE) -> bool {
    BindIoCompletionCallback(h, Some(recv), 0).as_bool()
}

pub struct HandleRef(pub HANDLE, AtomicBool);

impl HandleRef {
    pub fn new(h: HANDLE) -> Arc<Self> {
        Arc::new(Self(h, AtomicBool::new(false)))
    }

    pub fn wrap<F>(self: &Arc<Self>, f: F) -> *mut OVERLAPPED where F: FnOnce(u32, u32) + Send + 'static {
        let h = self.clone();
        into_async(move |o, err| unsafe {
            let mut result = err;
            let mut size = 0u32;
            if result == ERROR_IO_PENDING.0 {
                let ok = GetOverlappedResult(h.0, o, &mut size, false);
                if ok.as_bool() {
                    result = 0;
                } else {
                    result = GetLastError().0;
    
                    if result != ERROR_MORE_DATA.0 {
                        size = 0;
                    }
                }
            }
    
            f(result, size);
        })
    }

    pub fn cancel(self: &Arc<Self>) {
        if !self.1.swap(true, Relaxed) {
            unsafe {
                CancelIoEx(self.0, None);
            }            
        }
    }

    pub fn cleanup(self: &Arc<Self>, ptr: *mut OVERLAPPED, err: u32) {
        if err != 0 && err != ERROR_MORE_DATA.0 && err != ERROR_IO_PENDING.0 {
            call_async(ptr, err);
        }

        if self.1.load(Relaxed) {
            unsafe {
                CancelIoEx(self.0, None);
            }            
        }
    }
}

impl Finalize for HandleRef {}

impl Drop for HandleRef {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub struct SendRef<T>(pub T);

unsafe impl<T> Send for SendRef<T> {

}

unsafe impl<T> Sync for SendRef<T> {

}
