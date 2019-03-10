use std::env;

use clap::{Arg, ArgMatches};

pub fn database_url_arg() -> Arg<'static, 'static> {
    Arg::with_name("DATABASE_URL")
        .long("database-url")
        .takes_value(true)
}

pub fn database_url_value(matches: &ArgMatches) -> String {
    matches
        .value_of("DATABASE_URL")
        .map(std::borrow::ToOwned::to_owned)
        .or_else(|| { env::var("DATABASE_URL").ok() })
        .unwrap_or_else(|| "index.sqlite".to_owned())
}
