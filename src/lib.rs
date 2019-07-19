extern crate util;
#[macro_use]
extern crate log;
use std::{
    collections::{
        HashMap,
    },
    fmt,
    io::{
        self,
        Cursor,
        Read,
    },
    path::{
        self,
        PathBuf,
    },
};

use sha2::{Digest, Sha256};

const BUFFER_SIZE: usize = 1024;

pub trait ReadFile {
    fn read_file<P: AsRef<path::Path>>(&mut self, file_path: P) -> io::Result<&Box<[u8]>>;
}

pub trait PathMapper {
    fn map<P: AsRef<path::Path>>(&mut self, file_name: P) -> Box<path::Path>;
}

impl fmt::Debug for GemFileSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ResourceLoader Path: {:#?}", self.root)
    }
}

impl fmt::Display for GemFileSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ResourceLoader Path: {:#?}", self.root)
    }
}

pub struct Cache {
    // storing the pointer of the file content: [T] in a HashMap
    pub sha2_map: HashMap<PathBuf, Box<[u8]>>,
    pub content_map: HashMap<PathBuf, Box<[u8]>>,
}

impl Cache {
    pub fn new() -> Cache {
        Cache {
            content_map: HashMap::new(),
            sha2_map: HashMap::new(),
        }
    }
    pub fn store_file(&mut self, key: PathBuf, content_ptr: Box<[u8]>) {
        let hash = process_sha256::<Sha256, _>(&mut Cursor::new(&content_ptr));
        debug!("{:#?}",key);
        debug!("{:#?}",hash);
        self.sha2_map.insert(key.clone(), hash.into_boxed_slice());
        self.content_map.insert(key.clone(), content_ptr);
    }
}
/// two purposes of gfs:
/// read, cache, and manage file in the heap, regardless of file location
/// map relative file path to absolute path for external usage 
pub struct GemFileSystem {
    pub cache: Cache,
    pub root: path::PathBuf,
}

pub enum FileSyncState {
    HashMatch,
    HashUnmatch,
}

impl GemFileSystem {
    pub fn new<P: AsRef<path::Path>>(root: P) -> GemFileSystem {
        GemFileSystem {
            cache: Cache::new(),
            root: root.as_ref().to_path_buf(),
        }
    }
    
    /// load and return file into self.cache
    // since Box<[u8]> holds the ownership of the file content, we can only return
    // a reference to it.
    pub fn fetch_and_cache_file<P: AsRef<path::Path>>(&mut self, file_path: P)
        -> Option<&Box<[u8]>> {
        let mut absolute_path = self.root.clone();
        absolute_path.push(file_path.as_ref().clone());
        debug!("{}", absolute_path.display());
        
        match absolute_path.exists() & &absolute_path.is_file() {
            true => {
                let file_ptr = util::load_file_as_u8(&absolute_path);
                self.cache.store_file(file_path.as_ref().to_path_buf(), file_ptr);
                // now file_ptr is moved, the ownership is transferred to Cache
                self.cache.content_map.get(file_path.as_ref())
            }
            false => {
                None
            }
        }
    }
    
    pub fn check_for_sync_file<P: AsRef<path::Path>>(&mut self, file_path: P) -> io::Result<FileSyncState> {
        let if_file_in_cache = self.cache.sha2_map.contains_key(file_path.as_ref());
        match if_file_in_cache {
            false => {
                let mut err = String::from("Resource not found in cache, cannot check for \
                synchronicity");
                err.push_str(&format!("{:#?}", file_path.as_ref().to_path_buf()));
                Err(io::Error::new(io::ErrorKind::Other, err))
            }
            true => {
                let mut absolute_path = self.root.clone();
                absolute_path.push(file_path.as_ref().clone());
                debug!("{}",absolute_path.display());
                
                if absolute_path.exists() && absolute_path.is_file() {
                    let disk_file = util::load_file_as_u8(&absolute_path);
                    let disk_file_hash = process_sha256::<Sha256, _>(&mut Cursor::new(disk_file));
                    let cached_file_hash = self.cache.sha2_map.get(file_path.as_ref()).unwrap();
                    let diff_count = disk_file_hash
                        .iter()
                        .zip(cached_file_hash.iter())
                        .filter(|&
                                 (a, b)| a
                            != b).count();
                    if diff_count == 0 {
                        return Ok(FileSyncState::HashMatch);
                    } else {
                        return Ok(FileSyncState::HashUnmatch);
                    }
                } else {
                    let mut err = String::from("Resource not found at path: ");
                    err.push_str(&format!("{:#?}", file_path.as_ref().to_path_buf()));
                    Err(io::Error::new(io::ErrorKind::Other, err))
                }
            }
        }
    }
}

impl ReadFile for GemFileSystem {
    /// format: gfs.read_file(&"models/chest.obj")
    /// or anything, typed AsRef<path::Path>, with a string formatted as "models/chest.obj" or like
    fn read_file<P: AsRef<path::Path>>(&mut self, file_path: P) -> io::Result<&Box<[u8]>> {
        let if_file_in_cache = self.cache.content_map.contains_key(file_path.as_ref());
        match if_file_in_cache {
            false => {
                if let Some(file_ptr) = self.fetch_and_cache_file(&file_path) {
                    return Ok(file_ptr);
                } else {
                    // if reach here, it means it cannot find the file both in cache or in disk
                    let mut err = String::from("Resource not found at path: ");
                    err.push_str(&format!("{:#?}", file_path.as_ref()));
                    Err(io::Error::new(io::ErrorKind::Other, err))
                }
            }
            true => {
                return Ok(self.fetch_and_cache_file(&file_path).unwrap());
            }
        }
    }
}

impl PathMapper for GemFileSystem {
    fn map<P: AsRef<path::Path>>(&mut self, file_path: P) -> Box<path::Path> {
        let mut absolute_path = self.root.clone();
        absolute_path.push(file_path.as_ref().clone());
        absolute_path.into_boxed_path()
    }
    
}
fn process_sha256<D: Digest + Default, R: Read>(reader: &mut R) -> Vec<u8> {
    let mut sh = D::default();
    let mut buffer = [0u8; BUFFER_SIZE];
    loop {
        let n = match reader.read(&mut buffer) {
            Ok(n) => n,
            Err(_) => panic!(),
        };
        sh.input(&buffer[..n]);
        if n == 0 || n < BUFFER_SIZE {
            break;
        }
    }
    sh.result().to_vec()
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
