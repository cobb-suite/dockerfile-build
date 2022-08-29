use std::path::Path;

use flate2::{write::GzEncoder, Compression};

pub fn tarball(path: &str, dockerfile_name: &str) -> Result<Vec<u8>, std::io::Error> {
    let path = Path::new(path);

    let enc = GzEncoder::new(Vec::new(), Compression::default());
    let mut tar = tar::Builder::new(enc);
    if path.is_dir() {
        tar.append_dir_all("./", path)?;
    } else {
        tar.append_path_with_name(path, dockerfile_name)?;
    }
    Ok(tar.into_inner().unwrap().finish().unwrap())
}