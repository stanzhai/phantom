mod jellyfin;
mod oof;
mod tree;

use oof::client::ClientOof;

use clap::{crate_version, App, Arg};
use oof::oof_fs::OofFS;
use std::convert::Infallible;

use crate::jellyfin::fs::JellyfinFS;
use webdav_handler::fs::DavFileSystem;
use webdav_handler::{fakels::FakeLs, DavHandler};

#[tokio::main]
async fn main() {
    env_logger::init();
    let matches = App::new("phantomFS")
        .version(crate_version!())
        .author("StanZhai")
        .arg(
            Arg::with_name("port")
                .short("p")
                .default_value("4918")
                .help("webdav server port"),
        )
        .arg(
            Arg::with_name("type")
                .short("t")
                .default_value("oof")
                .help("FS type, oof or jellyfin"),
        )
        .get_matches();

    let fs: Box<dyn DavFileSystem> = match matches.value_of("type").unwrap() {
        "oof" => OofFS::new(),
        _ => JellyfinFS::new(),
    };

    let dav_server = DavHandler::builder()
        .filesystem(fs)
        .locksystem(FakeLs::new())
        .build_handler();

    let make_service = hyper::service::make_service_fn(move |_| {
        let dav_server = dav_server.clone();
        async move {
            let func = move |req| {
                let dav_server = dav_server.clone();
                async move { Ok::<_, Infallible>(dav_server.handle(req).await) }
            };
            Ok::<_, Infallible>(hyper::service::service_fn(func))
        }
    });

    let port = matches.value_of("port").unwrap().parse().unwrap();
    let addr = ([127, 0, 0, 1], port).into();
    tracing::info!("Serving on {}", addr);
    let _ = hyper::Server::bind(&addr)
        .serve(make_service)
        .await
        .map_err(|e| eprintln!("server error: {}", e));
}
