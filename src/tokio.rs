use failure::{Error, format_err};
use futures::Future;

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
