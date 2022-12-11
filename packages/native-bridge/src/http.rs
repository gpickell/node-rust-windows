use super::win32::*;

use windows::core::PCSTR;
use windows::core::PCWSTR;

use windows::Win32::System::IO::*;
use windows::Win32::Foundation::*;
use windows::Win32::Networking::HttpServer::*;

use core::ptr::*;
use std::ffi::*;
use std::slice::from_raw_parts;
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
    queue: HANDLE,
    session: u64,
    urls: u64,
}

impl Session {
    pub fn create(name: &str) -> Result<Session, WinError> {
        unsafe {
            let flags = HTTP_CREATE_REQUEST_QUEUE_FLAG_CONTROLLER;
            let name_wide = wide(name);
            let name_ptr = wide_ptr(&name_wide);

            let mut session: u64 = 0;
            let err = HttpCreateServerSession(ver_init, &mut session, 0);
            if err != 0 {
                return Err(WinError("HttpCreateServerSession", err));
            }

            let mut urls: u64 = 0;
            let err = HttpCreateUrlGroup(session, &mut urls, 0);
            if err != 0 {
                HttpCloseServerSession(session);
                return Err(WinError("HttpCreateUrlGroup", err));
            }

            let mut queue = HANDLE(-1);
            let err = HttpCreateRequestQueue(ver_init, name_ptr, null_mut(), flags, &mut queue);
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
            let err = HttpSetUrlGroupProperty(urls, prop, &info as *const HTTP_BINDING_INFO as *const c_void, size);
            if err != 0 {
                HttpCloseUrlGroup(urls);
                HttpCloseServerSession(session);
                CloseHandle(queue);
                return Err(WinError("HttpSetUrlGroupProperty", err));
            }
   
            Ok(Self { active: AtomicBool::new(false), queue, session, urls })
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
            CancelIoEx(h, None);
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

    pub async fn send_data(&self, id: u64, flags: u32, source: SendRef<*mut HTTP_DATA_CHUNK>, count: u16) -> OverlappedResult<u32> {
        unsafe {
            let arc = self.arc.clone();
            let mut helper = OverlappedHelper::new();
            let mut result = OverlappedResult::<u32>::new(Auto(0), 4);
            let slice = SendRef(from_raw_parts(source.0, count as usize));
            let err = HttpSendResponseEntityBody(arc.0, id, flags, Some(slice.0), result.as_mut_ptr(), None, 0, helper.as_mut_ptr(), null_mut());
            self.cancel_io_maybe(arc.0);
            result.finish(arc.0, err, &mut helper).await;

            result
        }
    }

    pub fn push(&self, id: u64, verb: i32, path: *const u16, query: *const u8, headers: *const HTTP_REQUEST_HEADERS ) -> Result<(), WinError> {
        unsafe {
            let arc = self.arc.clone();
            let mut query_opt: Option<PCSTR> = None;
            let tail = query.add(1);
            if *tail != 0 {
                query_opt = Some(PCSTR(tail))
            }

            let err = HttpDeclarePush(arc.0, id, HTTP_VERB(verb), PCWSTR(path), query_opt, Some(headers));
            if err != 0 {
                return Err(WinError("HttpDeclarePush", err));
            }

            Ok(())
        }
    }
    
    pub fn close(&self) {
        self.cancel_all.store(true, Relaxed);

        unsafe {
            CancelIoEx(self.arc.0, None);
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
    let name = cx.argument::<JsString>(0)?.value(&mut cx);
    match Session::create(&name) {
        Ok(session) => JsArc::export(&mut cx, session),
        Err(err) => cx.throw_type_error(format!("{}", err))
    }
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
    let target = Slice(slice.as_mut_ptr(), slice.len() as u32);
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
            if slice.as_ptr() == result.as_ptr() {
                obj.set(&mut cx, "data", js_data)?;
                return Ok(obj);
            }

            let mut js_data = cx.array_buffer(result.size as usize)?;
            obj.set(&mut cx, "data", js_data)?;

            unsafe {
                let slice = js_data.as_mut_slice(&mut cx);
                copy(result.as_ptr(), slice.as_mut_ptr(), result.size as usize);
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
    let mut i = 2..cx.len();
    let mut unknown = Vec::<HTTP_UNKNOWN_HEADER>::new();
    let mut response = Box::new(HTTP_RESPONSE_V2 {
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
                KnownHeaders: [HTTP_KNOWN_HEADER { RawValueLength: 0, pRawValue: PCSTR::null() }; HttpHeaderResponseMaximum.0 as usize]
            },
            EntityChunkCount: 0,
            pEntityChunks: null_mut(),
        },
        ResponseInfoCount: 0,
        pResponseInfo: null_mut(),
    });

    let block = arg_at::<JsBuffer>(&mut cx, &mut i)?;
    let root = block.root(&mut cx);
    let base = &mut response.as_mut().Base;
    let opaque = arg_at::<JsBoolean>(&mut cx, &mut i)?.value(&mut cx);
    let more = arg_at::<JsBoolean>(&mut cx, &mut i)?.value(&mut cx);
    let disconnect = arg_at::<JsBoolean>(&mut cx, &mut i)?.value(&mut cx);
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

    if disconnect {
        flags |= HTTP_SEND_RESPONSE_FLAG_DISCONNECT;
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

fn http_request_send_data(mut cx: FunctionContext) -> JsResult<JsPromise> {
    let arc = JsArc::<Request>::import(&mut cx, 0)?;
    let id = **cx.argument::<JsBox<u64>>(1)?;
    let mut i = 2..cx.len();
    let mut count = arg_at::<JsNumber>(&mut cx, &mut i)?.value(&mut cx) as u16;
    let mut chunks = Vec::<HTTP_DATA_CHUNK>::new();
    let mut unknown = Vec::<HTTP_UNKNOWN_HEADER>::new();
    let mut roots = Vec::<Root<JsBuffer>>::new();
    while count > 0 {
        let mut block = arg_at::<JsBuffer>(&mut cx, &mut i)?;
        roots.push(block.root(&mut cx));

        let slice = block.as_mut_slice(&mut cx);
        if slice.len() > 0 {
            chunks.push(HTTP_DATA_CHUNK {
                DataChunkType: HttpDataChunkFromMemory,
                Anonymous: HTTP_DATA_CHUNK_0 {
                    FromMemory: HTTP_DATA_CHUNK_0_3 {
                        BufferLength: slice.len() as u32,
                        pBuffer: slice.as_mut_ptr() as *mut c_void
                    }
                }
            });            
        }

        count -= 1;
    }

    let block = arg_at::<JsBuffer>(&mut cx, &mut i)?;
    roots.push(block.root(&mut cx));

    let opaque = arg_at::<JsBoolean>(&mut cx, &mut i)?.value(&mut cx);
    let more = arg_at::<JsBoolean>(&mut cx, &mut i)?.value(&mut cx);
    let disconnect = arg_at::<JsBoolean>(&mut cx, &mut i)?.value(&mut cx);
    while !i.is_empty() {
        let name = arg_ptr_at(&mut cx, &block, &mut i)?;
        let value = arg_ptr_at(&mut cx, &block, &mut i)?;
        unknown.push(HTTP_UNKNOWN_HEADER {
            NameLength: name.1 as u16,
            pName: PCSTR(name.0),
            RawValueLength: value.1 as u16,
            pRawValue: PCSTR(value.0)
        });
    }

    if unknown.len() > 0 {
        chunks.push(HTTP_DATA_CHUNK {
            DataChunkType: HttpDataChunkTrailers,
            Anonymous: HTTP_DATA_CHUNK_0 {
                Trailers: HTTP_DATA_CHUNK_0_4 {
                    TrailerCount: unknown.len() as u16,
                    pTrailers: unknown.as_mut_ptr()
                }
            }
        });
    }

    let mut flags = 0;
    if opaque {
        flags |= HTTP_SEND_RESPONSE_FLAG_OPAQUE;
    }

    if more {
        flags |= HTTP_SEND_RESPONSE_FLAG_MORE_DATA;
    }

    if disconnect {
        flags |= HTTP_SEND_RESPONSE_FLAG_DISCONNECT;
    }

    let count = chunks.len() as u16;
    let source = SendRef(chunks.as_mut_ptr());
    let transfer = SendRef((roots, unknown, chunks));
    let tx = cx.channel();
    let (def, promise) = cx.promise();
    let func = async move {
        let result = arc.send_data(id, flags, source, count).await;
        drop(transfer);

        def.settle_with(&tx, move |mut cx| {
            Ok(cx.number(result.err))
        });
    };

    tasks().spawn_ok(func);
    Ok(promise)
}

fn http_request_push(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    let arc = JsArc::<Request>::import(&mut cx, 0)?;
    let id = **cx.argument::<JsBox<u64>>(1)?;
    let mut i = 2..cx.len();
    let mut unknown = Vec::<HTTP_UNKNOWN_HEADER>::new();
    let mut headers = Box::new(HTTP_REQUEST_HEADERS {
        UnknownHeaderCount: 0,
        pUnknownHeaders: null_mut(),
        TrailerCount: 0,
        pTrailers: null_mut(),
        KnownHeaders: [HTTP_KNOWN_HEADER { RawValueLength: 0, pRawValue: PCSTR::null() }; HttpHeaderRequestMaximum.0 as usize]
    });

    let block = arg_at::<JsBuffer>(&mut cx, &mut i)?;
    let base = headers.as_mut();
    let verb = arg_at::<JsNumber>(&mut cx, &mut i)?.value(&mut cx) as i32;
    let path = arg_ptr_at(&mut cx, &block, &mut i)?;
    let query = arg_ptr_at(&mut cx, &block, &mut i)?;
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

            base.UnknownHeaderCount = unknown.len() as u16;
            base.pUnknownHeaders = unknown.as_mut_ptr();            
        } else {
            base.KnownHeaders[id as usize] = HTTP_KNOWN_HEADER {
                RawValueLength: value.1 as u16,
                pRawValue: PCSTR(value.0)
            };
        }
    }

    match arc.push(id, verb, path.0 as *const u16, query.0, headers.as_ref()) {
        Ok(_) => Ok(cx.undefined()),
        Err(err) => cx.throw_type_error(format!("{}", err))
    }
}

fn http_request_close(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    let arc = JsArc::<Request>::import(&mut cx, 0)?;
    arc.close();

    Ok(cx.undefined())
}

fn http_init(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    let config = cx.argument::<JsBoolean>(0)?.value(&mut cx);
    let server = cx.argument::<JsBoolean>(1)?.value(&mut cx);
    let mut flags = 0u32;
    if config {
        flags |= HTTP_INITIALIZE_CONFIG.0;
    }

    if server {
        flags |= HTTP_INITIALIZE_SERVER.0;
    }

    unsafe  {
        let err = HttpInitialize(ver_init, HTTP_INITIALIZE(flags), None);
        match err {
            0 => Ok(cx.undefined()),
            _ => cx.throw_type_error(format!("HttpInitialize: Win32_Error = {}", err))
        }
    }
}

fn http_request_create(mut cx: FunctionContext) -> JsArcResult<Request> {
    let name = cx.argument::<JsString>(0)?.value(&mut cx);
    unsafe {
        let flags = HTTP_CREATE_REQUEST_QUEUE_FLAG_OPEN_EXISTING;
        let name_wide = wide(&name);
        let name_ptr = wide_ptr(&name_wide);

        let mut queue = HANDLE(-1);
        let err = HttpCreateRequestQueue(ver_init, name_ptr, null_mut(), flags, &mut queue);
        if err != 0 {
            return cx.throw_type_error(format!("HttpCreateRequestQueue: Win32_Error = {}", err));
        }

        if !bind_io(queue) {
            CloseHandle(queue);
            let err = GetLastError().0;
            return cx.throw_type_error(format!("BindIoCompletionCallback: Win32_Error = {}", err));
        }

        let request = Request::new(queue);
        JsArc::export(&mut cx, request)
    }
}

pub fn http_bind(cx: &mut ModuleContext) -> NeonResult<()> {
    cx.export_function("http_init", http_init)?;

    cx.export_function("http_session_create", http_session_create)?;
    cx.export_function("http_session_listen", http_session_listen)?;
    cx.export_function("http_session_close", http_session_close)?;

    cx.export_function("http_request_create", http_request_create)?;
    cx.export_function("http_request_cancel", http_request_cancel)?;
    cx.export_function("http_request_receive", http_request_receive)?;
    cx.export_function("http_request_receive_data", http_request_receive_data)?;
    cx.export_function("http_request_send", http_request_send)?;
    cx.export_function("http_request_send_data", http_request_send_data)?;
    cx.export_function("http_request_push", http_request_push)?;
    cx.export_function("http_request_close", http_request_close)?;

    Ok(())
}
