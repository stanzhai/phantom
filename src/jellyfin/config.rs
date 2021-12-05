use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct Config {
    pub server: String,
    pub user_id: String,
    pub root_folder_id: String,
    pub api_key: String,
}