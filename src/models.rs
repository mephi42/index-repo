use diesel::Queryable;

#[derive(Queryable)]
pub struct Repo {
    pub id: i32,
    pub uri: String,
    pub primary_db: String,
}

#[derive(Queryable)]
pub struct RpmPackage {
    #[column_name = "pkgKey"]
    pub pkg_key: i32,
    #[column_name = "pkgId"]
    pub pkg_id: String,
    pub name: String,
    pub arch: String,
    pub version: String,
    pub epoch: String,
    pub release: String,
    pub size_package: i32,
    pub location_href: String,
    pub checksum_type: String,
}
