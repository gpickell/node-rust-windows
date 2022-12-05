mod support;
mod http;
mod win32;

use neon::prelude::*;

use std::ffi::OsString;
use std::sync::Mutex;
use std::time::Duration;
use std::thread::spawn;

use windows_service::*;
use windows_service::service::*;
use windows_service::service_control_handler::*;

use support::*;
use http::*;

#[allow(non_upper_case_globals)]
static callbacks: CallbackList = CallbackList::new();

#[allow(non_upper_case_globals)]
static gate_init: Event = Event::new();

#[allow(non_upper_case_globals)]
static gate_start: Event = Event::new();

#[allow(non_upper_case_globals)]
static gate_stop: Event = Event::new();

#[allow(non_upper_case_globals)]
static gate_done: Event = Event::new();

#[allow(non_upper_case_globals)]
static handle: Mutex<Option<ServiceStatusHandle>> = Mutex::new(None);

#[allow(non_upper_case_globals)]
static mut accept_pause: bool = false;

#[allow(non_upper_case_globals)]
static mut service_name: String = String::new();

#[allow(non_upper_case_globals)]
static mut checkpoint: u32 = 0;

define_windows_service!(ffi_service_main, service_main);

fn service_main(_: Vec<OsString>) {
    callbacks.notify("start");

    if let Ok(mut value) = handle.lock() {
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
            *value = Some(h);
        }
    }

    gate_start.set(true);
    gate_stop.wait();
    gate_done.set(true);
}

fn report(next_state: ServiceState, exit_code: u32) -> bool {
    if let Ok(opt) = handle.lock() {
        if let Some(h) = *opt {
            let cp = unsafe {
                let tmp = checkpoint;
                checkpoint += 1;
                tmp
            };

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
                checkpoint: cp,
                wait_hint: Duration::new(30, 0),
                process_id: None                
            };
            
            return h.set_service_status(status).is_ok();    
        }
    }

    return false;
}

#[neon::main]
fn main(mut cx: ModuleContext) -> NeonResult<()> {
    http_bind(&mut cx)?;

    cx.export_function("start", start)?;
    cx.export_function("simulate", simulate)?;
    cx.export_function("shutdown", shutdown)?;

    cx.export_function("pausePending", pause_pending)?;
    cx.export_function("continuePending", continue_pending)?;
    cx.export_function("startPending", start_pending)?;
    cx.export_function("stopPending", stop_pending)?;
    cx.export_function("running", paused)?;
    cx.export_function("running", running)?;
    cx.export_function("stopped", stopped)?;

    cx.export_function("watch", watch)?;
    cx.export_function("clear", clear)?;
    cx.export_function("post", post)?;

    Ok(())
}

fn watch(mut cx: FunctionContext) -> JsResult<JsBox<usize>> {
    let cb = cx.argument::<JsFunction>(0)?;
    let ptr = callbacks.add(&mut cx, cb);
    return Ok(cx.boxed(ptr));
}

fn clear(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    if cx.len() > 0 {
        if let Ok(ptr) = cx.argument::<JsBox<usize>>(0) {
            callbacks.remove(**ptr);
        }
    } else {
        callbacks.clear();
    }

    return Ok(cx.undefined());
}

fn post(mut cx: FunctionContext) -> JsResult<JsUndefined> {
    let msg = cx.argument::<JsString>(0)?;
    callbacks.notify(&msg.value(&mut cx));

    return Ok(cx.undefined());
}

fn start(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let name = cx.argument::<JsString>(0)?;
    let flag = cx.argument::<JsBoolean>(1)?;
    if !gate_init.set(true) {
        return Ok(cx.boolean(false));
    }

    unsafe {
        accept_pause = flag.value(&mut cx);
        service_name.push_str(&name.value(&mut cx));
    }

    spawn(move || {
        let hr = service_dispatcher::start(unsafe { service_name.clone() }, ffi_service_main);
        if hr.is_err() {
            gate_start.set(false);
        }
    });

    let r = gate_start.wait();
    return Ok(cx.boolean(r));
}

fn simulate(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let name = cx.argument::<JsString>(0)?;
    let flag = cx.argument::<JsBoolean>(1)?;
    if !gate_init.set(true) {
        return Ok(cx.boolean(false));    
    }

    unsafe {
        accept_pause = flag.value(&mut cx);
        service_name.push_str(&name.value(&mut cx));
    }

    spawn(move || {
        let hr = service_dispatcher::start(unsafe { service_name.clone() }, ffi_service_main);
        if hr.is_err() {
            callbacks.notify("start");
            gate_start.set(true);
            gate_stop.wait();
            gate_done.set(true);
        }
    });

    let r = gate_start.wait();
    return Ok(cx.boolean(r));
}

fn shutdown(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    gate_init.set(false);

    if gate_init.wait() {
        gate_stop.set(true);
        gate_done.wait();

        return Ok(cx.boolean(true));
    }

    return Ok(cx.boolean(false));
}

fn continue_pending(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let r = report(ServiceState::ContinuePending, 0);
    return Ok(cx.boolean(r));
}

fn pause_pending(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let r = report(ServiceState::PausePending, 0);
    return Ok(cx.boolean(r));
}

fn start_pending(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let r = report(ServiceState::StartPending, 0);
    return Ok(cx.boolean(r));
}

fn stop_pending(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let r = report(ServiceState::StopPending, 0);
    return Ok(cx.boolean(r));
}

fn paused(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let r = report(ServiceState::Paused, 0);
    return Ok(cx.boolean(r));
}

fn running(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let r = report(ServiceState::Running, 0);
    return Ok(cx.boolean(r));
}

fn stopped(mut cx: FunctionContext) -> JsResult<JsBoolean> {
    let r = report(ServiceState::Stopped, 0);
    return Ok(cx.boolean(r));
}
