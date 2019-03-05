table! {
    repos (id) {
        id -> Integer,
        uri -> Text,
        primary_db -> Text,
    }
}

table! {
    #[sql_name="packages"]
    rpm_packages (pkgKey) {
        pkgKey -> Integer,
        pkgId -> Text,
        name -> Text,
        arch -> Text,
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
