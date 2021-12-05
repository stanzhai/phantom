mod tree;
mod jellyfin;
mod oof;

use oof::client::ClientOof;

use oof::oof_fs::OofFS;
use std::convert::Infallible;

use webdav_handler::{DavHandler, fakels::FakeLs};
use crate::jellyfin::fs::JellyfinFS;

#[tokio::main]
async fn main() {
    env_logger::init();

    let dav_server = DavHandler::builder()
        //.filesystem(OofFS::new())
        .filesystem(JellyfinFS::new())
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

    let addr = ([127, 0, 0, 1], 4918).into();
    tracing::info!("Serving on {}", addr);
    let _ = hyper::Server::bind(&addr)
        .serve(make_service)
        .await
        .map_err(|e| eprintln!("server error: {}", e));
}
