use neon::prelude::*;

use super::support::*;
use super::win32::*;

use std::ffi::*;
use std::ptr::copy;
use std::slice::*;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Security::*;
use windows::Win32::Security::Authorization::*;
use windows::Win32::System::Memory::*;
use windows::Win32::System::Threading::*;

struct Resolver {
    name: Vec<u16>,
    domain: Vec<u16>
}

impl Resolver {
    fn new() -> Self {
        Self {
            name: Vec::with_capacity(24),
            domain: Vec::with_capacity(24),
        }
    }

    unsafe fn append<'a>(mut self, cx: &mut FunctionContext<'a>, list: &Handle<JsArray>, psid: PSID) -> NeonResult<()> {
        let mut vec = Vec::<u8>::with_capacity(GetLengthSid(psid) as usize);
        copy(psid.0 as *const u8, vec.as_mut_ptr(), vec.capacity());

        let builder = cx.task(move || {
            let psid = PSID(vec.as_mut_ptr() as *mut c_void);
            if let Some(value) = self.resolve(psid) {
                return value;                
            }
    
            drop(vec);
            String::from("")
        });

        let promise = builder.promise(move |mut cx, value| {
            Ok(cx.string(value))
        });

        list.set(cx, 2, promise)?;
        Ok(())
    }

    unsafe fn resolve(&mut self, sid: PSID) -> Option<String> {
        let mut tmp = SID_NAME_USE(0);
        let mut name_ptr = self.name.as_mut_ptr();
        let mut name_size = self.name.capacity() as u32;
        let mut domain_ptr = self.domain.as_mut_ptr();
        let mut domain_size = self.domain.capacity() as u32;
        let mut result = LookupAccountSidW(PCWSTR::null(), sid, PWSTR(name_ptr), &mut name_size, PWSTR(domain_ptr), &mut domain_size, &mut tmp);
        if !result.as_bool() && GetLastError() == ERROR_INSUFFICIENT_BUFFER {
            resize(&mut self.name, name_size as usize);
            resize(&mut self.domain, domain_size as usize);

            name_ptr = self.name.as_mut_ptr();
            name_size = self.name.capacity() as u32;
            domain_ptr = self.domain.as_mut_ptr();
            domain_size = self.domain.capacity() as u32;
            result = LookupAccountSidW(PCWSTR::null(), sid, PWSTR(name_ptr), &mut name_size, PWSTR(domain_ptr), &mut domain_size, &mut tmp);
        }

        if result.as_bool() {
            let name = PCWSTR(name_ptr).to_string().unwrap();
            let domain = PCWSTR(domain_ptr).to_string().unwrap();
            if domain.len() > 0 {
                return Some(format!("{}\\{}", domain, name));
            }

            return Some(name);
        }

        None
    }
}

unsafe fn resize<T>(vec: &mut Vec<T>, size: usize) -> *mut c_void {
    if vec.capacity() < size {
        *vec = Vec::with_capacity(size as usize);
    }

    return vec.as_mut_ptr() as *mut c_void;
}

unsafe fn add_type_sid<'a>(cx: &mut FunctionContext<'a>, name: Handle<'a, JsString>, psid: PSID) -> JsResult<'a, JsArray> {
    let mut value = PSTR::null();
    let list = cx.empty_array();
    let result = ConvertSidToStringSidA(psid, &mut value);
    if result.as_bool() {
        if let Ok(sid) = value.to_string() {
            list.set(cx, 0, name)?;
            
            let value = cx.string(sid);
            list.set(cx, 1, value)?;
        }

        LocalFree(value.as_ptr() as isize);
    }

    return Ok(list)
}

fn user_groups(mut cx: FunctionContext) -> JsResult<JsArray> {
    cx.export(());

    unsafe {
        let mut i = 0;
        let mut handle = HANDLE(-1);
        let method = cx.arg_string(&mut i)?;
        let resolve = cx.arg_bool(&mut i)?;
        if method.eq("viaProcess") {
            let process = GetCurrentProcess();
            let result = OpenProcessToken(process, TOKEN_QUERY, &mut handle);
            if !result.as_bool() {
                return cx.throw_type_error(format!("OpenProcessToken: {}", GetLastError().0));
            }
        }

        if method.eq("viaThread") {
            let process = GetCurrentThread();
            let result = OpenThreadToken(process, TOKEN_QUERY, true, &mut handle);
            if !result.as_bool() {
                return cx.throw_type_error(format!("OpenThreadToken: {}", GetLastError().0));
            }
        }

        if method.eq("viaUser") {
            let process = GetCurrentThread();
            let result = OpenThreadToken(process, TOKEN_QUERY, false, &mut handle);
            if !result.as_bool() {
                return cx.throw_type_error(format!("OpenThreadToken: {}", GetLastError().0));
            }
        }

        if method.eq("viaToken") {
            let arc = cx.import::<HandleRef>(&mut i)?;
            handle = (*arc).0;
        }

        let js_result = cx.empty_array();
        let js_user = cx.string("user");
        let js_group = cx.string("group");
        let js_deny_only = cx.string("deny-only-group");

        let mut i = 0;
        let mut size = 0;
        let mut buf = Vec::<u8>::new();
        let mut ptr = resize(&mut buf, size as usize);
        let mut result = GetTokenInformation(handle, TokenUser, Some(ptr), buf.capacity() as u32, &mut size);
        if !result.as_bool() && GetLastError() == ERROR_INSUFFICIENT_BUFFER {
            ptr = resize(&mut buf, size as usize);
            result = GetTokenInformation(handle, TokenUser, Some(ptr), buf.capacity() as u32, &mut size);            
        }

        if result.as_bool() {
            let user = &*(ptr as *const TOKEN_USER);
            let mut value = PSTR::null();
            let result = ConvertSidToStringSidA(user.User.Sid, &mut value);
            if result.as_bool() {
                let list = add_type_sid(&mut cx, js_user, user.User.Sid)?;
                if list.len(&mut cx) > 0 {
                    js_result.set(&mut cx, i, list)?;
                    i += 1;

                    if resolve {
                        let resolver = Resolver::new();
                        resolver.append(&mut cx, &list, user.User.Sid)?;
                    }
                }
            }
        }

        let mut result = GetTokenInformation(handle, TokenGroups, Some(ptr), buf.capacity() as u32, &mut size);
        if !result.as_bool() && GetLastError() == ERROR_INSUFFICIENT_BUFFER {
            ptr = resize(&mut buf, size as usize);
            result = GetTokenInformation(handle, TokenGroups, Some(ptr), buf.capacity() as u32, &mut size);            
        }

        if result.as_bool() {
            let groups = &*(ptr as *const TOKEN_GROUPS);
            let slice = from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize);
            for group in slice {
                if group.Attributes & 4 != 0 {
                    let list = add_type_sid(&mut cx, js_group, group.Sid)?;
                    if list.len(&mut cx) > 0 {
                        js_result.set(&mut cx, i, list)?;
                        i += 1;

                        if resolve {
                            let resolver = Resolver::new();
                            resolver.append(&mut cx, &list, group.Sid)?;
                        }
                    }
                }

                if group.Attributes & 16 != 0 {
                    let list = add_type_sid(&mut cx, js_deny_only, group.Sid)?;
                    if list.len(&mut cx) > 0 {
                        js_result.set(&mut cx, i, list)?;
                        i += 1;

                        if resolve {
                            let resolver = Resolver::new();
                            resolver.append(&mut cx, &list, group.Sid)?;
                        }
                    }
                }
            }
        }

        Ok(js_result)
    }
}

fn user_close(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    cx.dispose::<HandleRef>(0)?;
    Ok(cx.undefined())
}

pub fn user_bind(cx: &mut ModuleContext) -> NeonResult<()> {
    cx.export_function("user_groups", user_groups)?;
    cx.export_function("user_close", user_close)?;

    Ok(())
}
