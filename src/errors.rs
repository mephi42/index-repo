use std::fmt::Display;

use failure::{Context, Fail};
use futures::{Future, Stream};

#[macro_export]
macro_rules! try_future {
    ( $x:expr ) => {
        match $x {
            Ok(t) => t,
            Err(e) => return Box::new(futures::future::failed(e.into())),
        }
    }
}

pub trait FutureExt<T, E> {
    fn context<D>(self, context: D) -> Box<Future<Item=T, Error=Context<D>> + Send> where
        D: Display + Send + Sync + 'static,
    ;

    fn with_context<F, D>(self, f: F) -> Box<Future<Item=T, Error=Context<D>> + Send> where
        F: FnOnce(&E) -> D + Send + 'static,
        D: Display + Send + Sync + 'static,
    ;
}

impl<FF> FutureExt<<FF as Future>::Item, <FF as Future>::Error> for FF where
    FF: Future + Send + 'static,
    <FF as Future>::Error: Fail,
{
    fn context<D>(self, context: D)
                  -> Box<Future<Item=<FF as Future>::Item, Error=Context<D>> + Send> where
        D: Display + Send + Sync + 'static,
    {
        Box::new(self.map_err(|failure| failure.context(context)))
    }

    fn with_context<F, D>(self, f: F)
                          -> Box<Future<Item=<FF as Future>::Item, Error=Context<D>> + Send> where
        F: FnOnce(&<FF as Future>::Error) -> D + Send + 'static,
        D: Display + Send + Sync + 'static,
    {
        Box::new(self.map_err(|failure| {
            let context = f(&failure);
            failure.context(context)
        }))
    }
}

pub trait StreamExt<T, E> {
    fn context<D>(self, context: D) -> Box<Stream<Item=T, Error=Context<D>> + Send> where
        D: Display + Send + Sync + Clone + 'static,
    ;

    fn with_context<S, D>(self, s: S) -> Box<Stream<Item=T, Error=Context<D>> + Send> where
        S: FnMut(&E) -> D + Send + 'static,
        D: Display + Send + Sync + 'static,
    ;
}

impl<S> StreamExt<<S as Stream>::Item, <S as Stream>::Error> for S where
    S: Stream + Send + 'static,
    <S as Stream>::Error: Fail,
{
    fn context<D>(self, context: D)
                  -> Box<Stream<Item=<S as Stream>::Item, Error=Context<D>> + Send> where
        D: Display + Send + Sync + Clone + 'static,
    {
        Box::new(self.map_err(move |failure| failure.context(context.clone())))
    }

    fn with_context<F, D>(self, mut f: F)
                          -> Box<Stream<Item=<S as Stream>::Item, Error=Context<D>> + Send> where
        F: FnMut(&<S as Stream>::Error) -> D + Send + 'static,
        D: Display + Send + Sync + 'static,
    {
        Box::new(self.map_err(move |failure| {
            let context = f(&failure);
            failure.context(context)
        }))
    }
}
