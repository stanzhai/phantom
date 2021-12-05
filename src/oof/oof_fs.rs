use std::collections::HashMap;
use std::io::{Error, ErrorKind, SeekFrom};
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::Mutex;

use webdav_handler::davpath::DavPath;
use webdav_handler::fs::{
    DavDirEntry, DavFile, DavFileSystem, DavMetaData, DavProp, FsError, FsFuture, FsResult,
    FsStream, OpenOptions, ReadDirMeta,
};

use crate::{tree, ClientOof};
use bytes::{Buf, Bytes};
use futures::{
    future,
    future::{FutureExt},
};

type Tree = tree::Tree<Vec<u8>, OofFSNode>;

#[derive(Debug)]
pub struct OofFS {
    client: Arc<ClientOof>,
    tree: Arc<Mutex<Tree>>,
    visited: Arc<Mutex<Vec<u64>>>,
}

#[derive(Debug, Clone)]
enum OofFSNode {
    Dir(OofFSDirNode),
    File(OofFSFileNode),
}

#[derive(Debug, Clone)]
struct OofFSDirNode {
    props: HashMap<String, DavProp>,
    mtime: SystemTime,
    crtime: SystemTime,
}

#[derive(Debug, Clone)]
struct OofFSFileNode {
    props: HashMap<String, DavProp>,
    mtime: SystemTime,
    crtime: SystemTime,
    pickcode: String,
    size: usize,
    data: Vec<u8>,
}

#[derive(Debug, Clone)]
struct OofFSEntry {
    mtime: SystemTime,
    crtime: SystemTime,
    is_dir: bool,
    name: Vec<u8>,
    size: u64,
}

#[derive(Debug)]
struct OofFSFile {
    tree: Arc<Mutex<Tree>>,
    node_id: u64,
    pickcode: String,
    client: Arc<ClientOof>,
    pos: usize,
    append: bool,
}

impl OofFS {
    /// Create a new "OofFS" filesystem.
    pub fn new() -> Box<OofFS> {
        let root = OofFSNode::new_dir();
        Box::new(OofFS {
            client: Arc::new(ClientOof::new()),
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

        let pickcode = match node {
            OofFSNode::File(f) => f.pickcode.to_owned(),
            _ => "".to_string(),
        };

        Ok(Box::new(OofFSFile {
            tree: self.tree.clone(),
            client: self.client.clone(),
            pickcode,
            node_id,
            pos: 0,
            append: options.append,
        }))
    }
}

impl Clone for OofFS {
    fn clone(&self) -> Self {
        OofFS {
            client: self.client.clone(),
            tree: Arc::clone(&self.tree),
            visited: Arc::clone(&self.visited),
        }
    }
}

impl DavFileSystem for OofFS {
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
            if !tree.get_node(node_id)?.is_dir() {
                return Err(FsError::Forbidden);
            }

            let visited = &mut self.visited.lock().await;
            if !visited.iter().any(|id| node_id == *id) {
                let entries = self.client.opendir(node_id).await;
                for entry in entries {
                    let node = if entry.is_file {
                        OofFSNode::File(OofFSFileNode {
                            crtime: entry.ctime,
                            mtime: entry.ctime,
                            pickcode: entry.pickcode,
                            props: HashMap::new(),
                            size: entry.size,
                            data: entry.data.unwrap(),
                        })
                    } else {
                        OofFSNode::Dir(OofFSDirNode {
                            crtime: entry.ctime,
                            mtime: entry.ctime,
                            props: HashMap::new(),
                        })
                    };
                    tree.add_child(entry.id, node_id, entry.name.into_bytes(), node, false);
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

impl DavDirEntry for OofFSEntry {
    fn name(&self) -> Vec<u8> {
        self.name.clone()
    }

    fn metadata<'a>(&'a self) -> FsFuture<Box<dyn DavMetaData>> {
        let meta = (*self).clone();
        Box::pin(future::ok(Box::new(meta) as Box<dyn DavMetaData>))
    }
}

impl DavFile for OofFSFile {
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
            Ok(Bytes::copy_from_slice(&file.data[start..end]))
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

impl DavMetaData for OofFSEntry {
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

impl OofFSNode {
    fn new_dir() -> OofFSNode {
        OofFSNode::Dir(OofFSDirNode {
            crtime: SystemTime::now(),
            mtime: SystemTime::now(),
            props: HashMap::new(),
        })
    }

    // helper to create OofFSDirEntry from a node.
    fn as_dirent(&self, name: &[u8]) -> OofFSEntry {
        let (is_dir, size, mtime, crtime) = match self {
            //&OofFSNode::File(ref file) => (false, file.data.len() as u64, file.mtime, file.crtime),
            &OofFSNode::File(ref file) => (false, file.size, file.mtime, file.crtime),
            &OofFSNode::Dir(ref dir) => (true, 0, dir.mtime, dir.crtime),
        };
        OofFSEntry {
            name: name.to_vec(),
            mtime: mtime,
            crtime: crtime,
            is_dir: is_dir,
            size: size as u64,
        }
    }

    fn update_mtime(&mut self, tm: std::time::SystemTime) {
        match self {
            &mut OofFSNode::Dir(ref mut d) => d.mtime = tm,
            &mut OofFSNode::File(ref mut f) => f.mtime = tm,
        }
    }

    fn is_dir(&self) -> bool {
        match self {
            &OofFSNode::Dir(_) => true,
            &OofFSNode::File(_) => false,
        }
    }

    fn as_file(&self) -> FsResult<&OofFSFileNode> {
        match self {
            &OofFSNode::File(ref n) => Ok(n),
            _ => Err(FsError::Forbidden),
        }
    }

    fn as_file_mut(&mut self) -> FsResult<&mut OofFSFileNode> {
        match self {
            &mut OofFSNode::File(ref mut n) => Ok(n),
            _ => Err(FsError::Forbidden),
        }
    }

    fn get_props(&self) -> &HashMap<String, DavProp> {
        match self {
            &OofFSNode::File(ref n) => &n.props,
            &OofFSNode::Dir(ref d) => &d.props,
        }
    }

    fn get_props_mut(&mut self) -> &mut HashMap<String, DavProp> {
        match self {
            &mut OofFSNode::File(ref mut n) => &mut n.props,
            &mut OofFSNode::Dir(ref mut d) => &mut d.props,
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
