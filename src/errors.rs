use std::error;

use diesel;
use futures::Future;

error_chain! {
    foreign_links {
        Diesel(diesel::result::Error);
    }
}

#[macro_export]
macro_rules! try_future {
    ( $x:expr ) => {
        match $x {
            Ok(t) => t,
            Err(e) => return Box::new(futures::future::failed(e)),
        }
    }
}

// https://github.com/rust-lang-nursery/error-chain/issues/90#issuecomment-280703711

pub type SFuture<T> = Box<Future<Item=T, Error=Error> + Send>;

pub trait FutureChainErr<T> {
    fn chain_err<F, E>(self, callback: F) -> SFuture<T>
        where F: FnOnce() -> E + Send + 'static,
              E: Into<ErrorKind>;
}

impl<F> FutureChainErr<F::Item> for F
    where F: Future + Send + 'static,
          F::Item: Send,
          F::Error: error::Error + Send + 'static,
{
    fn chain_err<C, E>(self, callback: C) -> SFuture<F::Item>
        where C: FnOnce() -> E + Send + 'static,
              E: Into<ErrorKind>,
    {
        Box::new(self.then(|r| r.chain_err(callback)))
    }
}
