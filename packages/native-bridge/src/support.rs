use neon::prelude::*;
use neon::types::buffer::*;

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::RwLock;

pub struct Delegate<T>(RwLock<Vec<(Arc<()>, Box<dyn Fn(T) + Send + Sync + 'static>)>>);

impl<T> Delegate<T> {
    pub const fn new() -> Self {
        Self(RwLock::new(Vec::new()))
    }

    pub fn clear(&self) {
        if let Ok(mut vec) = self.0.write() {
            vec.clear();
        }
    }

    pub fn push<D>(&self, d: D) -> Arc<()> where D: Fn(T) + Send + Sync + 'static {
        let arc = Arc::new(());
        if let Ok(mut vec) = self.0.write() {
            vec.push((arc.clone(), Box::new(d)));
        }

        return arc;
    }

    pub fn delete(&self, arc: Arc<()>) {
        if let Ok(mut vec) = self.0.write() {
            vec.retain(move |x| !Arc::ptr_eq(&x.0, &arc));
        }
    }

    pub fn send<'a, F, X>(&self, f: F) -> bool where F: Fn() -> X, X: Into<T> {
        if let Ok(vec) = self.0.read() {
            for d in vec.iter() {
                d.1(f().into());
            }

            return true;
        }

        false
    }
}

pub trait FunctionContextEx<'a> {
    fn arg_opt(&mut self, i: &mut i32) -> bool;

    fn arg_bool(&mut self, i: &mut i32) -> NeonResult<bool>;
    fn arg_buffer(&mut self, i: &mut i32) -> JsResult<'a, JsBuffer>;
    fn arg_string(&mut self, i: &mut i32) -> NeonResult<String>;

    fn arg_u16(&mut self, i: &mut i32) -> NeonResult<u16>;
    fn arg_i32(&mut self, i: &mut i32) -> NeonResult<i32>;
    fn arg_u32(&mut self, i: &mut i32) -> NeonResult<u32>;
    fn arg_u64(&mut self, i: &mut i32) -> NeonResult<u64>;
    fn arg_ptr(&mut self, i: &mut i32, block: &Handle<'a, JsBuffer>) -> NeonResult<(*const u8, usize)>;

    fn export<T: Finalize + Send + Sync + 'static>(&mut self, value: T) -> Handle<'a, JsValue>;
    fn import<T: Finalize + Send + Sync + 'static>(&mut self, i: &mut i32) -> NeonResult<Arc<T>>;
    fn dispose<T: Finalize + Send + Sync + 'static>(&mut self, i: i32) -> NeonResult<()>;
}

impl<'a> FunctionContextEx<'a> for FunctionContext<'a> {
    fn arg_opt(&mut self, i: &mut i32) -> bool {
        if let Some(value) = self.argument_opt(*i) {
            if value.is_a::<JsUndefined, _>(self) {
                *i += 1;
                return false;
            }

            return true;
        }

        false
    }

    fn arg_bool(&mut self, i: &mut i32) -> NeonResult<bool> {
        let result = self.argument::<JsBoolean>(*i)?.value(self);
        *i += 1;

        Ok(result)
    }

    fn arg_buffer(&mut self, i: &mut i32) -> JsResult<'a, JsBuffer> {
        let result = self.argument::<JsBuffer>(*i)?;
        *i += 1;

        Ok(result)
    }

    fn arg_string(&mut self, i: &mut i32) -> NeonResult<String> {
        let result = self.argument::<JsString>(*i)?.value(self);
        *i += 1;

        Ok(result)
    }

    fn arg_u16(&mut self, i: &mut i32) -> NeonResult<u16> {
        let result = self.argument::<JsNumber>(*i)?.value(self);
        *i += 1;

        Ok(result as u16)
    }

    fn arg_i32(&mut self, i: &mut i32) -> NeonResult<i32> {
        let result = self.argument::<JsNumber>(*i)?.value(self);
        *i += 1;

        Ok(result as i32)
    }

    fn arg_u32(&mut self, i: &mut i32) -> NeonResult<u32> {
        let result = self.argument::<JsNumber>(*i)?.value(self);
        *i += 1;

        Ok(result as u32)
    }

    fn arg_u64(&mut self, i: &mut i32) -> NeonResult<u64> {
        let result = self.argument::<JsBox<u64>>(*i)?;
        *i += 1;

        Ok(**result)
    }

    fn arg_ptr(&mut self, i: &mut i32, block: &Handle<'a, JsBuffer>) -> NeonResult<(*const u8, usize)> {
        let off = self.arg_u32(i)? as usize;
        let len = self.arg_u32(i)? as usize;
        let ptr = block.as_slice(self)[off..off].as_ptr();
        Ok((ptr, len))
    }

    fn export<T: Finalize + Send + Sync + 'static>(&mut self, value: T) -> Handle<'a, JsValue> {
        let result = self.boxed(RefCell::new(Some(Arc::new(value))));
        result.upcast()
    }

    fn import<T: Finalize + Send + Sync + 'static>(&mut self, i: &mut i32) -> NeonResult<Arc<T>> {
        let result = self.argument::<JsBox<RefCell<Option<Arc<T>>>>>(*i)?;
        *i += 1;

        if let Some(arc) = (**result).borrow().as_ref() {
            return Ok(arc.clone());
        }

        self.throw_type_error("Object disposed.")
    }

    fn dispose<T: Finalize + Send + Sync + 'static>(&mut self, i: i32) -> NeonResult<()> {
        let result = self.argument::<JsBox<RefCell<Option<Arc<T>>>>>(i)?;
        (**result).replace(None);

        Ok(())
    }
}
