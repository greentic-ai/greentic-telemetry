use crate::context::TelemetryCtx;
use std::{cell::RefCell, future::Future};

tokio::task_local! {
    static GT_TELEMETRY_CTX: RefCell<Option<TelemetryCtx>>;
}

/// Set the task-local telemetry context. No-op if outside a Tokio task.
pub fn set_current_telemetry_ctx(ctx: TelemetryCtx) {
    let _ = GT_TELEMETRY_CTX.try_with(|slot| {
        *slot.borrow_mut() = Some(ctx);
    });
}

/// Execute `f` with the telemetry context currently stored on this task, if any.
pub fn with_current_telemetry_ctx<R>(f: impl FnOnce(Option<&TelemetryCtx>) -> R) -> R {
    let mut f = Some(f);
    let result = GT_TELEMETRY_CTX.try_with(|slot| {
        let guard = slot.borrow();
        let func = f
            .take()
            .expect("telemetry context closure already consumed");
        func(guard.as_ref())
    });

    match result {
        Ok(value) => value,
        Err(_) => {
            let func = f
                .take()
                .expect("telemetry context closure already consumed");
            func(None)
        }
    }
}

/// Run `fut` with a task-local telemetry context slot initialized.
pub async fn with_task_local<Fut, R>(fut: Fut) -> R
where
    Fut: Future<Output = R>,
{
    GT_TELEMETRY_CTX.scope(RefCell::new(None), fut).await
}
