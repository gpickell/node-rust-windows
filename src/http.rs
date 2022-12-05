use super::win32::*;

use windows::core::PCWSTR;
use windows::Win32::System::IO::*;
use windows::Win32::System::Threading::*;
use windows::Win32::Foundation::*;
use windows::Win32::Networking::HttpServer::*;

use core::ptr::*;
use std::ffi::*;
use std::mem;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;

#[allow(non_upper_case_globals)]
static ver: HTTPAPI_VERSION = HTTPAPI_VERSION {
    HttpApiMajorVersion: 2,
    HttpApiMinorVersion: 0,
};

#[allow(dead_code)]
enum HttpServerProperty {
    HttpServerAuthenticationProperty,
    HttpServerLoggingProperty,
    HttpServerQosProperty,
    HttpServerTimeoutsProperty,
    HttpServerQueueLengthProperty,
    HttpServerStateProperty,
    HttpServer503VerbosityProperty,
    HttpServerBindingProperty,
    HttpServerExtendedAuthenticationProperty,
    HttpServerListenEndpointProperty,
    HttpServerChannelBindProperty,
    HttpServerProtectionLevelProperty,
    HttpServerDelegationProperty = 16,
}

#[allow(dead_code)]
enum CreateRequestQueueFlags {
    OpenExisting = 1,
    Controller = 2,
    Delegation = 8,
}

struct Session {
    active: AtomicBool,
    controller: bool,
    queue: HANDLE,
    session: u64,
    urls: u64,
}

impl Session {
    pub fn create(name: Option<&str>) -> Result<Session, WinError> {
        unsafe {
            let mut controller = false;
            let mut flags = 0;
            let mut name_ptr = PCWSTR::null();
            let name_wide;
            if let Some(str) = name {
                controller = true;
                flags = CreateRequestQueueFlags::Controller as u32;
                name_wide = wide(str);
                name_ptr = wide_ptr(&name_wide);
            }

            let mut err: u32;
            err = HttpInitialize(ver, HTTP_INITIALIZE(1), None);
            if err != 0 {
                return Err(WinError("HttpInitialize", err));
            }

            let mut session: u64 = 0;
            err = HttpCreateServerSession(ver, &mut session, 0);
            if err != 0 {
                return Err(WinError("HttpCreateServerSession", err));
            }

            let mut urls: u64 = 0;
            err = HttpCreateUrlGroup(session, &mut urls, 0);
            if err != 0 {
                HttpCloseServerSession(session);
                return Err(WinError("HttpCreateUrlGroup", err));
            }

            let mut queue = HANDLE(-1);
            err = HttpCreateRequestQueue(ver, name_ptr, null_mut(), flags, &mut queue);
            if err != 0 {
                HttpCloseServerSession(session);
                HttpCloseUrlGroup(urls);
                return Err(WinError("HttpCreateRequestQueue", err));
            }

            let prop = HttpServerProperty::HttpServerBindingProperty;
            let info = HTTP_BINDING_INFO {
                Flags: HTTP_PROPERTY_FLAGS {
                    _bitfield: 1
                },
                RequestQueueHandle: queue
            };

            let size = mem::size_of::<HTTP_BINDING_INFO>() as u32;
            err = HttpSetUrlGroupProperty(urls, HTTP_SERVER_PROPERTY(prop as i32), &info as *const HTTP_BINDING_INFO as *const c_void, size);
            if err != 0 {
                HttpCloseUrlGroup(urls);
                HttpCloseServerSession(session);
                CloseHandle(queue);
                return Err(WinError("HttpSetUrlGroupProperty", err));
            }
   
            Ok(Self { active: AtomicBool::new(false), controller, queue, session, urls })
        }
    }

    pub fn open(name: &str) -> Result<Session, WinError> {
        unsafe {
            let flags = CreateRequestQueueFlags::OpenExisting as u32;
            let name_wide = wide(name);
            let name_ptr = wide_ptr(&name_wide);

            let mut err: u32;
            err = HttpInitialize(ver, HTTP_INITIALIZE(1), None);
            if err != 0 {
                return Err(WinError("HttpInitialize", err));
            }

            let mut queue = HANDLE(-1);
            err = HttpCreateRequestQueue(ver, name_ptr, null_mut(), flags, &mut queue);
            if err != 0 {
                return Err(WinError("HttpCreateRequestQueue", err));
            }

            Ok(Self { active: AtomicBool::new(false), controller: false, queue, session: 0, urls: 0 })
        }
    }

    pub fn listen(&self, url: &str) -> Result<(), WinError> {
        unsafe {
            let url_wide = wide(url);
            let err = HttpAddUrlToUrlGroup(self.urls, wide_ptr(&url_wide), 0, 0);
            if err != 0 {
                return Err(WinError("HttpSetUrlGroupProperty", err));
            }
    
            Ok(())
        }
    }

    pub fn request(&self) -> Result<Request, WinError> {
        unsafe {
            let mut queue = HANDLE(-1);
            let opts = DUPLICATE_HANDLE_OPTIONS(2);
            let this = GetCurrentProcess();
            let result1 = DuplicateHandle(this, self.queue, this, &mut queue, 0, false, opts);
            if !result1.as_bool() {
                return Err(WinError("DuplicateHandle", GetLastError().0));
            }

            if !bind_io(queue) {
                return Err(WinError("BindIoCompletionCallback", GetLastError().0));
            }

            Ok(Request { arc: HandleRef::new(queue) })
        }
    }
    
    pub fn close(&self) {
        if !self.active.swap(true, Relaxed) {
            unsafe {
                HttpCloseUrlGroup(self.urls);
                HttpCloseServerSession(self.session);
                CloseHandle(self.queue);
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.close();
    }
}

struct Request {
    arc: Arc<HandleRef>,
}

impl Request {
    pub async fn receive(&self, id: u64, size: u32) -> OverlappedResult<HTTP_REQUEST_V2> {
        unsafe {
            let arc = self.arc.clone();
            let mut helper = OverlappedHelper::new();
            let mut result = OverlappedResult::<HTTP_REQUEST_V2>::new(size);
            let flags = HTTP_RECEIVE_HTTP_REQUEST_FLAGS(0);
            let err = HttpReceiveHttpRequest(arc.0, id, flags, result.as_mut_ptr(), size, None, helper.as_mut_ptr());
            result.finish(arc.0, err, &mut helper).await;

            result
        }
    }

    pub async fn receive_data(&self, id: u64, size: u32) -> OverlappedResult<u8> {
        unsafe {
            let arc = self.arc.clone();
            let mut helper = OverlappedHelper::new();
            let mut result = OverlappedResult::<u8>::new(size);
            let err = HttpReceiveRequestEntityBody(arc.0, id, 0, result.as_mut_ptr() as *mut c_void, size, None, helper.as_mut_ptr());
            result.finish(arc.0, err, &mut helper).await;

            result
        }
    }

    pub fn close(&self) {
        unsafe {
            CancelIo(self.arc.0);
        }
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        self.close();
    }
}

/*
async fn fun(req: Arc<Request>) {
    let r1 = req.receive(0, 4096).await;
    println!("receive {} {} {}", r1.err, r1.more, r1.size);

    let id = r1.as_ref().Base.RequestId;
    let r2 = req.receive_data(id, 4096).await;
    println!("receive_data {} {} {}", r2.err, r2.more, r2.size);
}

fn sandbox() -> Result<(), WinError> {
    let session = Session::create(Some("test-v4"))?;
    session.listen("http://localhost:9480/")?;

    let session2 = Session::open("test-v4")?;
    let req = Arc::new(session2.request()?);
    tasks().spawn_ok(fun(req.clone()));

    unsafe { Sleep(300000); }
    drop(session);
    drop(session2);

    Ok(())
}

pub fn ____test() {
    std::thread::spawn(move || {
        if let Err(err) = sandbox() {
            println!("{}", err);      
        }
    });
}*/

use neon::prelude::*;
use super::support::*;
// use neon::types::buffer::TypedArray;

/*
async fn fill(tx: Channel, root_cb: Root<JsFunction>, root_buf: Root<JsBuffer>) {
    if let Ok(session) = Session::open("test-v4") {
        if let Ok(req) = session.request() {
            let r1 = req.receive(0, 4096).await;
            let id = r1.as_ref().Base.RequestId;
            let r2 = req.receive_data(id, 4096).await;
            println!("--- r2 {}", r2.err);

            tx.send(move |mut cx| {
                let cb = root_cb.into_inner(&mut cx);
                let mut buf = root_buf.into_inner(&mut cx);

                unsafe {
                    let data = buf.as_mut_slice(&mut cx);
                    copy(r2.as_ptr(), &mut data[0], r2.size as usize);
                }

                cb.call_with(&mut cx).arg(buf).exec(&mut cx)?;

                Ok(())
            });
        }
    }
}

fn sandbox() -> Result<(), WinError> {
    let session = Session::create(Some("test-v4"))?;
    session.listen("http://localhost:9480/")?;

    unsafe { Sleep(300000); }
    drop(session);

    Ok(())
}

pub fn __test(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    std::thread::spawn(move || {
        if let Err(err) = sandbox() {
            println!("{}", err);
        }
    });

    let cb = cx.argument::<JsFunction>(0)?;
    let buf = cx.buffer(4096)?;
    tasks().spawn_ok(fill(cx.channel(), cb.root(&mut cx), buf.root(&mut cx)));
    
    return Ok(cx.undefined());
}*/

fn http_session_create(mut cx: FunctionContext) -> JsArcResult<Session> {
    let name: String;
    let mut name_opt: Option<&str> = None;
    if let Some(arg) = opt_arg_at::<JsString>(&mut cx, 0)? {
        name = arg.value(&mut cx);
        name_opt = Some(&name);
    }

    match Session::create(name_opt) {
        Ok(session) => JsArc::export(&mut cx, session),
        Err(err) => cx.throw_type_error(format!("{}", err))
    }
}

fn http_session_open(mut cx: FunctionContext) -> JsArcResult<Session> {
    let arg = cx.argument::<JsString>(0)?;
    let name = arg.value(&mut cx);
    match Session::open(&name) {
        Ok(session) => JsArc::export(&mut cx, session),
        Err(err) => cx.throw_type_error(format!("{}", err))
    } 
}

fn http_session_is_controller(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let arc = JsArc::<Session>::import(&mut cx, 0)?;
    Ok(cx.boolean(arc.controller))
}

fn http_session_listen(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    let arc = JsArc::<Session>::import(&mut cx, 0)?;
    let url_arg = cx.argument::<JsString>(1)?;
    let url = url_arg.value(&mut cx);
    match arc.listen(&url) {
        Ok(()) => Ok(cx.undefined()),
        Err(err) => cx.throw_type_error(format!("{}", err))
    } 
}

fn http_session_close(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    let arc = JsArc::<Session>::import(&mut cx, 0)?;
    arc.close();

    Ok(cx.undefined())
}

fn http_session_request(mut cx: FunctionContext) -> JsArcResult<Request> {
    let arc = JsArc::<Session>::import(&mut cx, 0)?;
    match arc.request() {
        Ok(request) => JsArc::export(&mut cx, request),
        Err(err) => cx.throw_type_error(format!("{}", err))
    } 
}

fn http_request_receive(mut cx: FunctionContext) -> JsResult<JsPromise> {
    let mut id = 0u64;
    let mut size = 4096u32;
    let arc = JsArc::<Request>::import(&mut cx, 0)?;
    if let Some(arg) = opt_arg_at::<JsBox<u64>>(&mut cx, 1)? {
        id = **arg;
    }

    if let Some(arg) = opt_arg_at::<JsNumber>(&mut cx, 2)? {
        size = arg.value(&mut cx) as u32;
    }
    
    let tx = cx.channel();
    let (def, promise) = cx.promise();
    let func = async move {
        let result = arc.receive(id, size).await;
        def.settle_with(&tx, move |mut cx| {
            let info = &result.as_ref().Base;
            let obj = cx.empty_object();
            let js_err = cx.number(result.err);
            obj.set(&mut cx, "code", js_err)?;

            let js_more = cx.boolean(result.more);
            obj.set(&mut cx, "more", js_more)?;

            if result.err != 0 {
                return Ok(obj);
            }

            let js_id = cx.boxed(info.RequestId);
            obj.set(&mut cx, "id", js_id)?;

            if result.more {
                return Ok(obj);
            }

            let js_verb = cx.number(info.Verb.0);
            obj.set(&mut cx, "verb", js_verb)?;

            Ok(obj)
        });
    };

    tasks().spawn_ok(func);
    Ok(promise)
}

fn http_request_receive_data(mut cx: FunctionContext) -> JsResult<JsPromise> {
    let mut size = 4096u32;
    let arc = JsArc::<Request>::import(&mut cx, 0)?;
    let id = **cx.argument::<JsBox<u64>>(1)?;
    if let Some(arg) = opt_arg_at::<JsNumber>(&mut cx, 2)? {
        size = arg.value(&mut cx) as u32;
    }
    
    let tx = cx.channel();
    let (def, promise) = cx.promise();
    let func = async move {
        let result = arc.receive_data(id, size).await;
        def.settle_with(&tx, move |mut cx| {
            let info = &result.as_ref().Base;
            let obj = cx.empty_object();
            let js_err = cx.number(result.err);
            obj.set(&mut cx, "code", js_err)?;

            let js_more = cx.boolean(result.more);
            obj.set(&mut cx, "more", js_more)?;

            if result.err != 0 {
                return Ok(obj);
            }

            let js_id = cx.boxed(info.RequestId);
            obj.set(&mut cx, "id", js_id)?;


            Ok(obj)
        });
    };

    tasks().spawn_ok(func);
    Ok(promise)
}

pub fn http_bind(cx: &mut ModuleContext) -> NeonResult<()> {
    cx.export_function("http_session_create", http_session_create)?;
    cx.export_function("http_session_open", http_session_open)?;
    cx.export_function("http_session_is_controller", http_session_is_controller)?;
    cx.export_function("http_session_listen", http_session_listen)?;
    cx.export_function("http_session_request", http_session_request)?;
    cx.export_function("http_session_close", http_session_close)?;

    cx.export_function("http_request_receive", http_request_receive)?;
    cx.export_function("http_request_receive_data", http_request_receive_data)?;

    Ok(())
}
