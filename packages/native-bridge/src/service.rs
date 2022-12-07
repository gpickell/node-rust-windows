use neon::prelude::*;

use std::ffi::OsString;
use std::sync::{Condvar, Mutex};
use std::time::Duration;
use std::thread::{spawn, JoinHandle};

use windows_service::*;
use windows_service::service::*;
use windows_service::service_control_handler::*;

use super::support::*;

#[derive(PartialEq, PartialOrd)]
enum State {
    PreStart,
    ThreadInStartup,
    ThreadInWait,
    ThreadInExit,
}

use State::*;

#[allow(non_upper_case_globals)]
static callbacks: CallbackList = CallbackList::new();

#[allow(non_upper_case_globals)]
static state: Mutex<State> = Mutex::new(PreStart);

#[allow(non_upper_case_globals)]
static cvar: Condvar = Condvar::new();

#[allow(non_upper_case_globals)]
static handle: Mutex<Option<ServiceStatusHandle>> = Mutex::new(None);

#[allow(non_upper_case_globals)]
static mut accept_pause: bool = false;

#[allow(non_upper_case_globals)]
static mut service_name: String = String::new();

#[allow(non_upper_case_globals)]
static mut checkpoint: u32 = 0;

#[allow(non_upper_case_globals)]
static mut join_handle: Option<JoinHandle<()>> = None;

#[allow(non_upper_case_globals)]
static mut start_error: i32 = 0;

define_windows_service!(ffi_service_main, service_main_native);

fn service_main_native(_: Vec<OsString>) {
    service_main();
}

fn service_main() {
    callbacks.notify("start");

    let hr = service_control_handler::register(unsafe { service_name.clone() }, move |req| {
        match req {
            ServiceControl::Continue => {
                callbacks.notify("control-continue");
                ServiceControlHandlerResult::NoError
            },

            ServiceControl::Pause => {
                callbacks.notify("control-pause");
                ServiceControlHandlerResult::NoError
            },

            ServiceControl::Stop => {
                callbacks.notify("control-stop");
                ServiceControlHandlerResult::NoError
            },

            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    });

    if let Ok(h) = hr {
        if let Ok(mut value) = handle.lock() {
            *value = Some(h);
        }

        callbacks.notify("control-register");
    } else {
        callbacks.notify("control-idle");
    }

    let mut guard = state.lock();
    while let Ok(mut value) = guard {
        if *value > ThreadInWait {
            break;
        }

        if *value < ThreadInWait {
            *value = ThreadInWait;
            cvar.notify_all();
        }

        guard = cvar.wait(value);
    }
}

fn start(name: &str, flag_pause: bool, simulate: bool) -> (bool, i32) {
    if let Ok(mut value) = state.lock() {
        if *value != PreStart {
            return (false, 0);
        }

        unsafe {
            accept_pause = flag_pause;
            service_name.push_str(&name);
        }
        
        let h = spawn(move || {
            let mut error = 0;
            let hr = service_dispatcher::start(unsafe { service_name.clone() }, ffi_service_main);
            if let Err(e) = hr {
                if let Error::Winapi(winapi) = e {
                    if let Some(code) = winapi.raw_os_error() {
                        error = code;

                        if error == 1063 && simulate {
                            error = 0;
                            service_main();
                        }
                    }
                }
            }

            if let Ok(mut value) = state.lock() {
                unsafe {
                    *value = ThreadInExit;
                    start_error = error;                    
                }
            }
            
            cvar.notify_all();
        });

        unsafe {
            *value = ThreadInStartup;
            join_handle = Some(h);
        }
    }

    let mut guard = state.lock();
    while let Ok(value) = guard {
        if *value != ThreadInStartup {
            let error = unsafe { start_error }; 
            return (error == 0, error);
        }

        guard = cvar.wait(value);
    }

    return (false, 0);
}

fn shutdown() {
    let mut guard = state.lock();
    while let Ok(mut value) = guard {
        if *value != ThreadInStartup {
            *value = ThreadInExit;
            break;
        }

        guard = cvar.wait(value);
    }

    cvar.notify_all();
}

fn report(next_state: ServiceState, exit_code: u32) -> bool {
    if let Ok(opt) = handle.lock() {
        if let Some(h) = *opt {
            let mut code = ServiceExitCode::Win32(0);
            if exit_code > 0 {
                code = ServiceExitCode::ServiceSpecific(exit_code);
            }

            let mut accept = ServiceControlAccept::STOP;
            if unsafe { accept_pause } {
                accept |= ServiceControlAccept::PAUSE_CONTINUE;
            }

            let status = ServiceStatus {
                service_type: ServiceType::OWN_PROCESS,
                current_state: next_state,
                controls_accepted: accept,
                exit_code: code,
                checkpoint: unsafe { checkpoint },
                wait_hint: Duration::new(30, 0),
                process_id: None                
            };

            unsafe { checkpoint += 1 };
            return h.set_service_status(status).is_ok();    
        }
    }

    return false;
}

pub fn service_bind(cx: &mut ModuleContext) -> NeonResult<()> {
    cx.export_function("service_start", service_start)?;
    cx.export_function("service_simulate", service_simulate)?;
    cx.export_function("service_shutdown", service_shutdown)?;

    cx.export_function("service_pause_pending", service_pause_pending)?;
    cx.export_function("service_continue_pending", service_continue_pending)?;
    cx.export_function("service_start_pending", service_start_pending)?;
    cx.export_function("service_stop_pending", service_stop_pending)?;
    cx.export_function("service_paused", service_paused)?;
    cx.export_function("service_running", service_running)?;
    cx.export_function("service_stopped", service_stopped)?;

    cx.export_function("service_watch", service_watch)?;
    cx.export_function("service_clear", service_clear)?;
    cx.export_function("service_post", service_post)?;

    Ok(())
}

fn service_watch(mut cx: FunctionContext) -> JsResult<JsBox<usize>> {
    let cb = cx.argument::<JsFunction>(0)?;
    let ptr = callbacks.add(&mut cx, cb);
    return Ok(cx.boxed(ptr));
}

fn service_clear(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    if cx.len() > 0 {
        if let Ok(ptr) = cx.argument::<JsBox<usize>>(0) {
            callbacks.remove(**ptr);
        }
    } else {
        callbacks.clear();
    }

    return Ok(cx.undefined());
}

fn service_post(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    let msg = cx.argument::<JsString>(0)?;
    callbacks.notify(&msg.value(&mut cx));

    return Ok(cx.undefined());
}

fn service_start_with<'a>(cx: &mut FunctionContext<'a>, simulate: bool) -> JsResult<'a, JsObject> {
    let name = cx.argument::<JsString>(0)?.value(cx);
    let flag_pause = cx.argument::<JsBoolean>(1)?.value(cx);
    let (ready, error) = start(&name, flag_pause, simulate);
    let obj = cx.empty_object();
    let js_ready = cx.boolean(ready);
    let js_error = cx.number(error);
    obj.set(cx, "ready", js_ready)?;
    obj.set(cx, "error", js_error)?;
    
    Ok(obj)
}

fn service_start(mut cx: FunctionContext) -> JsResult<JsObject> {
    service_start_with(&mut cx, false)
}

fn service_simulate(mut cx: FunctionContext) -> JsResult<JsObject> {
    service_start_with(&mut cx, true)
}

fn service_shutdown(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    shutdown();
    return Ok(cx.undefined());
}

fn service_continue_pending(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let r = report(ServiceState::ContinuePending, 0);
    return Ok(cx.boolean(r));
}

fn service_pause_pending(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let r = report(ServiceState::PausePending, 0);
    return Ok(cx.boolean(r));
}

fn service_start_pending(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let r = report(ServiceState::StartPending, 0);
    return Ok(cx.boolean(r));
}

fn service_stop_pending(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let r = report(ServiceState::StopPending, 0);
    return Ok(cx.boolean(r));
}

fn service_paused(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let r = report(ServiceState::Paused, 0);
    return Ok(cx.boolean(r));
}

fn service_running(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let r = report(ServiceState::Running, 0);
    return Ok(cx.boolean(r));
}

fn service_stopped(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let r = report(ServiceState::Stopped, 0);
    return Ok(cx.boolean(r));
}
