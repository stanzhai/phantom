mod cloud_fs;
mod client_115;
mod file_info;

use clap::{crate_version, App, Arg};
use fuser::MountOption;
use crate::cloud_fs::CloudFS;

fn main() {
    let matches = App::new("phantomFS")
        .version(crate_version!())
        .author("StanZhai")
        .arg(
            Arg::with_name("MOUNT_POINT")
                .required(true)
                .index(1)
                .help("Act as a client, and mount FUSE at given path"),
        )
        .arg(
            Arg::with_name("auto_unmount")
                .long("auto_unmount")
                .help("Automatically unmount on process exit"),
        )
        .arg(
            Arg::with_name("allow-root")
                .long("allow-root")
                .help("Allow root user to access filesystem"),
        )
        .get_matches();

    env_logger::init();
    let mountpoint = matches.value_of("MOUNT_POINT").unwrap();
    let mut options = vec![MountOption::RO, MountOption::FSName("phantom".to_string())];
    options.push(MountOption::AutoUnmount);
    if matches.is_present("allow-root") {
        options.push(MountOption::AllowRoot);
    }
    fuser::mount2(CloudFS::default(), mountpoint, &options).unwrap();
}
