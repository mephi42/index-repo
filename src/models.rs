#[derive(Queryable)]
pub struct Repo {
    pub id: i32,
    pub uri: String,
    pub primary_db: String,
}

#[derive(Queryable)]
pub struct Package {
    #[column_name = "pkgKey"]
    pub pkg_key: i32,
    #[column_name = "pkgId"]
    pub pkg_id: String,
    pub name: String,
    pub arch: String,
    pub size_package: i32,
    pub location_href: String,
    pub checksum_type: String,
}
