use std::path::Path;

use diesel::dsl::exists;
use diesel::prelude::*;
use diesel::query_source::joins::{Inner, Join};
use diesel::sql_types;
use failure::{bail, Error, format_err, ResultExt};
use itertools::Itertools;
use smallvec::SmallVec;

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
            [rowid] => Ok(*rowid),
            _ => bail!("Could not find {}", $desc),
        }
    }}
}

pub fn persist_repo(
    conn: &SqliteConnection,
    repo_uri: &str,
    primary_db_data: &repomd::Data,
) -> Result<i32, Error> {
    insert_into_returning_rowid![
        conn,
        repos::table,
        repos::id,
        "a repo",
        (
            repos::uri.eq(repo_uri),
            repos::primary_db.eq(&primary_db_data.location.href),
        )]
}

pub fn persist_package(
    conn: &SqliteConnection,
    repo_id: i32,
    p: &RpmPackage,
) -> Result<i32, Error> {
    insert_into_returning_rowid![
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
        )]
}

pub fn persist_file(
    conn: &SqliteConnection,
    package_id: i32,
    name: &str,
) -> Result<i32, Error> {
    insert_into_returning_rowid![
        conn,
        files::table,
        files::id,
        "a file",
        (
            files::package_id.eq(package_id),
            files::name.eq(name),
        )]
}

fn persist_string(
    conn: &SqliteConnection,
    s: &str,
) -> Result<i32, Error> {
    let query = strings::table
        .filter(strings::name.eq(s))
        .select(strings::id)
        .limit(1);
    let rows = query
        .load::<i32>(conn)
        .context(format!("Failed to query a string"))?;
    match rows.as_slice() {
        [rowid] => Ok(*rowid),
        [] => {
            insert_into_returning_rowid![
                conn,
                strings::table,
                strings::id,
                "a string",
                (strings::name.eq(s))]
        }
        _ => unreachable!(),
    }
}

pub fn persist_elf_symbol(
    conn: &SqliteConnection,
    file_id: i32,
    strtab: &goblin::strtab::Strtab,
    sym: &goblin::elf::Sym,
) -> Result<i32, Error> {
    let name = strtab.get(sym.st_name)
        .ok_or_else(|| format_err!(
        "Failed to resolve an ELF symbol name (st_name={:x})", sym.st_name))??;
    let name_id = persist_string(conn, name)?;
    insert_into_returning_rowid![
        conn,
        elf_symbols::table,
        elf_symbols::id,
        "an ELF symbol",
        (
            elf_symbols::file_id.eq(file_id),
            elf_symbols::name_id.eq(name_id),
            elf_symbols::st_info.eq(sym.st_info as i32),
            elf_symbols::st_other.eq(sym.st_other as i32),
        )]
}
