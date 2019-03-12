use std::time::Instant;

use clap::{app_from_crate, Arg, crate_authors, crate_description, crate_name, crate_version};
use diesel::debug_query;
use diesel::prelude::*;
use diesel::sqlite::Sqlite;
use diesel_migrations::run_pending_migrations;
use dotenv::dotenv;
use failure::{Error, ResultExt};
use prettytable::{cell, row, Table};

use index_repo::clap::{database_url_arg, database_url_value};
use index_repo::schema::*;

fn main() -> Result<(), Error> {
    dotenv().ok();
    let matches = app_from_crate!()
        .arg(database_url_arg())
        .arg(Arg::with_name("SYMBOL")
            .required(true)
            .index(1)
            .multiple(true))
        .get_matches();
    let database_url = database_url_value(&matches);
    let symbols = matches.values_of_lossy("SYMBOL").unwrap();
    let conn = SqliteConnection::establish(&database_url)
        .context(format!("SqliteConnection::establish({}) failed", database_url))?;
    run_pending_migrations(&conn)
        .context("run_pending_migrations() failed")?;
    let t0 = Instant::now();
    let query = strings::table
        .inner_join(elf_symbols::table
            .inner_join(files::table
                .inner_join(packages::table)))
        .filter(strings::name.eq_any(symbols))
        .select((packages::name, files::name, strings::name));
    println!("sql> {}", debug_query::<Sqlite, _>(&query));
    let rows = query
        .load::<(String, String, String)>(&conn)
        .context("Failed to query a symbol")?;
    let t = Instant::now() - t0;
    let len = rows.len();
    let mut table = Table::new();
    table.set_format(*prettytable::format::consts::FORMAT_NO_LINESEP_WITH_TITLE);
    table.set_titles(row!["Package", "File", "Symbol"]);
    for (package, file, symbol) in rows {
        table.add_row(row![package, file, symbol]);
    };
    table.printstd();
    println!("{} rows retrieved in {:?}", len, t);
    Ok(())
}
