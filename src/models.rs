#[derive(Queryable)]
pub struct Repo {
    pub id: i32,
    pub uri: String,
    pub primary_db: String,
}
