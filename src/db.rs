use std::collections::{HashMap, HashSet};
use std::iter::FromIterator;
use std::path::Path;

use diesel::dsl::exists;
use diesel::prelude::*;
use diesel::query_source::joins::{Inner, Join};
use diesel::sql_types;
use failure::{bail, Error, format_err, ResultExt};
use itertools::Itertools;
use smallvec::SmallVec;

use crate::metrics::{timed, timed_result, update_metrics};
use crate::models::*;
use crate::repomd;
use crate::schema::*;

fn like_from_wildcard(s: &str) -> String {
    s.chars().flat_map(|c| {
        let mut v = SmallVec::<[char; 2]>::new();
        match c {
            '*' => v.push('%'),
            '?' => v.push('_'),
            '%' => v.extend_from_slice(&['\\', '%']),
            '_' => v.extend_from_slice(&['\\', '_']),
            '\\' => v.extend_from_slice(&['\\', '\\']),
            x => v.push(x),
        };
        v
    }).collect()
}

pub fn get_packages(
    path: &Path,
    arches: &Option<Vec<String>>,
    requirements: &Option<Vec<String>>,
) -> Result<Vec<RpmPackage>, Error> {
    let database_url = "file:".to_owned() +
        path.to_str().ok_or_else(|| format_err!("Malformed path: {:?}", path))? +
        "?mode=ro";
    let conn = SqliteConnection::establish(&database_url)
        .with_context(|_| format!(
            "SqliteConnection::establish({}) failed", database_url))?;
    let mut query = rpm_packages::table.into_boxed();
    if let Some(requirements) = requirements {
        // https://stackoverflow.com/a/48712715/3832536
        // https://github.com/diesel-rs/diesel/issues/1544#issuecomment-363440046
        type B = Box<BoxableExpression<
            Join<rpm_requires::table, rpm_packages::table, Inner>,
            diesel::sqlite::Sqlite,
            SqlType=sql_types::Bool>>;
        let like: B = requirements
            .iter()
            .map(|r| -> B {
                Box::new(rpm_requires::name.like(like_from_wildcard(r)))
            }).fold1(|q, l| Box::new(q.or(l)))
            .unwrap();
        query = query.filter(exists(rpm_requires::table.filter(
            rpm_requires::pkgKey.eq(rpm_packages::pkgKey).and(like))));
    }
    if let Some(arches) = arches {
        query = query.filter(rpm_packages::arch.eq_any(arches));
    }
    query.load::<RpmPackage>(&conn)
        .context("Failed to query packages")
        .map_err(Error::from)
}

macro_rules! and_all {
    ($x:expr) => {
        $x
    };
    ($x:expr, $($xs:expr),+ $(,)?) => {{
        $x.and(and_all![$($xs),*])
    }};
}

macro_rules! insert_into_returning_rowid {
    ($conn:expr, $table: expr, $rowid: expr, $desc: expr, ($($vs:expr),* $(,)?)) => {{
        diesel::insert_into($table)
            .values(($($vs,)*))
            .execute($conn)
            .context(format!("Failed to insert {}", $desc))?;
        let rows = $table
            .filter(and_all![$($vs),*])
            .select($rowid)
            .limit(1)
            .load::<i32>($conn)
            .context(format!("Failed to query {}", $desc))?;
        match rows.as_slice() {
            [rowid] => Ok::<i32, Error>(*rowid),
            _ => bail!("Could not find {}", $desc),
        }
    }}
}

pub fn persist_repo(
    conn: &SqliteConnection,
    repo_uri: &str,
    primary_db_data: &repomd::Data,
) -> Result<i32, Error> {
    insert_into_returning_rowid!(
        conn,
        repos::table,
        repos::id,
        "a repo",
        (
            repos::uri.eq(repo_uri),
            repos::primary_db.eq(&primary_db_data.location.href),
        ))
}

pub fn persist_package(
    conn: &SqliteConnection,
    repo_id: i32,
    p: &RpmPackage,
) -> Result<i32, Error> {
    let (package_id, t) = timed_result(|| insert_into_returning_rowid!(
        conn,
        packages::table,
        packages::id,
        "a package",
        (
            packages::repo_id.eq(repo_id),
            packages::name.eq(&p.name),
            packages::arch.eq(&p.arch),
            packages::version.eq(&p.version),
            packages::epoch.eq(&p.epoch),
            packages::release.eq(&p.release),
        )))?;
    update_metrics(|metrics| {
        metrics.sql_packages_insert_count += 1;
        metrics.sql_packages_insert_time += t;
    })?;
    Ok(package_id)
}

pub fn persist_file(
    conn: &SqliteConnection,
    package_id: i32,
    name: &str,
) -> Result<i32, Error> {
    let (file_id, t) = timed_result(|| insert_into_returning_rowid!(
        conn,
        files::table,
        files::id,
        "a file",
        (
            files::package_id.eq(package_id),
            files::name.eq(name),
        )))?;
    update_metrics(|metrics| {
        metrics.sql_files_insert_count += 1;
        metrics.sql_files_insert_time += t;
    })?;
    Ok(file_id)
}

fn query_strings<'a>(
    conn: &SqliteConnection,
    strings: &mut HashSet<&'a str>,
    mappings: &mut HashMap<&'a str, i32>,
) -> Result<(), Error> {
    let sqlite_max_variable_number = 999;
    let strings_vec: Vec<&'a str> = Vec::from_iter(strings.iter().cloned());
    for chunk in strings_vec.chunks(sqlite_max_variable_number) {
        let (rows, t) = timed_result(|| strings::table
            .filter(strings::name.eq_any(chunk))
            .select((strings::id, strings::name))
            .load::<(i32, String)>(conn)
            .context("Failed to query strings"))?;
        update_metrics(|metrics| {
            metrics.sql_strings_query_count_in += chunk.len();
            metrics.sql_strings_query_count_out += rows.len();
            metrics.sql_strings_query_time += t;
        })?;
        for (string_id, string_name) in rows {
            match strings.take(string_name.as_str()) {
                Some(t) => mappings.insert(t, string_id),
                None => bail!("Query has returned an unknown string"),
            };
        }
    }
    Ok(())
}

fn persist_strings<'a>(
    conn: &SqliteConnection,
    mut strings: HashSet<&'a str>,
) -> Result<HashMap<&'a str, i32>, Error> {
    let mut mappings: HashMap<&'a str, i32> = HashMap::with_capacity(strings.len());
    query_strings(conn, &mut strings, &mut mappings)?;
    if !strings.is_empty() {
        let (_, t) = timed_result(|| diesel::insert_into(strings::table)
            .values(strings
                .iter()
                .map(|string| strings::name.eq(string))
                .collect::<Vec<_>>())
            .execute(conn)
            .context("Failed to insert strings"))?;
        update_metrics(|metrics| {
            metrics.sql_strings_insert_count += strings.len();
            metrics.sql_strings_insert_time += t;
        })?;
        query_strings(conn, &mut strings, &mut mappings)?;
        if !strings.is_empty() {
            bail!("Failed to persist all strings");
        }
    }
    Ok(mappings)
}

pub fn persist_elf_symbols(
    conn: &SqliteConnection,
    package_id: i32,
    file_name: &str,
    symbols: Vec<(&str, i32, i32)>,
) -> Result<(), Error> {
    let file_id = persist_file(conn, package_id, file_name)?;
    let (strings, t): (HashSet<&str>, _) = timed(|| HashSet::from_iter(symbols
        .iter()
        .map(|x| x.0)));
    update_metrics(|metrics| {
        metrics.strings_hashing_time += t;
    })?;
    let mappings = persist_strings(conn, strings)?;
    let (symbols_values, t) = timed_result(|| symbols
        .into_iter()
        .map(|(name, st_info, st_other)| {
            match mappings.get(name) {
                Some(name_id) => Ok((
                    elf_symbols::file_id.eq(file_id),
                    elf_symbols::name_id.eq(*name_id),
                    elf_symbols::st_info.eq(st_info),
                    elf_symbols::st_other.eq(st_other),
                )),
                None => Err(format_err!("persist_strings() has returned an unknown string")),
            }
        })
        .collect::<Result<Vec<_>, Error>>())?;
    update_metrics(|metrics| {
        metrics.symbols_mapping_time += t;
    })?;
    let count = symbols_values.len();
    let (_, t) = timed_result(|| diesel::insert_into(elf_symbols::table)
        .values(symbols_values)
        .execute(conn)
        .context("Failed to insert ELF symbols"))?;
    update_metrics(|metrics| {
        metrics.sql_symbols_insert_count += count;
        metrics.sql_symbols_insert_time += t;
    })?;
    Ok(())
}
