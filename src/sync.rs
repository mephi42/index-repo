use failure::Error;
use futures::future::poll_fn;
use tokio_sync::semaphore::{Permit, Semaphore};

pub struct SemaphoreGuard<'a> {
    semaphore: &'a Semaphore,
    permit: Permit,
}

impl<'a> Drop for SemaphoreGuard<'a> {
    fn drop(&mut self) {
        self.permit.release(&self.semaphore)
    }
}

pub async fn semaphore_acquire(semaphore: &Semaphore) -> Result<SemaphoreGuard, Error> {
    let mut guard = SemaphoreGuard {
        semaphore,
        permit: Permit::new(),
    };
    await_old!(poll_fn(|| guard.permit.poll_acquire(&guard.semaphore)))?;
    Ok(guard)
}
