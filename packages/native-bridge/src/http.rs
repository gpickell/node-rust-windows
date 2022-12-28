use super::support::*;
use crate::user::user_groups_internal;
use super::win32::*;

use neon::prelude::*;
use neon::types::buffer::*;
use windows::Win32::Networking::WinSock::AF_INET;
use windows::Win32::Networking::WinSock::AF_INET6;
use windows::Win32::Networking::WinSock::SOCKADDR_IN;
use windows::Win32::Networking::WinSock::SOCKADDR_IN6;

use core::ptr::*;
use std::cell::RefCell;

use windows::core::PCSTR;
use windows::core::PCWSTR;

use windows::Win32::Foundation::*;
use windows::Win32::Networking::HttpServer::*;
use windows::Win32::Security::Authentication::Identity::*;

use std::ffi::*;
use std::mem::size_of;
use std::slice::from_raw_parts;
use std::slice::from_raw_parts_mut;
use std::sync::Arc;

#[allow(non_upper_case_globals)]
static ver_init: HTTPAPI_VERSION = HTTPAPI_VERSION {
    HttpApiMajorVersion: 2,
    HttpApiMinorVersion: 0,
};

fn find_user_token(req: &HTTP_REQUEST_V2) -> Option<Arc<HandleRef>> {
    unsafe {
        let slice = from_raw_parts(req.pRequestInfo, req.RequestInfoCount  as usize);    
        for info in slice {
            if info.InfoType == HttpRequestInfoTypeAuth {
                let auth = &*(info.pInfo as *const HTTP_REQUEST_AUTH_INFO);
                FreeContextBuffer(auth.PackedContext).ok();
                
                return Some(HandleRef::new(auth.AccessToken));
            }
        }
    }
    
    None
}

struct Session {
    queue: HANDLE,
    session: u64,
    urls: u64,
}

impl Finalize for Session {}

impl Session {
    pub fn create(name: &str) -> Result<Session, (&str, u32)> {
        unsafe {
            let flags = HTTP_CREATE_REQUEST_QUEUE_FLAG_CONTROLLER;
            let name_wide = wide(name);
            let name_ptr = wide_ptr(&name_wide);

            let err = HttpInitialize(ver_init, HTTP_INITIALIZE_SERVER, None);
            if err != 0 {
                return Err(("HttpInitialize", err));
            }

            let mut session: u64 = 0;
            let err = HttpCreateServerSession(ver_init, &mut session, 0);
            if err != 0 {
                return Err(("HttpCreateServerSession", err));
            }

            let mut urls: u64 = 0;
            let err = HttpCreateUrlGroup(session, &mut urls, 0);
            if err != 0 {
                HttpCloseServerSession(session);
                return Err(("HttpCreateUrlGroup", err));
            }

            let mut queue = HANDLE(-1);
            let err = HttpCreateRequestQueue(ver_init, name_ptr, null_mut(), flags, &mut queue);
            if err != 0 {
                HttpCloseServerSession(session);
                HttpCloseUrlGroup(urls);
                return Err(("HttpCreateRequestQueue", err));
            }

            let prop = HttpServerBindingProperty;
            let info = HTTP_BINDING_INFO {
                Flags: HTTP_PROPERTY_FLAGS {
                    _bitfield: 1
                },
                RequestQueueHandle: queue
            };

            let size = size_of::<HTTP_BINDING_INFO>() as u32;
            let err = HttpSetUrlGroupProperty(urls, prop, &info as *const HTTP_BINDING_INFO as *const c_void, size);
            if err != 0 {
                HttpCloseUrlGroup(urls);
                HttpCloseServerSession(session);
                CloseHandle(queue);
                return Err(("HttpSetUrlGroupProperty", err));
            }
   
            Ok(Self { queue, session, urls })
        }
    }

    pub fn config(self: &Arc<Self>, vec: Vec<String>) -> Result<(), (&'static str, u32)> {
        unsafe {
            let mut auth = false;
            let mut auth_ex = false;
            let mut auth_config = HTTP_SERVER_AUTHENTICATION_INFO::default();
            for flag in vec.iter() {
                match &flag[..] {
                    "auth" => auth = true,
                    "auth-extended" => auth_ex = true,
                    "ntlm" => auth_config.AuthSchemes |= HTTP_AUTH_ENABLE_NTLM,
                    "negotiate" => auth_config.AuthSchemes |= HTTP_AUTH_ENABLE_NEGOTIATE,
                    "kerberos" => auth_config.AuthSchemes |= HTTP_AUTH_ENABLE_KERBEROS,
                    "cache-credentials" => auth_config.ExFlags |= HTTP_AUTH_EX_FLAG_ENABLE_KERBEROS_CREDENTIAL_CACHING as u8,
                    "capture-credentials" => auth_config.ExFlags |= HTTP_AUTH_EX_FLAG_CAPTURE_CREDENTIAL as u8,
                    _ => (),
                }
            }

            auth_config.Flags = HTTP_PROPERTY_FLAGS {
                _bitfield: 1
            };

            auth_config.ReceiveContextHandle = BOOLEAN(1);

            if auth {
                let ptr = &auth_config as *const HTTP_SERVER_AUTHENTICATION_INFO as *const c_void;
                let size = size_of::<HTTP_SERVER_AUTHENTICATION_INFO>();
                let err = HttpSetServerSessionProperty(self.session, HttpServerAuthenticationProperty, ptr, size as u32);
                if err != 0 {
                    return Err(("HttpSetServerSessionProperty", err))
                }
            }

            if auth_ex {
                let ptr = &auth_config as *const HTTP_SERVER_AUTHENTICATION_INFO as *const c_void;
                let size = size_of::<HTTP_SERVER_AUTHENTICATION_INFO>();
                let err = HttpSetServerSessionProperty(self.session, HttpServerExtendedAuthenticationProperty, ptr, size as u32);
                if err != 0 {
                    return Err(("HttpSetServerSessionProperty", err))
                }
            }

            Ok(())
        }
    }

    pub fn listen(self: &Arc<Self>, url: &str) -> Result<(), (&str, u32)> {
        unsafe {
            let url_wide = wide(url);
            let err = HttpAddUrlToUrlGroup(self.urls, wide_ptr(&url_wide), 0, 0);
            if err != 0 {
                return Err(("HttpAddUrlToUrlGroup", err));
            }
    
            Ok(())
        }
    }
    
    pub fn release(self: &Arc<Self>, url: &str) -> Result<(), (&str, u32)> {
        unsafe {
            let url_wide = wide(url);
            let mut url_wide_ptr = wide_ptr(&url_wide);
            let mut flags = 0;
            if url == "all" {
                url_wide_ptr = PCWSTR::null();
                flags = HTTP_URL_FLAG_REMOVE_ALL;
            }
            
            let err = HttpRemoveUrlFromUrlGroup(self.urls, url_wide_ptr, flags);
            if err != 0 {
                return Err(("HttpAddUrlToUrlGroup", err));
            }
    
            Ok(())
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            HttpCloseUrlGroup(self.urls);
            HttpCloseServerSession(self.session);
            CloseHandle(self.queue);
        }
    }
}

struct Request {
    arc: Arc<HandleRef>,
}

impl Finalize for Request {}

impl Request {
    pub fn create(name: &str) -> Result<Self, (&str, u32)> {
        unsafe {
            let name_wide = wide(&name);
            let name_ptr = wide_ptr(&name_wide);

            let err = HttpInitialize(ver_init, HTTP_INITIALIZE_SERVER, None);
            if err != 0 {
                return Err(("HttpInitialize", err));
            }

            let mut queue = HANDLE(-1);
            let flags = HTTP_CREATE_REQUEST_QUEUE_FLAG_OPEN_EXISTING;
            let err = HttpCreateRequestQueue(ver_init, name_ptr, null_mut(), flags, &mut queue);
            if err != 0 {
                return Err(("HttpCreateRequestQueue", err));
            }

            if !bind_io(queue) {
                CloseHandle(queue);
                let err = GetLastError().0;
                return Err(("BindIoCompletionCallback", err));                
            }

            Ok(Self { arc: HandleRef::new(queue) })
        }
    }

    pub fn cancel<F>(self: &Arc<Self>, id: u64, f: F) where F: FnOnce(u32) + Send + 'static {
        unsafe {
            let h = &self.arc;
            let o = h.wrap(move |err, _| f(err));
            let err = HttpCancelHttpRequest(h.0, id, o);
            h.cleanup(o, err);
        }
    }

    pub fn receive<F>(self: &Arc<Self>, mut size: u32, f: F) where F: FnOnce(u32, Vec<u8>, &'static SendRef<HTTP_REQUEST_V2>) + Send + 'static {
        unsafe {
            if size < 1 {
                size = 4096;
            }

            if size < 2048 { 
                size = 2048;
            }

            let h0 = &self.arc;
            let h1 = h0.clone();
            let vec = Vec::<u8>::with_capacity(size as usize);
            let ptr = vec.as_ptr() as *mut HTTP_REQUEST_V2;
            let flags = HTTP_RECEIVE_HTTP_REQUEST_FLAGS(0);
            let o = h0.wrap(move |err, size| {
                let result = &*(vec.as_ptr() as *const SendRef<HTTP_REQUEST_V2>);
                if err == ERROR_MORE_DATA.0 {
                    // Make sure we close the handle
                    find_user_token(&result.0);

                    let id = result.0.Base.RequestId;
                    let vec = Vec::<u8>::with_capacity(size as usize);
                    let ptr = vec.as_ptr() as *mut HTTP_REQUEST_V2;
                    let o = h1.wrap(move |err, _| {
                        let result = &*(vec.as_ptr() as *const SendRef<HTTP_REQUEST_V2>);
                        f(err, vec, result);
                    });

                    let err = HttpReceiveHttpRequest(h1.0, id, flags, ptr, size, None, o);
                    h1.cleanup(o, err);
                } else {
                    f(err, vec, result);
                }
            });

            let err = HttpReceiveHttpRequest(h0.0, 0, flags, ptr, size, None, o);
            h0.cleanup(o, err);
        }
    }

    pub fn receive_data<F>(self: &Arc<Self>, id: u64, slice: &mut [u8], f: F) where F: FnOnce(u32, u32) + Send + 'static {
        unsafe {
            let h = &self.arc;
            let o = h.wrap(f);
            let err = HttpReceiveRequestEntityBody(h.0, id, 0, slice.as_mut_ptr() as *mut c_void, slice.len() as u32, None, o);
            h.cleanup(o, err);
        }
    }

    pub fn send<F>(self: &Arc<Self>, id: u64, flags: u32, response: &mut HTTP_RESPONSE_V2, f: F) where F: FnOnce(u32, u32) + Send + 'static {
        unsafe {
            let h = &self.arc;
            let mut size = Box::new(0u32);
            let size_ptr = size.as_mut() as *mut u32;
            let o = h.wrap(move |err, _| f(err, *size));
            let err = HttpSendHttpResponse(h.0, id, flags, response, null_mut(), size_ptr, None, 0, o, null_mut());
            h.cleanup(o, err);
        }
    }

    pub fn send_data<F>(self: &Arc<Self>, id: u64, flags: u32, chunks: &mut [HTTP_DATA_CHUNK], f: F) where F: FnOnce(u32, u32) + Send + 'static {
        unsafe {
            let h = &self.arc;
            let mut size = Box::new(0u32);
            let size_ptr = size.as_mut() as *mut u32;
            let o = h.wrap(move |err, _| f(err, *size));
            let err = HttpSendResponseEntityBody(h.0, id, flags, Some(chunks), size_ptr, None, 0, o, null_mut());
            h.cleanup(o, err);
        }
    }

    pub fn push(&self, id: u64, verb: i32, path: *const u16, query: *const u8, headers: *const HTTP_REQUEST_HEADERS ) -> Result<(), (&str, u32)> {
        unsafe {
            let arc = self.arc.clone();
            let mut query_opt: Option<PCSTR> = None;
            let tail = query.add(1);
            if *tail != 0 {
                query_opt = Some(PCSTR(tail))
            }

            let err = HttpDeclarePush(arc.0, id, HTTP_VERB(verb), PCWSTR(path), query_opt, Some(headers));
            if err != 0 {
                return Err(("HttpDeclarePush", err));
            }

            Ok(())
        }
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        self.arc.cancel();
    }
}

fn http_session_create(mut cx: FunctionContext) -> JsResult<JsValue> {
    let mut i = 0;
    let name = cx.arg_string(&mut i)?;
    match Session::create(&name) {
        Ok(session) => Ok(cx.export(session)),
        Err((hint, err)) => cx.throw_type_error(format!("{}: {}", hint, err))
    }
}

fn http_session_config(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    let mut i = 0;
    let arc = cx.import::<Session>(&mut i)?;
    let mut flags = Vec::<String>::new();
    while i < cx.len() {
        let flag = cx.arg_string(&mut i)?;
        flags.push(flag);
    }
    
    match arc.config(flags) {
        Ok(()) => Ok(cx.undefined()),
        Err((hint, err)) => cx.throw_type_error(format!("{}: {}", hint, err))
    } 
}

fn http_session_listen(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    let mut i = 0;
    let arc = cx.import::<Session>(&mut i)?;
    let url = cx.arg_string(&mut i)?;
    match arc.listen(&url) {
        Ok(()) => Ok(cx.undefined()),
        Err((hint, err)) => cx.throw_type_error(format!("{}: {}", hint, err))
    } 
}

fn http_session_release(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    let mut i = 0;
    let arc = cx.import::<Session>(&mut i)?;
    let url = cx.arg_string(&mut i)?;
    match arc.release(&url) {
        Ok(()) => Ok(cx.undefined()),
        Err((hint, err)) => cx.throw_type_error(format!("{}: {}", hint, err))
    } 
}

fn http_session_close(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    cx.dispose::<Session>(0)?;
    Ok(cx.undefined())
}

fn http_request_cancel(mut cx: FunctionContext) -> JsResult<JsPromise> {
    let mut i = 0;
    let arc = cx.import::<Request>(&mut i)?;
    let id = cx.arg_u64(&mut i)?;
    let tx = cx.channel();
    let (def, promise) = cx.promise();
    arc.cancel(id, move |err| {
        def.settle_with(&tx, move |mut cx| {
            Ok(cx.number(err))
        });
    });

    Ok(promise)
}

fn http_request_receive(mut cx: FunctionContext) -> JsResult<JsPromise> {
    let mut i = 0;
    let arc = cx.import::<Request>(&mut i)?;
    let size = cx.arg_u32(&mut i)?;
    let tx = cx.channel();
    let (def, promise) = cx.promise();
    arc.receive(size, move |err, vec, result| {
        let mut user_opt: Option<Arc<HandleRef>> = None;
        if err == 0 || err == ERROR_MORE_DATA.0 {
            user_opt = find_user_token(&result.0);
        }

        def.settle_with(&tx, move |mut cx| {
            let info = &result.0.Base;
            let obj = cx.empty_object();
            let js_err = cx.number(err);
            obj.set(&mut cx, "code", js_err)?;

            if err != 0 && err != ERROR_MORE_DATA.0 {
                return Ok(obj);
            }

            let js_id = cx.boxed(info.RequestId);
            obj.set(&mut cx, "id", js_id)?;

            if err != 0 {
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

                let mut i = 0;
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
                                let js_key = cx.string(key);
                                js_unknown.set(&mut cx, i, js_key)?;
                                i += 1;

                                let js_value = cx.string(value);
                                js_unknown.set(&mut cx, i, js_value)?;
                                i += 1;
                            }
                        }
                    }
                }

                let addr = &*info.Address.pLocalAddress;
                let addr_ptr = info.Address.pLocalAddress as *const u8;
                if addr.sa_family == AF_INET.0 as u16 {
                    let size = size_of::<SOCKADDR_IN>();
                    let mut js_addr = cx.buffer(size)?;
                    copy(addr_ptr, js_addr.as_mut_slice(&mut cx).as_mut_ptr(), size);
                    obj.set(&mut cx, "sockaddr", js_addr)?;
                }

                if addr.sa_family == AF_INET6.0 as u16 {
                    let size = size_of::<SOCKADDR_IN6>();
                    let mut js_addr = cx.buffer(size)?;
                    copy(addr_ptr, js_addr.as_mut_slice(&mut cx).as_mut_ptr(), size);
                    obj.set(&mut cx, "sockaddr", js_addr)?;
                }
            }

            if let Some(user) = user_opt {
                let js_user_sid = user_groups_internal(&mut cx, user.0, true)?;
                let js_user = cx.boxed(RefCell::new(Some(user)));
                obj.set(&mut cx, "user", js_user)?;
                obj.set(&mut cx, "user_sid", js_user_sid)?;
            }

            drop(vec);
            Ok(obj)
        });
    });
    
    Ok(promise)
}

fn http_request_receive_data(mut cx: FunctionContext) -> JsResult<JsPromise> {
    let mut i = 0;
    let arc = cx.import::<Request>(&mut i)?;
    let id = cx.arg_u64(&mut i)?;
    let mut buf = cx.arg_buffer(&mut i)?;
    let root = buf.root(&mut cx);
    let tx = cx.channel();
    let (def, promise) = cx.promise();
    let slice = buf.as_mut_slice(&mut cx);
    arc.receive_data(id, slice, move |err, size| {
        drop(root);
        
        def.settle_with(&tx, move |mut cx| {
            let obj = cx.empty_object();
            let js_err = cx.number(err);
            obj.set(&mut cx, "code", js_err)?;

            let js_size = cx.number(size);
            obj.set(&mut cx, "size", js_size)?;

            let js_eof = cx.boolean(err == ERROR_HANDLE_EOF.0);
            obj.set(&mut cx, "eof", js_eof)?;

            Ok(obj)
        });
    });

    Ok(promise)
}

fn http_request_send(mut cx: FunctionContext) -> JsResult<JsPromise> {
    let mut i = 0;
    let arc = cx.import::<Request>(&mut i)?;
    let id = cx.arg_u64(&mut i)?;
    let mut infos = Vec::<HTTP_RESPONSE_INFO>::new(); 
    let mut known = Vec::<HTTP_KNOWN_HEADER>::new();
    let mut multiple = Vec::<HTTP_MULTIPLE_KNOWN_HEADERS>::new();
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

    let block = cx.arg_buffer(&mut i)?;
    let root = block.root(&mut cx);
    let base = &mut response.as_mut().Base;
    let opaque = cx.arg_bool(&mut i)?;
    let more = cx.arg_bool(&mut i)?;
    let disconnect = cx.arg_bool(&mut i)?;
    let status = cx.arg_u16(&mut i)?;
    let major = cx.arg_u16(&mut i)?;
    let minor = cx.arg_u16(&mut i)?;
    base.StatusCode = status;
    base.Version = HTTP_VERSION {
        MajorVersion: major,
        MinorVersion: minor,
    };

    let reason = cx.arg_ptr(&mut i, &block)?;
    base.ReasonLength = reason.1 as u16;
    base.pReason = PCSTR(reason.0);

    while i < cx.len() {
        let id = cx.arg_i32(&mut i)?;
        let value = cx.arg_ptr(&mut i, &block)?;
        if id < 0 {
            let name = cx.arg_ptr(&mut i, &block)?;
            unknown.push(HTTP_UNKNOWN_HEADER {
                NameLength: name.1 as u16,
                pName: PCSTR(name.0),
                RawValueLength: value.1 as u16,
                pRawValue: PCSTR(value.0)
            });
        } else {
            let mut assign = true;
            let first = &mut base.Headers.KnownHeaders[id as usize];
            let next = HTTP_KNOWN_HEADER {
                RawValueLength: value.1 as u16,
                pRawValue: PCSTR(value.0)    
            };
            
            if let Some(last) = multiple.last_mut() {
                if last.HeaderId.0 == id {
                    assign = false;
                    known.push(next);
                    last.KnownHeaderCount += 1;
                }                
            }

            if assign {
                if first.RawValueLength > 0 {
                    let mut flags = 0;
                    if id == HttpHeaderWwwAuthenticate.0 {
                        flags |= HTTP_RESPONSE_INFO_FLAGS_PRESERVE_ORDER;
                    }

                    known.push(*first);
                    known.push(next);
                    multiple.push(HTTP_MULTIPLE_KNOWN_HEADERS {
                        HeaderId: HTTP_HEADER_ID(id),
                        Flags: flags,
                        KnownHeaderCount: 2,
                        KnownHeaders: null_mut()
                    });

                    *first = HTTP_KNOWN_HEADER::default();
                } else {
                    *first = next;
                }
            }
        }
    }

    if unknown.len() > 0 {
        base.Headers.UnknownHeaderCount = unknown.len() as u16;
        base.Headers.pUnknownHeaders = unknown.as_mut_ptr();            
    }

    if multiple.len() > 0 {
        let mut slice = known.as_mut_slice();
        for i in multiple.iter_mut() {
            let count = i.KnownHeaderCount as usize;
            i.KnownHeaders = slice.as_mut_ptr();
            slice = &mut slice[0..count];

            infos.push(HTTP_RESPONSE_INFO {
                Type: HttpResponseInfoTypeMultipleKnownHeaders,
                Length: size_of::<HTTP_MULTIPLE_KNOWN_HEADERS>() as u32,
                pInfo: i as *mut HTTP_MULTIPLE_KNOWN_HEADERS as *mut c_void
            });
        }
    }

    if infos.len() > 0 {
        response.ResponseInfoCount = infos.len() as u16;
        response.pResponseInfo = infos.as_mut_ptr();  
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
    
    let ptr = response.as_mut() as *mut HTTP_RESPONSE_V2;
    let transfer = SendRef((root, response, infos, multiple, known, unknown));
    let tx = cx.channel();
    let (def, promise) = cx.promise();
    arc.send(id, flags, unsafe { &mut *ptr }, move |err, size| {
        drop(transfer);

        def.settle_with(&tx, move |mut cx| {
            let obj = cx.empty_object();
            let js_err = cx.number(err);
            obj.set(&mut cx, "code", js_err)?;

            let js_size = cx.number(size);
            obj.set(&mut cx, "size", js_size)?;

            let js_eof = cx.boolean(err == ERROR_HANDLE_EOF.0);
            obj.set(&mut cx, "eof", js_eof)?;

            Ok(obj)
        });
    });

    Ok(promise)
}

fn http_request_send_data(mut cx: FunctionContext) -> JsResult<JsPromise> {
    let mut i = 0;
    let arc = cx.import::<Request>(&mut i)?;
    let id = cx.arg_u64(&mut i)?;
    let mut count = cx.arg_u16(&mut i)?;
    let mut chunks = Vec::<HTTP_DATA_CHUNK>::new();
    let mut unknown = Vec::<HTTP_UNKNOWN_HEADER>::new();
    let mut roots = Vec::<Root<JsBuffer>>::new();
    while count > 0 {
        let mut block = cx.arg_buffer(&mut i)?;
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

    let block = cx.arg_buffer(&mut i)?;
    roots.push(block.root(&mut cx));

    let opaque = cx.arg_bool(&mut i)?;
    let more = cx.arg_bool(&mut i)?;
    let disconnect = cx.arg_bool(&mut i)?;
    while i < cx.len() {
        let name = cx.arg_ptr(&mut i, &block)?;
        let value = cx.arg_ptr(&mut i, &block)?;
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

    let ptr = chunks.as_mut_ptr();
    let count = chunks.len();
    let slice = unsafe { from_raw_parts_mut(ptr, count) };
    let transfer = SendRef((chunks, roots, unknown));
    let tx = cx.channel();
    let (def, promise) = cx.promise();
    arc.send_data(id, flags, slice, move |err, size|  {
        drop(transfer);

        def.settle_with(&tx, move |mut cx| {
            let obj = cx.empty_object();
            let js_err = cx.number(err);
            obj.set(&mut cx, "code", js_err)?;

            let js_size = cx.number(size);
            obj.set(&mut cx, "size", js_size)?;

            Ok(obj)
        });
    });

    Ok(promise)
}

fn http_request_push(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    let mut i = 0;
    let arc = cx.import::<Request>(&mut i)?;
    let id = cx.arg_u64(&mut i)?;
    let mut unknown = Vec::<HTTP_UNKNOWN_HEADER>::new();
    let mut headers = Box::new(HTTP_REQUEST_HEADERS {
        UnknownHeaderCount: 0,
        pUnknownHeaders: null_mut(),
        TrailerCount: 0,
        pTrailers: null_mut(),
        KnownHeaders: [HTTP_KNOWN_HEADER { RawValueLength: 0, pRawValue: PCSTR::null() }; HttpHeaderRequestMaximum.0 as usize]
    });

    let block = cx.arg_buffer(&mut i)?;
    let base = headers.as_mut();
    let verb = cx.arg_i32(&mut i)?;
    let path = cx.arg_ptr(&mut i, &block)?;
    let query = cx.arg_ptr(&mut i, &block)?;
    while i < cx.len() {
        let id = cx.arg_i32(&mut i)?;
        let value = cx.arg_ptr(&mut i, &block)?;
        if id < 0 {
            let name = cx.arg_ptr(&mut i, &block)?;
            unknown.push(HTTP_UNKNOWN_HEADER {
                NameLength: name.1 as u16,
                pName: PCSTR(name.0),
                RawValueLength: value.1 as u16,
                pRawValue: PCSTR(value.0)
            });
        } else {
            base.KnownHeaders[id as usize] = HTTP_KNOWN_HEADER {
                RawValueLength: value.1 as u16,
                pRawValue: PCSTR(value.0)
            };
        }
    }

    if unknown.len() > 0 {
        base.UnknownHeaderCount = unknown.len() as u16;
        base.pUnknownHeaders = unknown.as_mut_ptr();
    }

    match arc.push(id, verb, path.0 as *const u16, query.0, headers.as_ref()) {
        Ok(_) => Ok(cx.undefined()),
        Err((hint, err)) => cx.throw_type_error(format!("{}: {}", hint, err))
    }
}

fn http_request_close(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    cx.dispose::<Request>(0)?;
    Ok(cx.undefined())
}

fn http_request_create(mut cx: FunctionContext) -> JsResult<JsValue> {
    let mut i = 0;
    let name = cx.arg_string(&mut i)?;
    match Request::create(&name) {
        Ok(request) => Ok(cx.export(request)),
        Err((hint, err)) => cx.throw_type_error(format!("{}: {}", hint, err))
    }
}

pub fn http_bind(cx: &mut ModuleContext) -> NeonResult<()> {
    cx.export_function("http_session_create", http_session_create)?;
    cx.export_function("http_session_config", http_session_config)?;
    cx.export_function("http_session_listen", http_session_listen)?;
    cx.export_function("http_session_release", http_session_release)?;
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
