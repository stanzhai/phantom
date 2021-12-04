use std::time::SystemTime;

pub struct FileInfo {
    pub id: u64,
    pub pickcode: String,
    pub name: String,
    pub size: usize,
    pub ctime: SystemTime,
    pub is_file: bool,
    pub data: Option<Vec<u8>>,
}
