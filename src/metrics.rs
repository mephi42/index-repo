use std::fmt::{Debug, Formatter};
use std::ops::Sub;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use failure::{Error, format_err};
use lazy_static::lazy_static;
use log::info;
use tokio_timer::sleep;

#[derive(Clone, Default)]
pub struct Size {
    pub v: u64,
}

impl Sub for Size {
    type Output = Size;

    fn sub(self, rhs: Size) -> Self::Output {
        Size {
            v: self.v - rhs.v,
        }
    }
}

impl Debug for Size {
    fn fmt(&self, f: &mut Formatter) -> Result<(), std::fmt::Error> {
        write!(f, "{}", pretty_bytes::converter::convert(self.v as f64))
    }
}

#[derive(Clone, Default)]
pub struct Metrics {
    pub indexed_packages_count: usize,
    pub indexed_packages_size: Size,
    pub sql_files_insert_count: usize,
    pub sql_files_insert_time: Duration,
    pub sql_mutex_acquisition_count: usize,
    pub sql_mutex_acquisition_time: Duration,
    pub sql_mutex_hold_time: Duration,
    pub sql_packages_insert_count: usize,
    pub sql_packages_insert_time: Duration,
    pub sql_strings_insert_count: usize,
    pub sql_strings_insert_time: Duration,
    pub sql_strings_query_count_in: usize,
    pub sql_strings_query_count_out: usize,
    pub sql_strings_query_time: Duration,
    pub sql_symbols_insert_count: usize,
    pub sql_symbols_insert_time: Duration,
    pub total_packages_count: usize,
    pub total_packages_size: Size,
}

struct State {
    t0: Instant,
    last: Metrics,
    current: Metrics,
}

lazy_static! {
    static ref STATE: Mutex<State> = Mutex::new(State {
        t0: Instant::now(),
        last: Metrics::default(),
        current: Metrics::default(),
    });
}

pub fn update_metrics<F: FnOnce(&mut Metrics) -> ()>(f: F) -> Result<(), Error> {
    let mut state = STATE
        .lock()
        .map_err(|_| format_err!("Failed to lock metrics"))?;
    Ok(f(&mut state.current))
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

pub fn log_metrics() -> Result<(), Error> {
    let (last, current) = {
        let mut state = STATE
            .lock()
            .map_err(|_| format_err!("Failed to lock metrics"))?;
        info!("Elapsed: {:?}", Instant::now() - state.t0);
        let last = state.last.clone();
        let current = state.current.clone();
        state.last = state.current.clone();
        (last, current)
    };
    handle_metrics!(
            last,
            current,
            (
                indexed_packages_count,
                indexed_packages_size,
                sql_files_insert_count,
                sql_files_insert_time,
                sql_mutex_acquisition_count,
                sql_mutex_acquisition_time,
                sql_mutex_hold_time,
                sql_packages_insert_count,
                sql_packages_insert_time,
                sql_strings_insert_count,
                sql_strings_insert_time,
                sql_strings_query_count_in,
                sql_strings_query_count_out,
                sql_strings_query_time,
                sql_symbols_insert_count,
                sql_symbols_insert_time,
                total_packages_count,
                total_packages_size,
            ));
    Ok(())
}

pub async fn monitor_metrics() -> Result<(), Error> {
    loop {
        log_metrics()?;
        await_old!(sleep(Duration::from_secs(5)))?;
    }
}
