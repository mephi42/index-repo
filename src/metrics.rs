use std::fmt::{Debug, Formatter};
use std::ops::Sub;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use failure::{Error, format_err};
use lazy_static::lazy_static;
use log::info;
use prettytable::{cell, row, Table};
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
    pub elapsed_time: Duration,
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
    pub strings_hashing_time: Duration,
    pub symbols_mapping_time: Duration,
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
    f(&mut state.current);
    Ok(())
}

fn handle_metric<T: Clone + Debug + Sub<Output=T>>(
    table: &mut Table, last: &T, current: &T, name: &str,
) -> Result<(), Error> {
    table.add_row(row![
        name,
        format!("{:?}", current),
        format!("{:?}", current.clone() - last.clone()),
    ]);
    Ok(())
}

macro_rules! handle_metrics {
    ($table: expr, $last:expr, $current:expr, ($($metric:ident),* $(,)?)) => {{
        $(
            handle_metric($table, &$last.$metric, &$current.$metric, stringify!($metric))?;
        )*
    }}
}

pub fn log_metrics() -> Result<(), Error> {
    let (last, current) = {
        let mut state = STATE
            .lock()
            .map_err(|_| format_err!("Failed to lock metrics"))?;
        state.current.elapsed_time = Instant::now() - state.t0;
        let last = state.last.clone();
        let current = state.current.clone();
        state.last = state.current.clone();
        (last, current)
    };
    let mut table = Table::new();
    table.set_format(*prettytable::format::consts::FORMAT_NO_LINESEP_WITH_TITLE);
    table.set_titles(row!["Metric", "Value", "Delta"]);
    handle_metrics!(
            &mut table,
            &last,
            &current,
            (
                elapsed_time,
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
                strings_hashing_time,
                symbols_mapping_time,
                total_packages_count,
                total_packages_size,
            ));
    let mut s = Vec::new();
    table.print(&mut s)?;
    info!("\n{}", std::str::from_utf8(&s)?);
    Ok(())
}

pub async fn monitor_metrics() -> Result<(), Error> {
    loop {
        log_metrics()?;
        await_old!(sleep(Duration::from_secs(5)))?;
    }
}

pub fn timed<T, F: FnOnce() -> T>(f: F) -> (T, Duration) {
    let t0 = Instant::now();
    let result = f();
    let t = Instant::now() - t0;
    (result, t)
}

pub fn timed_result<T, E, F: FnOnce() -> Result<T, E>>(
    f: F
) -> Result<(T, Duration), E> {
    let (result, t) = timed(f);
    let result = result?;
    Ok((result, t))
}
