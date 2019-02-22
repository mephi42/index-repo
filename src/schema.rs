table! {
    repos (id) {
        id -> Integer,
        uri -> Text,
        primary_db -> Text,
    }
}

table! {
    packages (pkgKey) {
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
    requires (name) {
        name -> Text,
        pkgKey -> Integer,
    }
}

joinable!(requires -> packages (pkgKey));
allow_tables_to_appear_in_same_query!(requires, packages);
