use crate::oof::oof_file::OofFile;
use reqwest::header::{HeaderMap, COOKIE, USER_AGENT};
use reqwest::Client;
use serde_json::Value::Array;
use std::fs;
use std::ops::Add;
use std::time::{Duration, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct ClientOof {
    client: Client,
}

impl ClientOof {
    pub fn new() -> ClientOof {
        let cookie = fs::read_to_string("115.cookie").expect("file `115.cookie` does not exits");
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_16_0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/83.0.4103.61 Safari/537.36 115Browser/24.1.0.13".parse().unwrap());
        headers.insert(COOKIE, cookie.trim().parse().unwrap());

        let client = Client::builder().default_headers(headers).build().unwrap();

        ClientOof { client }
    }

    pub async fn opendir(&self, cid: u64) -> Vec<OofFile> {
        let cid = match cid {
            1 => 0,
            _ => cid,
        };

        let url = format!("https://webapi.115.com/files?aid=1&cid={}&o=user_ptime&asc=0&offset=0&show_dir=1&limit=1000", cid);
        let res: serde_json::Value = self
            .client
            .get(url)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        if let Some(error) = res.get("error") {
            tracing::error!("opendir failed! {}", error.as_str().unwrap());
        }

        let mut files = Vec::new();
        match &res["data"] {
            Array(data) => {
                for d in data {
                    let mut name = d["n"].as_str().unwrap().to_string();
                    let ut: u64 = d["te"].as_str().unwrap().parse().unwrap();
                    let time = UNIX_EPOCH.add(Duration::from_secs(ut));

                    let file_info = if let Some(fid) = d.get("fid") {
                        let ino = fid.as_str().unwrap().parse().unwrap();
                        if let Some(_) = d.get("play_long") {
                            let pickcode = d["pc"].as_str().unwrap();
                            let file_content = self.download(pickcode).await;
                            let size = file_content.len();
                            let data = Some(file_content);
                            name = format!("{}.m3u8", name);

                            OofFile {
                                id: ino,
                                name,
                                size,
                                pickcode: pickcode.to_owned(),
                                ctime: time,
                                is_file: true,
                                data,
                            }
                        } else {
                            continue;
                        }
                    } else {
                        let ino = d["cid"].as_str().unwrap().parse().unwrap();
                        OofFile {
                            id: ino,
                            name,
                            size: 0,
                            pickcode: "".to_owned(),
                            ctime: time,
                            is_file: false,
                            data: None,
                        }
                    };

                    tracing::info!(
                        "load file info: {} -> {} (size: {})",
                        file_info.id,
                        file_info.name,
                        file_info.size
                    );
                    files.push(file_info);
                }
            }
            _ => {}
        }
        files
    }

    async fn download(&self, pickcode: &str) -> Vec<u8> {
        let url = format!("http://115.com/api/video/m3u8/{}.m3u8", pickcode);
        let res = self
            .client
            .get(url)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();

        // fix m3u8, just keep one video address.
        let mut t = vec![];
        for text in res.lines() {
            t.push(text.trim());
            if text.starts_with("http") {
                break;
            }
        }

        let result = t.join("\r\n");
        result.into_bytes()
    }
}

impl Default for ClientOof {
    fn default() -> Self {
        ClientOof::new()
    }
}
