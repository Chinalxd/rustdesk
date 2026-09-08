use hbb_common::{log, ResultType};
use std::{ops::Deref, sync::Mutex};

use crate::CliprdrServiceContext;

const CLIPBOARD_RESPONSE_WAIT_TIMEOUT_SECS: u32 = 30;

lazy_static::lazy_static! {
    static ref CONTEXT_SEND: ContextSend = ContextSend::default();
}

#[derive(Default)]
pub struct ContextSend(Mutex<Option<Box<dyn CliprdrServiceContext>>>);

impl Deref for ContextSend {
    type Target = Mutex<Option<Box<dyn CliprdrServiceContext>>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl ContextSend {
    #[inline]
    pub fn is_enabled() -> bool {
        CONTEXT_SEND.lock().unwrap().is_some()
    }

    pub fn set_is_stopped() {
        let _res = Self::proc(|c| c.set_is_stopped().map_err(|e| e.into()));
    }

    pub fn enable(enabled: bool) {
        let mut lock = CONTEXT_SEND.lock().unwrap();
        if enabled {
            if lock.is_some() {
                return;
            }
            match crate::create_cliprdr_context(true, false, CLIPBOARD_RESPONSE_WAIT_TIMEOUT_SECS) {
                Ok(context) => {
                    log::info!("clipboard context for file transfer created.");
                    *lock = Some(context)
                }
                Err(err) => {
                    log::error!(
                        "create clipboard context for file transfer: {}",
                        err.to_string()
                    );
                }
            }
        } else if let Some(_clp) = lock.take() {
            *lock = None;
            log::info!("clipboard context for file transfer destroyed.");
        }
    }

    /// make sure the clipboard context is enabled.
    ///
    /// If the context exists but was stopped (e.g. by a permission change or an
    /// internal error path), try to reset it. If resetting fails, or if there is
    /// no context at all, create a fresh one. This prevents file copy-paste from
    /// silently dying after long-running sessions.
    pub fn make_sure_enabled() -> ResultType<()> {
        let mut lock = CONTEXT_SEND.lock().unwrap();
        if let Some(ctx) = lock.as_mut() {
            if ctx.is_stopped() {
                log::info!("clipboard context is stopped, resetting");
                if let Err(e) = ctx.reset() {
                    log::warn!("failed to reset clipboard context: {}, recreating", e);
                    *lock = None;
                } else {
                    return Ok(());
                }
            } else {
                return Ok(());
            }
        }

        let ctx = crate::create_cliprdr_context(true, false, CLIPBOARD_RESPONSE_WAIT_TIMEOUT_SECS)?;
        *lock = Some(ctx);
        log::info!("clipboard context for file transfer recreated.");
        Ok(())
    }

    pub fn proc<F: FnOnce(&mut Box<dyn CliprdrServiceContext>) -> ResultType<()>>(
        f: F,
    ) -> ResultType<()> {
        let mut lock = CONTEXT_SEND.lock().unwrap();
        match lock.as_mut() {
            Some(context) => f(context),
            None => Ok(()),
        }
    }
}
