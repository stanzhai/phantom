use std::collections::HashMap;
use std::fs;
use std::io::{Error, ErrorKind, SeekFrom};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::Mutex;

use webdav_handler::davpath::DavPath;
use webdav_handler::fs::{
    DavDirEntry, DavFile, DavFileSystem, DavMetaData, DavProp, FsError, FsFuture, FsResult,
    FsStream, OpenOptions, ReadDirMeta,
};

use crate::jellyfin::client::JellyfinClient;
use crate::jellyfin::config::Config;
use crate::{jellyfin, tree};
use bytes::{Buf, Bytes};
use futures::{future, future::FutureExt};

type Tree = tree::Tree<Vec<u8>, FSNode>;

#[derive(Debug)]
pub struct JellyfinFS {
    client: Arc<JellyfinClient>,
    tree: Arc<Mutex<Tree>>,
    visited: Arc<Mutex<Vec<u64>>>,
}

#[derive(Debug, Clone)]
enum FSNode {
    Dir(FSDirNode),
    File(FSFileNode),
}

#[derive(Debug, Clone)]
struct FSDirNode {
    id: String,
    props: HashMap<String, DavProp>,
    mtime: SystemTime,
    crtime: SystemTime,
}

#[derive(Debug, Clone)]
struct FSFileNode {
    props: HashMap<String, DavProp>,
    mtime: SystemTime,
    crtime: SystemTime,
    id: String,
    size: usize,
    data: String,
}

#[derive(Debug, Clone)]
struct FSEntry {
    mtime: SystemTime,
    crtime: SystemTime,
    is_dir: bool,
    name: Vec<u8>,
    size: u64,
}

#[derive(Debug)]
struct FSFile {
    tree: Arc<Mutex<Tree>>,
    node_id: u64,
    id: String,
    client: Arc<JellyfinClient>,
    pos: usize,
    append: bool,
}

impl JellyfinFS {
    /// Create a new "FS" filesystem.
    pub fn new() -> Box<JellyfinFS> {
        let demo_config = serde_json::ser::to_string(&Config::default()).unwrap();
        let msg = format!(
            "jellyfin.json does not exists, you should create with the content like:\r\n{}",
            demo_config
        );
        let config_str = fs::read_to_string("jellyfin.json").expect(msg.as_str());
        let mut config = serde_json::de::from_str::<Config>(config_str.as_str()).unwrap();
        if config.bitrate == 0 {
            config.bitrate = 4000000;
        }
        let root_id = config.root_folder_id.to_string();

        let client = JellyfinClient::new(config);
        let root = FSNode::new_dir(root_id);
        Box::new(JellyfinFS {
            client: Arc::new(client),
            tree: Arc::new(Mutex::new(Tree::new(root))),
            visited: Arc::new(Mutex::new(Vec::new())),
        })
    }

    fn do_open<'a>(
        &'a self,
        tree: &mut Tree,
        path: &[u8],
        options: OpenOptions,
    ) -> FsResult<Box<dyn DavFile>> {
        let node_id = match tree.lookup(path) {
            Ok(n) => {
                if options.create_new {
                    return Err(FsError::Exists);
                }
                n
            }
            Err(FsError::NotFound) => {
                return Err(FsError::NotFound);
            }
            Err(e) => return Err(e),
        };
        let node = tree.get_node_mut(node_id).unwrap();
        if node.is_dir() {
            return Err(FsError::Forbidden);
        }

        let file = node.as_file().unwrap();

        Ok(Box::new(FSFile {
            node_id,
            id: file.id.to_string(),
            tree: self.tree.clone(),
            client: self.client.clone(),
            pos: 0,
            append: options.append,
        }))
    }
}

impl Clone for JellyfinFS {
    fn clone(&self) -> Self {
        JellyfinFS {
            client: self.client.clone(),
            tree: Arc::clone(&self.tree),
            visited: Arc::clone(&self.visited),
        }
    }
}

impl DavFileSystem for JellyfinFS {
    fn open<'a>(&'a self, path: &'a DavPath, options: OpenOptions) -> FsFuture<Box<dyn DavFile>> {
        async move {
            let tree = &mut *self.tree.lock().await;
            self.do_open(tree, path.as_bytes(), options)
        }
        .boxed()
    }

    fn read_dir<'a>(
        &'a self,
        path: &'a DavPath,
        _meta: ReadDirMeta,
    ) -> FsFuture<FsStream<Box<dyn DavDirEntry>>> {
        async move {
            let tree = &mut self.tree.lock().await;
            let node_id = tree.lookup(path.as_bytes())?;
            let node = tree.get_node(node_id)?;
            if !node.is_dir() {
                return Err(FsError::Forbidden);
            }

            let dir_id = node.as_dir()?.id.to_string();

            let visited = &mut self.visited.lock().await;
            if !visited.iter().any(|id| node_id == *id) {
                let entries = self.client.opendir(dir_id.as_str()).await;
                for entry in entries {
                    let node = if entry.is_file {
                        FSNode::File(FSFileNode {
                            crtime: entry.ctime,
                            mtime: entry.ctime,
                            id: entry.id,
                            props: HashMap::new(),
                            size: entry.size,
                            data: entry.data.unwrap(),
                        })
                    } else {
                        FSNode::Dir(FSDirNode {
                            id: entry.id,
                            crtime: entry.ctime,
                            mtime: entry.ctime,
                            props: HashMap::new(),
                        })
                    };
                    tree.add_child(0, node_id, entry.name.into_bytes(), node, false);
                }
                visited.push(node_id);
            }

            let mut v: Vec<Box<dyn DavDirEntry>> = Vec::new();
            for (name, dnode_id) in tree.get_children(node_id)? {
                if let Ok(node) = tree.get_node(dnode_id) {
                    v.push(Box::new(node.as_dirent(&name)));
                }
            }
            let strm = futures::stream::iter(v.into_iter());
            Ok(Box::pin(strm) as FsStream<Box<dyn DavDirEntry>>)
        }
        .boxed()
    }

    fn metadata<'a>(&'a self, path: &'a DavPath) -> FsFuture<Box<dyn DavMetaData>> {
        async move {
            let tree = &*self.tree.lock().await;
            let node_id = tree.lookup(path.as_bytes())?;
            let meta = tree.get_node(node_id)?.as_dirent(path.as_bytes());
            Ok(Box::new(meta) as Box<dyn DavMetaData>)
        }
        .boxed()
    }
}

// small helper.
fn propkey(ns: &Option<String>, name: &str) -> String {
    ns.to_owned().as_ref().unwrap_or(&"".to_string()).clone() + name
}

// small helper.
fn cloneprop(p: &DavProp) -> DavProp {
    DavProp {
        name: p.name.clone(),
        namespace: p.namespace.clone(),
        prefix: p.prefix.clone(),
        xml: None,
    }
}

impl DavDirEntry for FSEntry {
    fn name(&self) -> Vec<u8> {
        self.name.clone()
    }

    fn metadata<'a>(&'a self) -> FsFuture<Box<dyn DavMetaData>> {
        let meta = (*self).clone();
        Box::pin(future::ok(Box::new(meta) as Box<dyn DavMetaData>))
    }
}

impl DavFile for FSFile {
    fn metadata<'a>(&'a mut self) -> FsFuture<Box<dyn DavMetaData>> {
        async move {
            let tree = &*self.tree.lock().await;
            let node = tree.get_node(self.node_id)?;
            let meta = node.as_dirent(b"");
            Ok(Box::new(meta) as Box<dyn DavMetaData>)
        }
        .boxed()
    }

    fn write_buf<'a>(&'a mut self, _buf: Box<dyn Buf + Send>) -> FsFuture<()> {
        async move { Err(Error::new(ErrorKind::PermissionDenied, "read only fs").into()) }.boxed()
    }

    fn write_bytes(&mut self, _buf: Bytes) -> FsFuture<()> {
        async move { Err(Error::new(ErrorKind::PermissionDenied, "read only fs").into()) }.boxed()
    }

    fn read_bytes(&mut self, count: usize) -> FsFuture<Bytes> {
        async move {
            let tree = &*self.tree.lock().await;
            let node = tree.get_node(self.node_id)?;
            let file = node.as_file()?;
            /*
            let curlen = file.size;

            let mut start = self.pos;
            let mut end = self.pos + count;
            if start > curlen {
                start = curlen
            }
            if end > curlen {
                end = curlen
            }
            let cnt = end - start;
            self.pos += cnt;
             */
            Ok(Bytes::from(file.data.to_string()))
        }
        .boxed()
    }

    fn seek(&mut self, pos: SeekFrom) -> FsFuture<u64> {
        async move {
            let (start, offset): (u64, i64) = match pos {
                SeekFrom::Start(npos) => {
                    self.pos = npos as usize;
                    return Ok(npos);
                }
                SeekFrom::Current(npos) => (self.pos as u64, npos),
                SeekFrom::End(npos) => {
                    let tree = &*self.tree.lock().await;
                    let node = tree.get_node(self.node_id)?;
                    let file = node.as_file()?;
                    (file.size as u64, npos)
                }
            };
            if offset < 0 {
                if -offset as u64 > start {
                    return Err(Error::new(ErrorKind::InvalidInput, "invalid seek").into());
                }
                self.pos = (start - (-offset as u64)) as usize;
            } else {
                self.pos = (start + offset as u64) as usize;
            }
            Ok(self.pos as u64)
        }
        .boxed()
    }

    fn flush(&mut self) -> FsFuture<()> {
        future::ok(()).boxed()
    }
}

impl DavMetaData for FSEntry {
    fn len(&self) -> u64 {
        self.size
    }

    fn modified(&self) -> FsResult<SystemTime> {
        Ok(self.mtime)
    }

    fn is_dir(&self) -> bool {
        self.is_dir
    }

    fn created(&self) -> FsResult<SystemTime> {
        Ok(self.crtime)
    }
}

impl FSNode {
    fn new_dir(root_id: String) -> FSNode {
        FSNode::Dir(FSDirNode {
            id: root_id,
            crtime: SystemTime::now(),
            mtime: SystemTime::now(),
            props: HashMap::new(),
        })
    }

    // helper to create FSDirEntry from a node.
    fn as_dirent(&self, name: &[u8]) -> FSEntry {
        let (is_dir, size, mtime, crtime) = match self {
            //&FSNode::File(ref file) => (false, file.data.len() as u64, file.mtime, file.crtime),
            &FSNode::File(ref file) => (false, file.size, file.mtime, file.crtime),
            &FSNode::Dir(ref dir) => (true, 0, dir.mtime, dir.crtime),
        };
        FSEntry {
            name: name.to_vec(),
            mtime: mtime,
            crtime: crtime,
            is_dir: is_dir,
            size: size as u64,
        }
    }

    fn update_mtime(&mut self, tm: std::time::SystemTime) {
        match self {
            &mut FSNode::Dir(ref mut d) => d.mtime = tm,
            &mut FSNode::File(ref mut f) => f.mtime = tm,
        }
    }

    fn is_dir(&self) -> bool {
        match self {
            &FSNode::Dir(_) => true,
            &FSNode::File(_) => false,
        }
    }

    fn as_dir(&self) -> FsResult<&FSDirNode> {
        match self {
            &FSNode::Dir(ref n) => Ok(n),
            _ => Err(FsError::Forbidden),
        }
    }

    fn as_file(&self) -> FsResult<&FSFileNode> {
        match self {
            &FSNode::File(ref n) => Ok(n),
            _ => Err(FsError::Forbidden),
        }
    }

    fn as_file_mut(&mut self) -> FsResult<&mut FSFileNode> {
        match self {
            &mut FSNode::File(ref mut n) => Ok(n),
            _ => Err(FsError::Forbidden),
        }
    }

    fn get_props(&self) -> &HashMap<String, DavProp> {
        match self {
            &FSNode::File(ref n) => &n.props,
            &FSNode::Dir(ref d) => &d.props,
        }
    }

    fn get_props_mut(&mut self) -> &mut HashMap<String, DavProp> {
        match self {
            &mut FSNode::File(ref mut n) => &mut n.props,
            &mut FSNode::Dir(ref mut d) => &mut d.props,
        }
    }
}

trait TreeExt {
    fn lookup_segs(&self, segs: Vec<&[u8]>) -> FsResult<u64>;
    fn lookup(&self, path: &[u8]) -> FsResult<u64>;
    fn lookup_parent(&self, path: &[u8]) -> FsResult<u64>;
}

impl TreeExt for Tree {
    fn lookup_segs(&self, segs: Vec<&[u8]>) -> FsResult<u64> {
        let mut node_id = tree::ROOT_ID;
        let mut is_dir = true;
        for seg in segs.into_iter() {
            if !is_dir {
                return Err(FsError::Forbidden);
            }
            if self.get_node(node_id)?.is_dir() {
                node_id = self.get_child(node_id, seg)?;
            } else {
                is_dir = false;
            }
        }
        Ok(node_id)
    }

    fn lookup(&self, path: &[u8]) -> FsResult<u64> {
        self.lookup_segs(path.split(|&c| c == b'/').filter(|s| s.len() > 0).collect())
    }

    // pop the last segment off the path, do a lookup, then
    // check if the result is a directory.
    fn lookup_parent(&self, path: &[u8]) -> FsResult<u64> {
        let mut segs: Vec<&[u8]> = path.split(|&c| c == b'/').filter(|s| s.len() > 0).collect();
        segs.pop();
        let node_id = self.lookup_segs(segs)?;
        if !self.get_node(node_id)?.is_dir() {
            return Err(FsError::Forbidden);
        }
        Ok(node_id)
    }
}

// helper
fn file_name(path: &[u8]) -> Vec<u8> {
    path.split(|&c| c == b'/')
        .filter(|s| s.len() > 0)
        .last()
        .unwrap_or(b"")
        .to_vec()
}
