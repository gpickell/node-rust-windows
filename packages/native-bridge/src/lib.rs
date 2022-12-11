mod support;
mod http;
mod service;
mod user;
mod win32;

use http::*;
use service::*;
use user::*;

use neon::prelude::*;

#[neon::main]
fn main(mut cx: ModuleContext) -> NeonResult<()> {
    http_bind(&mut cx)?;
    service_bind(&mut cx)?;
    user_bind(&mut cx)?;

    Ok(())
}
