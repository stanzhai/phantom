use std::ops::Add;
use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use fuser::{FileAttr, FileType};
use reqwest::header::{COOKIE, HeaderMap, HeaderValue, USER_AGENT};
use reqwest::blocking::Client;
use serde_json::Value::Array;
use crate::file_info::FileInfo;

#[derive(Clone)]
pub struct Client115 {
    client: Client,
}

impl Client115 {
    pub fn new() -> Client115 {
        let cookie = fs::read_to_string("115.cookie").unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_16_0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/83.0.4103.61 Safari/537.36 115Browser/24.1.0.13".parse().unwrap());
        headers.insert(COOKIE, cookie.trim().parse().unwrap());

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .unwrap();

        Client115 {
            client
        }
    }

    pub fn opendir(&self, cid: u64) -> Vec<FileInfo> {
        let url = format!("https://webapi.115.com/files?aid=1&cid={}&o=user_ptime&asc=0&offset=0&show_dir=1&limit=1000", cid);
        let res: serde_json::Value = self.client.get(url).send().unwrap().json().unwrap();
        let mut files = Vec::new();
        match &res["data"] {
            Array(data) => {
                for d in data {
                    let ut: u64 = d["te"].as_str().unwrap().parse().unwrap();
                    let time = UNIX_EPOCH.add(Duration::from_secs(ut));
                    let blksize: u32 = 512;
                    let mut attr = FileAttr {
                        ino: 1,
                        size: 0,
                        blocks: 0,
                        atime: time,
                        mtime: time,
                        ctime: time,
                        crtime: time,
                        kind: FileType::Directory,
                        perm: 0o755,
                        nlink: 2,
                        uid: 501,
                        gid: 20,
                        rdev: 0,
                        flags: 0,
                        blksize,
                    };

                    let mut pickcode = "";
                    let ino: u64;
                    if let Some(fid) = d.get("fid") {
                        ino = fid.as_str().unwrap().parse().unwrap();
                        let size: u64 = d["s"].as_u64().unwrap();
                        attr.ino = ino;
                        attr.size = size;
                        attr.blocks = size / (blksize as u64) + 1;
                        attr.kind = FileType::RegularFile;
                        attr.perm = 0o644;
                        attr.nlink = 1;
                        pickcode = d["pc"].as_str().unwrap();
                    } else {
                        ino = d["cid"].as_str().unwrap().parse().unwrap();
                        attr.ino = ino;
                    };

                    files.push(FileInfo {
                        ino,
                        attr,
                        name: d["n"].as_str().unwrap().to_string(),
                        kind: attr.kind,
                        pickcode: pickcode.to_string(),
                    });
                    println!("{} -> {}", ino, d["n"].as_str().unwrap())
                }
            },
            _ => {}
        }
        files
    }

    pub fn download(&self, pickcode: &str, offset: i64, size: u32) -> Vec<u8> {
        let url = format!("http://115.com/file/{}?share_id=", pickcode);
        let res = self.client.get(url)
            .header("Range", format!("bytes={}-{}", offset, size))
            .header("Referer", format!("https://115.com/?ct=download&ac=index&pickcode={}", pickcode))
            .send()
            .unwrap()
            .bytes()
            .unwrap();
        res.to_vec()
    }
}

impl Default for Client115 {
    fn default() -> Self {
        Client115::new()
    }
}