use fuser::{FileAttr, FileType};

pub struct FileInfo {
    pub ino: u64,
    pub kind: FileType,
    pub attr: FileAttr,
    pub name: String,
}