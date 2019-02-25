#[macro_export]
macro_rules! await_old {
    ( $f:expr ) => {
        {
            use tokio_async_await::compat::forward::IntoAwaitable;
            let mut f = $f;
            std::await!(f.into_awaitable())
        }
    }
}
