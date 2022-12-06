use futures::executor::ThreadPool;
use futures::Future;
use futures::task::AtomicWaker;

use windows::core::PCWSTR;
use windows::Win32::Foundation::*;
use windows::Win32::System::IO::*;

use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::fmt;
use std::pin::Pin;
use std::ptr::null_mut;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;

#[allow(non_upper_case_globals)]
static pool_init: AtomicBool = AtomicBool::new(false);

#[allow(non_upper_case_globals)]
static pool_mx: Mutex<()> = Mutex::new(());

#[allow(non_upper_case_globals)]
static mut pool: Option<ThreadPool> = None;

pub fn tasks() -> &'static ThreadPool {
    if pool_init.load(Relaxed) {
        unsafe {
            if let Some(ref tasks) = pool {
                return tasks;
            }
        }
    }

    if pool_mx.lock().is_ok() {
        if pool_init.load(Relaxed) {
            unsafe {
                if let Some(ref tasks) = pool {
                    return tasks;
                }
            }
        }
        
        if let Ok(result) = ThreadPool::new() {
            unsafe {
                pool = Some(result);

                if let Some(ref tasks) = pool {
                    return tasks;
                }
            }
        }

        pool_init.store(true, Relaxed);
    }

    panic!("ThreadPool did not create.");
}

pub fn wide(data: &str) -> Vec<u16> {
    let mut str = OsString::from(data);
    str.push("\0");

    return str.encode_wide().collect::<Vec<u16>>();
}

pub fn wide_ptr(data: &Vec<u16>) -> PCWSTR {
    return PCWSTR(Vec::as_ptr(data));
}

unsafe extern "system" fn recv(_: u32, _: u32, o: *mut OVERLAPPED) {
    let helper = &*(o as *const OverlappedState);
    helper.1.send();
}

pub unsafe fn bind_io(h: HANDLE) -> bool {
    BindIoCompletionCallback(h, Some(recv), 0).as_bool()
}

pub struct ArcWaker {
    base: Arc<(AtomicBool, AtomicWaker)>
}

impl ArcWaker {
    pub fn new() -> Self {
        let base = Arc::new((AtomicBool::new(false), AtomicWaker::new()));
        Self { base }
    }

    pub fn send(&self) {        
        let state = self.base.as_ref();
        state.0.store(true, Relaxed);
        state.1.wake();        
    }

    pub fn clone(&self) -> Self {
        let base = self.base.clone();
        Self { base }
    }
}

impl Future for ArcWaker {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let state = self.base.as_ref();
        if state.0.load(Relaxed) {
            return Poll::Ready(());
        }

        state.1.register(cx.waker());

        if state.0.load(Relaxed) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }   
}

type OverlappedState = (OVERLAPPED, ArcWaker);

pub struct OverlappedHelper {
    base: Box<OverlappedState>,
}

impl OverlappedHelper {
    pub unsafe fn new() -> OverlappedHelper {
        let base = Box::new((OVERLAPPED::default(), ArcWaker::new()));
        Self { base }
    }

    pub fn wait(&self) -> ArcWaker {
        self.base.1.clone()
    }

    pub fn as_mut_ptr(&mut self) -> *mut OVERLAPPED {
        &mut self.base.as_mut().0
    }
}

unsafe impl Send for OverlappedHelper {
    
}

pub enum Buffer {
    Auto(u32),
    Slice(*mut u8, u32),
    Vector(Vec<u8>),
}

use Buffer::*;

impl<'a> Buffer {
    pub fn as_ptr(&self) -> *const u8 {
        match self {
            Auto(_) => null_mut(),
            Slice(ptr, _) => *ptr,
            Vector(vec) => vec.as_ptr()
        }
    }

    pub fn as_mut_ptr(&mut self) -> *mut u8 {
        match self {
            Auto(_) => null_mut(),
            Slice(ptr, _) => *ptr,
            Vector(vec) => vec.as_mut_ptr()
        }
    }

    pub fn capacity(&self) -> u32 {
        match self {
            Auto(_) => 0,
            Slice(_, capacity) => *capacity,
            Vector(vec) => vec.capacity() as u32,
        }
    }

    pub fn alloc(self, min: u32) -> Self {
        if let Buffer::Auto(hint) = self {
            let size = match hint > min { true => hint, false => min };
            return Vector(Vec::<u8>::with_capacity(size as usize));
        }

        if self.capacity() < min {
            return Vector(Vec::<u8>::with_capacity(min as usize));
        }

        self
    }
}

unsafe impl Send for Buffer {

}

pub struct OverlappedResult<T> {
    pub err: u32,
    pub more: bool,
    pub size: u32,
    pub data: Buffer,
    pub data_ptr: *const T
}

unsafe impl<T> Send for OverlappedResult<T> {
    
}

impl<'a, T> OverlappedResult<T> {
    pub fn new(target: Buffer, min: u32) -> Self {
        let data = target.alloc(min);
        let data_ptr = data.as_ptr() as *const T;
        Self {
            err: 0,
            more: false,
            size: 0,
            data,
            data_ptr
        }
    }

    pub fn as_ref(&self) -> &T {
        unsafe {
            &*self.data_ptr
        }
    }

    pub fn as_ptr(&self) -> *const T {
        self.data_ptr
    }

    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.data.as_mut_ptr() as *mut T
    }

    pub fn capacity(&self) -> u32 {
        self.data.capacity()
    }

    pub fn complete(&mut self, h: HANDLE, o: *mut OVERLAPPED) {
        unsafe {
            let mut err = 0;
            let mut more = false;
            let mut size = 0;
            let r = GetOverlappedResult(h, o, &mut size, false);
            if !r.as_bool() {
                err = GetLastError().0;

                if err == ERROR_MORE_DATA.0 {
                    err = 0;
                    more = true;
                }
            }

            self.err = err;
            self.more = more;
            self.size = size;
        }
    }

    pub fn fail(&mut self, err: u32) {
        if err == ERROR_MORE_DATA.0 {
            self.err = 0;
            self.more = true;
        } else {
            self.err = err;
        }
    }

    pub async fn finish(&mut self, h: HANDLE, err: u32, helper: &mut OverlappedHelper) {
        if err == 0 || err == ERROR_IO_PENDING.0 {
            let waiter = helper.wait();
            waiter.await;
            
            self.complete(h, helper.as_mut_ptr());
        } else {
            self.fail(err);
        }
    }
}

#[derive(Debug)]
pub struct WinError(pub &'static str, pub u32);

impl std::error::Error for WinError {

}

impl fmt::Display for WinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: Win32_Error = {}", self.0, self.1)
    }
}

pub struct HandleRef(pub HANDLE);

impl HandleRef {
    pub fn new(h: HANDLE) -> Arc<Self> {
        Arc::new(Self(h))
    }
}

impl Drop for HandleRef {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }        
    }
}
