table! {
    repos (id) {
        id -> Integer,
        uri -> Text,
        primary_db -> Text,
    }
}

table! {
    packages (id) {
        id -> Integer,
        repo_id -> Integer,
        name -> Text,
        arch -> Text,
        version -> Text,
        epoch -> Text,
        release -> Text,
    }
}

joinable!(packages -> repos (repo_id));

table! {
    files (id) {
        id -> Integer,
        name -> Text,
        package_id -> Integer,
    }
}

joinable!(files -> packages (package_id));

table! {
    strings (id) {
        id -> Integer,
        name -> Text,
    }
}

table! {
    elf_symbols (id) {
        id -> Integer,
        file_id -> Integer,
        name_id -> Integer,
        st_info -> Integer,
        st_other -> Integer,
    }
}

joinable!(elf_symbols -> files (file_id));
joinable!(elf_symbols -> strings (name_id));

allow_tables_to_appear_in_same_query!(
    repos,
    packages,
    files,
    strings,
    elf_symbols,
);

table! {
    #[sql_name="packages"]
    rpm_packages (pkgKey) {
        pkgKey -> Integer,
        pkgId -> Text,
        name -> Text,
        arch -> Text,
        version -> Text,
        epoch -> Text,
        release -> Text,
        size_package -> Integer,
        location_href -> Text,
        checksum_type -> Text,
    }
}

table! {
    // FIXME: There is no primary key, so using an arbitrary column to make Diesel happy
    #[sql_name="requires"]
    rpm_requires (name) {
        name -> Text,
        pkgKey -> Integer,
    }
}

joinable!(rpm_requires -> rpm_packages (pkgKey));
allow_tables_to_appear_in_same_query!(rpm_requires, rpm_packages);
