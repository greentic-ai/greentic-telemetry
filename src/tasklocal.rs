use crate::context::TelemetryCtx;
use greentic_types::TenantCtx;
use std::cell::RefCell;
use std::future::Future;

tokio::task_local! {
    static GT_TENANT_CTX: RefCell<Option<TenantCtx>>;
    static GT_TELEMETRY_CTX: RefCell<Option<TelemetryCtx>>;
}

/// Run `fut` with task-local storage initialised for telemetry.
pub async fn with_task_local<Fut, T>(fut: Fut) -> T
where
    Fut: Future<Output = T>,
{
    GT_TENANT_CTX
        .scope(RefCell::new(None), async move {
            GT_TELEMETRY_CTX.scope(RefCell::new(None), fut).await
        })
        .await
}

/// Set the task-local tenant context for the current asynchronous task.
pub fn set_current_tenant_ctx(ctx: TenantCtx) {
    let tenant = ctx.clone();
    if let Ok(()) = GT_TENANT_CTX.try_with(|slot| {
        *slot.borrow_mut() = Some(tenant);
    }) {
        let telemetry = TelemetryCtx::from(&ctx);
        let _ = GT_TELEMETRY_CTX.try_with(|slot| {
            *slot.borrow_mut() = Some(telemetry);
        });
    }
}

/// Replace the task-local telemetry context with an explicit value.
pub fn set_current_telemetry_ctx(ctx: TelemetryCtx) {
    let _ = GT_TELEMETRY_CTX.try_with(|slot| {
        *slot.borrow_mut() = Some(ctx);
    });
}

/// Execute `f` with the currently configured telemetry context, if any.
pub fn with_current_telemetry_ctx<F, R>(f: F) -> R
where
    F: FnOnce(Option<TelemetryCtx>) -> R,
{
    let value = GT_TELEMETRY_CTX
        .try_with(|slot| {
            if let Some(existing) = slot.borrow().clone() {
                return Some(existing);
            }

            let base = GT_TENANT_CTX
                .try_with(|tenant| tenant.borrow().as_ref().map(TelemetryCtx::from))
                .ok()
                .flatten();

            if let Some(ref ctx) = base {
                *slot.borrow_mut() = Some(ctx.clone());
            }

            base
        })
        .ok()
        .flatten()
        .or_else(|| {
            GT_TENANT_CTX
                .try_with(|tenant| tenant.borrow().as_ref().map(TelemetryCtx::from))
                .ok()
                .flatten()
        });

    f(value)
}
