use std::fmt::Debug;
use std::ops::Sub;
use std::sync::Mutex;
use std::time::Duration;

use failure::{Error, format_err};
use lazy_static::lazy_static;
use log::info;
use tokio_timer::sleep;

#[derive(Clone, Default)]
pub struct Metrics {
    pub sql_strings_insert_count: usize,
    pub sql_strings_insert_time: Duration,
    pub sql_strings_query_count_in: usize,
    pub sql_strings_query_count_out: usize,
    pub sql_strings_query_time: Duration,
}

lazy_static! {
    static ref METRICS: Mutex<Metrics> = Mutex::new(Metrics::default());
}

pub fn update_metrics<F: FnOnce(&mut Metrics) -> ()>(f: F) -> Result<(), Error> {
    let mut metrics = METRICS
        .lock()
        .map_err(|_| format_err!("Failed to lock metrics"))?;
    Ok(f(&mut metrics))
}

fn handle_metric<T: Clone + Debug + Sub<Output=T>>(
    last: &T, current: &T, name: &str,
) -> Result<(), Error> {
    let delta: T = current.clone() - last.clone();
    info!("{}: {:?} {:?}", name, current, delta);
    Ok(())
}

macro_rules! handle_metrics {
    ($last:expr, $current:expr, ($($metric:ident),* $(,)?)) => {{
        $(
            handle_metric(&$last.$metric, &$current.$metric, stringify!($metric))?;
        )*
    }}
}

pub async fn monitor() -> Result<(), Error> {
    let mut last = METRICS
        .lock()
        .map_err(|_| format_err!("Failed to lock metrics"))?
        .clone();
    loop {
        let current = METRICS
            .lock()
            .map_err(|_| format_err!("Failed to lock metrics"))?
            .clone();
        handle_metrics![
            last,
            current,
            (
                sql_strings_insert_count,
                sql_strings_insert_time,
                sql_strings_query_count_in,
                sql_strings_query_count_out,
                sql_strings_query_time,
            )];
        last = current;
        await_old!(sleep(Duration::from_secs(5)))?;
    }
}
