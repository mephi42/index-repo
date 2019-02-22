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
        location_href -> Text,
        checksum_type -> Text,
    }
}
