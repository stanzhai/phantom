use std::time::SystemTime;

pub struct File {
    pub id: String,
    pub name: String,
    pub size: usize,
    pub ctime: SystemTime,
    pub is_file: bool,
}
