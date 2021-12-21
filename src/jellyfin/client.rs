use crate::jellyfin::config::Config;
use crate::jellyfin::file::File;

use bytes::Bytes;

use reqwest::header::{HeaderMap};
use reqwest::Client;
use serde_json::Value::Array;



use std::time::{SystemTime};

#[derive(Debug, Clone)]
pub struct JellyfinClient {
    client: Client,
    config: Config,
}

impl JellyfinClient {
    pub fn new(config: Config) -> JellyfinClient {
        let headers = HeaderMap::new();
        let client = Client::builder().default_headers(headers).build().unwrap();

        JellyfinClient { client, config }
    }

    pub async fn opendir(&self, item_id: &str) -> Vec<File> {
        let config = &self.config;
        let url = format!(
            "{}/Users/{}/Items?Fields=Path&ParentId={}&api_key={}",
            config.server, config.user_id, item_id, config.api_key
        );
        let res: serde_json::Value = self
            .client
            .get(url)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        let mut files = Vec::new();
        match &res["Items"] {
            Array(data) => {
                for d in data {
                    let id = d["Id"].as_str().unwrap().to_string();
                    //let mut name = d["Path"].as_str().unwrap().split("/").last().unwrap().to_string();
                    let mut name = d["Name"].as_str().unwrap().to_string();
                    let ctime = SystemTime::now();
                    let is_file = !d["IsFolder"].as_bool().unwrap();
                    let mut data: Option<String> = None;

                    let size = if is_file {
                        /*
                        let url = format!("{}/Users/{}/Items/{}?api_key={}", config.server, config.user_id, id, config.api_key);
                        let item_res: serde_json::Value = self.client.get(url).send().await.unwrap().json().await.unwrap();
                        let file_info = &item_res["MediaSources"].as_array().unwrap()[0];
                        file_info["Size"].as_u64().unwrap() as usize
                         */
                        /*
                        let url = format!("{}/Items/{}/PlaybackInfo?UserId={}&StartTimeTicks=0&IsPlayback=true&AutoOpenLiveStream=true&AudioStreamIndex=1&SubtitleStreamIndex=-1&MediaSourceId={}&MaxStreamingBitrate={}&api_key={}", config.server, id, config.user_id, id, config.bitrate, config.bitrate);
                        tracing::info!("{}", url);
                        let item_res: serde_json::Value = self.client.post(url).send().await.unwrap().json().await.unwrap();
                        let file_info = &item_res["MediaSources"].as_array().unwrap()[0];
                        let url_data = if let Some(transcoding_url) = file_info.get("TranscodingUrl") {
                            format!("{}{}", config.server, transcoding_url.as_str().unwrap())
                        } else {
                            format!("{}/Items/{}/Download?api_key={}", config.server, id, config.api_key)
                        };
                         */
                        let url_data = format!("#EXTM3U\r\n#EXT-X-VERSION:7\r\n{}/Videos/{}/stream.mov?Static=true&mediaSourceId={}&api_key={}", config.server, id, id, config.api_key);
                        let size = url_data.len();
                        data = Some(url_data);
                        size
                    } else {
                        0
                    };

                    if is_file {
                        name = format!("{}.m3u8", name);
                    }

                    let file = File {
                        id,
                        name,
                        size,
                        ctime,
                        is_file,
                        data,
                    };

                    tracing::info!(
                        "load file info: {} -> {} (size: {})",
                        file.id,
                        file.name,
                        file.size
                    );
                    files.push(file);
                }
            }
            _ => {}
        }
        files
    }

    async fn download(&self, id: &str, start: usize, end: usize) -> Bytes {
        tracing::info!("call download: {}, {}-{}", id, start, end);
        let config = &self.config;
        let url = format!(
            "{}/Items/{}/Download?api_key={}",
            config.server, id, config.api_key
        );
        /*
        let res = self
            .client
            .get(url)
            .header("Range", format!("bytes={}-{}", start, end))
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        res
         */
        Bytes::from(url)
    }
}
