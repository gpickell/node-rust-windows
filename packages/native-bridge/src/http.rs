use super::win32::*;

use windows::core::PCSTR;
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
static ver_init: HTTPAPI_VERSION = HTTPAPI_VERSION {
    HttpApiMajorVersion: 2,
    HttpApiMinorVersion: 0,
};

struct SendRef<T>(T);

unsafe impl<T> Send for SendRef<T> {

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
                flags = HTTP_CREATE_REQUEST_QUEUE_FLAG_CONTROLLER;
                name_wide = wide(str);
                name_ptr = wide_ptr(&name_wide);
            }

            let mut err: u32;
            err = HttpInitialize(ver_init, HTTP_INITIALIZE_SERVER, None);
            if err != 0 {
                return Err(WinError("HttpInitialize", err));
            }

            let mut session: u64 = 0;
            err = HttpCreateServerSession(ver_init, &mut session, 0);
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
            err = HttpCreateRequestQueue(ver_init, name_ptr, null_mut(), flags, &mut queue);
            if err != 0 {
                HttpCloseServerSession(session);
                HttpCloseUrlGroup(urls);
                return Err(WinError("HttpCreateRequestQueue", err));
            }

            let prop = HttpServerBindingProperty;
            let info = HTTP_BINDING_INFO {
                Flags: HTTP_PROPERTY_FLAGS {
                    _bitfield: 1
                },
                RequestQueueHandle: queue
            };

            let size = mem::size_of::<HTTP_BINDING_INFO>() as u32;
            err = HttpSetUrlGroupProperty(urls, prop, &info as *const HTTP_BINDING_INFO as *const c_void, size);
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
            let flags = HTTP_CREATE_REQUEST_QUEUE_FLAG_OPEN_EXISTING;
            let name_wide = wide(name);
            let name_ptr = wide_ptr(&name_wide);

            let mut err: u32;
            err = HttpInitialize(ver_init, HTTP_INITIALIZE(1), None);
            if err != 0 {
                return Err(WinError("HttpInitialize", err));
            }

            let mut queue = HANDLE(-1);
            err = HttpCreateRequestQueue(ver_init, name_ptr, null_mut(), flags, &mut queue);
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

            Ok(Request::new(queue))
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
    cancel_all: AtomicBool,
}

impl Request {
    pub fn new(h: HANDLE) -> Self {
        Self {
            arc: HandleRef::new(h),
            cancel_all: AtomicBool::new(false),
        }
    }

    unsafe fn cancel_io_maybe(&self, h: HANDLE) {
        if self.cancel_all.load(Relaxed) {
            CancelIo(h);
        }
    }

    pub async fn cancel(&self, id: u64) -> OverlappedResult<()> {
        unsafe {
            let arc = self.arc.clone();
            let mut helper = OverlappedHelper::new();
            let mut result = OverlappedResult::<()>::new(Auto(0), 0);
            let err = HttpCancelHttpRequest(arc.0, id, helper.as_mut_ptr());

            self.cancel_io_maybe(arc.0);
            result.finish(arc.0, err, &mut helper).await;

            result
        }
    }

    pub async fn receive(&self, id: u64, target: Buffer) -> OverlappedResult<HTTP_REQUEST_V2> {
        unsafe {
            let arc = self.arc.clone();
            let mut helper = OverlappedHelper::new();
            let mut result = OverlappedResult::<HTTP_REQUEST_V2>::new(target, 1024);
            let flags = HTTP_RECEIVE_HTTP_REQUEST_FLAGS(0);
            let err = HttpReceiveHttpRequest(arc.0, id, flags, result.as_mut_ptr(), result.capacity(), None, helper.as_mut_ptr());

            self.cancel_io_maybe(arc.0);
            result.finish(arc.0, err, &mut helper).await;

            result
        }
    }

    pub async fn receive_data(&self, id: u64, target: Buffer) -> OverlappedResult<u8> {
        unsafe {
            let arc = self.arc.clone();
            let mut helper = OverlappedHelper::new();
            let mut result = OverlappedResult::<u8>::new(target, 256);
            let err = HttpReceiveRequestEntityBody(arc.0, id, 0, result.as_mut_ptr() as *mut c_void, result.capacity(), None, helper.as_mut_ptr());

            self.cancel_io_maybe(arc.0);
            result.finish(arc.0, err, &mut helper).await;

            result
        }
    }

    pub async fn send(&self, id: u64, flags: u32, source: SendRef<*mut HTTP_RESPONSE_V2>) -> OverlappedResult<u32> {
        unsafe {
            let arc = self.arc.clone();
            let mut helper = OverlappedHelper::new();
            let mut result = OverlappedResult::<u32>::new(Auto(0), 4);
            let err = HttpSendHttpResponse(arc.0, id, flags, source.0, null_mut(), result.as_mut_ptr(), None, 0, helper.as_mut_ptr(), null_mut());

            self.cancel_io_maybe(arc.0);
            result.finish(arc.0, err, &mut helper).await;

            result
        }
    }

    pub fn close(&self) {
        self.cancel_all.store(true, Relaxed);

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

use Buffer::*;

use neon::prelude::*;
use super::support::*;
use neon::types::buffer::TypedArray;

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

fn http_request_cancel(mut cx: FunctionContext) -> JsResult<JsPromise> {
    let arc = JsArc::<Request>::import(&mut cx, 0)?;
    let id = **cx.argument::<JsBox<u64>>(1)?;
    let tx = cx.channel();
    let (def, promise) = cx.promise();
    let func = async move {
        let result = arc.cancel(id).await;
        def.settle_with(&tx, move |mut cx| {
            Ok(cx.number(result.err))
        });
    };
    
    tasks().spawn_ok(func);
    Ok(promise)
}

fn http_request_receive(mut cx: FunctionContext) -> JsResult<JsPromise> {
    let mut size = 4096u32;
    let arc = JsArc::<Request>::import(&mut cx, 0)?;
    if let Some(arg) = opt_arg_at::<JsNumber>(&mut cx, 1)? {
        size = arg.value(&mut cx) as u32;
    }
           
    let tx = cx.channel();
    let (def, promise) = cx.promise();
    let func = async move {
        let mut tmp = arc.receive(0, Auto(size)).await;
        if tmp.more {
            let id = tmp.as_ref().Base.RequestId;
            tmp = arc.receive(id, Auto(tmp.size)).await;
        }

        let result = tmp;
        def.settle_with(&tx, move |mut cx| {
            let info = &result.as_ref().Base;
            let obj = cx.empty_object();
            let js_err = cx.number(result.err);
            obj.set(&mut cx, "code", js_err)?;

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

            let ver = &info.Version;
            let version_str = format!("{}.{}", ver.MajorVersion, ver.MinorVersion);
            let js_version = cx.string(version_str);
            obj.set(&mut cx, "version", js_version)?;

            let body = (info.Flags & HTTP_REQUEST_FLAG_MORE_ENTITY_BODY_EXISTS) != 0;
            let js_body = cx.boolean(body);
            obj.set(&mut cx, "body", js_body)?;

            let http2 = (info.Flags & HTTP_REQUEST_FLAG_HTTP2) != 0;
            let js_http2 = cx.boolean(http2);
            obj.set(&mut cx, "http2", js_http2)?;
           
            unsafe {
                if info.UnknownVerbLength > 0 {
                    if let Ok(value) = info.pUnknownVerb.to_string() {
                        let js_custom_verb = cx.string(value);
                        obj.set(&mut cx, "customVerb", js_custom_verb)?;            
                    }
                }

                if info.RawUrlLength > 0 {
                    if let Ok(value) = info.pRawUrl.to_string() {
                        let js_url = cx.string(value);
                        obj.set(&mut cx, "url", js_url)?;            
                    }
                }

                let js_known = cx.empty_array();
                obj.set(&mut cx, "knownHeaders", js_known)?;

                let known = &info.Headers.KnownHeaders;
                for i in 0..known.len() {
                    let header = &known[i];
                    if header.RawValueLength > 0 {
                        if let Ok(value) = header.pRawValue.to_string() {
                            let key = format!("{}", i);
                            let js_value = cx.string(value);
                            js_known.set(&mut cx, key.as_str(), js_value)?;
                        }
                    }
                }

                let js_unknown = cx.empty_array();
                obj.set(&mut cx, "unknownHeaders", js_unknown)?;
                
                let mut next = info.Headers.pUnknownHeaders;
                let last = next.add(info.Headers.UnknownHeaderCount as usize);
                while next < last {
                    let header = &*next;
                    next = next.add(1);

                    if header.NameLength > 0 && header.RawValueLength > 0 {
                        if let Ok(key) = header.pName.to_string() {
                            if let Ok(value) = header.pRawValue.to_string() {
                                let js_value = cx.string(value);
                                js_unknown.set(&mut cx, key.as_str(), js_value)?;
                            }
                        }
                    }
                }
            }

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
    let mut data = cx.array_buffer(size as usize)?;
    let root = data.root(&mut cx);
    let slice = data.as_mut_slice(&mut cx);
    let target = Slice(&mut slice[0], slice.len() as u32);
    let (def, promise) = cx.promise();
    let func = async move {
        let result = arc.receive_data(id, target).await;
        def.settle_with(&tx, move |mut cx| {
            let obj = cx.empty_object();
            let js_err = cx.number(result.err);
            obj.set(&mut cx, "code", js_err)?;

            let js_eof = cx.boolean(result.err == ERROR_HANDLE_EOF.0);
            obj.set(&mut cx, "eof", js_eof)?;

            if result.err != 0 {
                return Ok(obj);
            }

            let js_size = cx.number(result.size);
            obj.set(&mut cx, "size", js_size)?;

            let js_data = root.to_inner(&mut cx);
            let slice = js_data.as_slice(&mut cx);
            if &slice[0] as *const u8 == result.as_ptr() {
                obj.set(&mut cx, "data", js_data)?;
                return Ok(obj);
            }

            let mut js_data = cx.array_buffer(result.size as usize)?;
            obj.set(&mut cx, "data", js_data)?;

            unsafe {
                let slice = js_data.as_mut_slice(&mut cx);
                copy(result.as_ptr(), &mut slice[0], result.size as usize);
            }

            Ok(obj)
        });
    };

    tasks().spawn_ok(func);
    Ok(promise)
}

fn http_request_send(mut cx: FunctionContext) -> JsResult<JsPromise> {
    let arc = JsArc::<Request>::import(&mut cx, 0)?;
    let id = **cx.argument::<JsBox<u64>>(1)?;
    let block = cx.argument::<JsBuffer>(2)?;
    let root = block.root(&mut cx);
    let mut unknown = Vec::<HTTP_UNKNOWN_HEADER>::new();
    let mut response = Box::<HTTP_RESPONSE_V2>::new(HTTP_RESPONSE_V2 {
        Base: HTTP_RESPONSE_V1 {
            Flags: 0,
            Version: HTTP_VERSION {
                MajorVersion: 0,
                MinorVersion: 0,
            },
            StatusCode: 0,
            ReasonLength: 0,
            pReason: PCSTR::null(),
            Headers: HTTP_RESPONSE_HEADERS {
                UnknownHeaderCount: 0,
                pUnknownHeaders: null_mut(),
                TrailerCount: 0,
                pTrailers: null_mut(),
                KnownHeaders: [HTTP_KNOWN_HEADER { RawValueLength: 0, pRawValue: PCSTR::null() }; 30]
            },
            EntityChunkCount: 0,
            pEntityChunks: null_mut(),
        },
        ResponseInfoCount: 0,
        pResponseInfo: null_mut(),
    });

    let base = &mut response.as_mut().Base;
    let mut i = 3..cx.len();
    let opaque = arg_at::<JsBoolean>(&mut cx, &mut i)?.value(&mut cx);
    let more = arg_at::<JsBoolean>(&mut cx, &mut i)?.value(&mut cx);
    let status = arg_at::<JsNumber>(&mut cx, &mut i)?.value(&mut cx);
    let major = arg_at::<JsNumber>(&mut cx, &mut i)?.value(&mut cx);
    let minor = arg_at::<JsNumber>(&mut cx, &mut i)?.value(&mut cx);
    base.StatusCode = status as u16;
    base.Version = HTTP_VERSION {
        MajorVersion: major as u16,
        MinorVersion: minor as u16,
    };

    let reason = arg_ptr_at(&mut cx, &block, &mut i)?;
    base.ReasonLength = reason.1 as u16;
    base.pReason = PCSTR(reason.0);

    while !i.is_empty() {
        let id = arg_at::<JsNumber>(&mut cx, &mut i)?.value(&mut cx);
        let value = arg_ptr_at(&mut cx, &block, &mut i)?;
        if id < 0.0 {
            let name = arg_ptr_at(&mut cx, &block, &mut i)?;
            unknown.push(HTTP_UNKNOWN_HEADER {
                NameLength: name.1 as u16,
                pName: PCSTR(name.0),
                RawValueLength: value.1 as u16,
                pRawValue: PCSTR(value.0)
            });

            base.Headers.UnknownHeaderCount = unknown.len() as u16;
            base.Headers.pUnknownHeaders = unknown.as_mut_ptr();            
        } else {
            base.Headers.KnownHeaders[id as usize] = HTTP_KNOWN_HEADER {
                RawValueLength: value.1 as u16,
                pRawValue: PCSTR(value.0)
            };
        }
    }

    let mut flags = 0;
    if opaque {
        flags |= HTTP_SEND_RESPONSE_FLAG_OPAQUE;
    }

    if more {
        flags |= HTTP_SEND_RESPONSE_FLAG_MORE_DATA;
    }

    let source = SendRef(response.as_mut() as *mut HTTP_RESPONSE_V2);
    let transfer = SendRef((root, unknown, response));
    let tx = cx.channel();
    let (def, promise) = cx.promise();
    let func = async move {
        let result = arc.send(id, flags, source).await;
        drop(transfer);

        def.settle_with(&tx, move |mut cx| {
            Ok(cx.number(result.err))
        });
    };

    tasks().spawn_ok(func);
    Ok(promise)
}

fn http_request_close(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    let arc = JsArc::<Request>::import(&mut cx, 0)?;
    arc.close();

    Ok(cx.undefined())
}

pub fn http_bind(cx: &mut ModuleContext) -> NeonResult<()> {
    cx.export_function("http_session_create", http_session_create)?;
    cx.export_function("http_session_open", http_session_open)?;
    cx.export_function("http_session_is_controller", http_session_is_controller)?;
    cx.export_function("http_session_listen", http_session_listen)?;
    cx.export_function("http_session_request", http_session_request)?;
    cx.export_function("http_session_close", http_session_close)?;

    cx.export_function("http_request_cancel", http_request_cancel)?;
    cx.export_function("http_request_receive", http_request_receive)?;
    cx.export_function("http_request_receive_data", http_request_receive_data)?;
    cx.export_function("http_request_send", http_request_send)?;
    // cx.export_function("http_request_send_data", http_request_send_data)?;
    cx.export_function("http_request_close", http_request_close)?;

    Ok(())
}
