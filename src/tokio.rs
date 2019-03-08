use failure::{Error, format_err};
use futures::Future;
use futures::future::poll_fn;

use tokio::runtime::Runtime;

pub fn main(bootstrap: impl Future<Item=(), Error=Error> + Send + 'static) -> Result<(), Error> {
    let mut runtime = Runtime::new()?;
    runtime.block_on(bootstrap)?;
    runtime
        .shutdown_now()
        .wait()
        .map_err(|_| format_err!("Runtime::shutdown_now() failed"))?;
    Ok(())
}

pub async fn blocking<F: FnOnce() -> Result<T, Error>, T>(
    f: F
) -> Result<T, Error> {
    let mut f = Some(f);
    let t = await_old!(poll_fn(|| tokio_threadpool::blocking(f.take().unwrap())))??;
    Ok(t)
}
