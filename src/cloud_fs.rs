use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
    Request,
};
use libc::ENOENT;
use std::ffi::OsStr;
use std::time::{Duration, UNIX_EPOCH};
use lru::LruCache;
use crate::client_115::Client115;
use crate::file_info::FileInfo;

const TTL: Duration = Duration::from_secs(1); // 1 second

const ROOT_DIR_ATTR: FileAttr = FileAttr {
    ino: 1,
    size: 0,
    blocks: 0,
    atime: UNIX_EPOCH, // 1970-01-01 00:00:00
    mtime: UNIX_EPOCH,
    ctime: UNIX_EPOCH,
    crtime: UNIX_EPOCH,
    kind: FileType::Directory,
    perm: 0o755,
    nlink: 2,
    uid: 501,
    gid: 20,
    rdev: 0,
    flags: 0,
    blksize: 512,
};

pub struct CloudFS {
    client: Client115,
    current_dir: u64,
    cache: LruCache<u64, Vec<FileInfo>>,
}

impl Default for CloudFS {
    fn default() -> Self {
        CloudFS {
            client: Client115::default(),
            current_dir: 1,
            cache: LruCache::new(50),
        }
    }
}

impl CloudFS {
    fn update_cache(&mut self, ino: u64) {
        if let None = self.cache.get(&ino) {
            let cid = match ino {
                1 => 0,
                _ => ino,
            };
            self.cache.put(ino, self.client.opendir(cid));
        }
    }
}

impl Filesystem for CloudFS {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        println!("call lookup, parent: {}, name: {:?}", parent, name);
        self.update_cache(parent);

        let name = name.to_str().unwrap().to_string();
        for entry in self.cache.get(&parent).unwrap() {
            if entry.name == name {
                reply.entry(&TTL, &entry.attr, 0);
                return;
            }
        }
        reply.error(ENOENT);
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        println!("call getattr, ino: {}", ino);
        self.update_cache(self.current_dir);

        if ino == 1 {
            reply.attr(&TTL, &ROOT_DIR_ATTR);
            return;
        }

        for entry in self.cache.get(&self.current_dir).unwrap() {
            if entry.ino == ino {
                reply.attr(&TTL, &entry.attr);
                return;
            }
        }
        reply.error(ENOENT);
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        size: u32,
        _flags: i32,
        _lock: Option<u64>,
        reply: ReplyData,
    ) {
        println!("call read, ino: {}, offset: {}, size: {}", ino, offset, size);

        self.update_cache(self.current_dir);
        for entry in self.cache.get(&self.current_dir).unwrap() {
            if entry.ino == ino {
                let data = self.client.download(entry.pickcode.as_str(), offset, size);
                reply.data(data.as_slice());
                //reply.error(ENOENT);
                return;
            }
        }
        reply.error(ENOENT);
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        println!("call readdir, ino: {}", ino);
        self.current_dir = ino;
        self.update_cache(ino);

        let entries = self.cache.get(&ino).unwrap();

        for (i, entry) in entries.into_iter().enumerate().skip(offset as usize) {
            // i + 1 means the index of the next entry
            if reply.add(entry.ino, (i + 1) as i64, entry.kind, entry.name.to_string()) {
                break;
            }
        }
        reply.ok();
    }
}