//! The program out of a checked archive: the one file the canary workflow packs
//! (`ssheesh-preview`, or `ssheesh-preview.exe` in Windows' zip). Nothing else in the archive
//! is written anywhere.

use std::io::Read;

use super::manifest::{Packing, MOST_BYTES};

/// The program in `archive`, by its name.
pub fn program(archive: &[u8], packing: Packing, name: &str) -> Result<Vec<u8>, String> {
    let bytes = match packing {
        Packing::TarGz => from_tar_gz(archive, name)?,
        Packing::Zip => from_zip(archive, name)?,
    };
    if bytes.is_empty() {
        return Err(format!("{name} is empty in the archive"));
    }
    Ok(bytes)
}

fn read_capped(r: impl Read, size_hint: u64) -> Result<Vec<u8>, String> {
    if size_hint > MOST_BYTES {
        return Err("the program in the archive is too big".into());
    }
    let mut out = Vec::with_capacity(size_hint as usize);
    r.take(MOST_BYTES + 1).read_to_end(&mut out).map_err(|e| format!("the archive doesn't unpack: {e}"))?;
    if out.len() as u64 > MOST_BYTES {
        return Err("the program in the archive is too big".into());
    }
    Ok(out)
}

fn from_tar_gz(archive: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    for entry in tar.entries().map_err(|e| format!("the archive doesn't unpack: {e}"))? {
        let entry = entry.map_err(|e| format!("the archive doesn't unpack: {e}"))?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        // The program at the top of the archive, as a plain file; links and folders are skipped.
        let top = path.components().filter(|c| !matches!(c, std::path::Component::CurDir)).count() == 1;
        if top && path.file_name().is_some_and(|n| n == name) && entry.header().entry_type().is_file() {
            let size = entry.header().size().unwrap_or(0);
            return read_capped(entry, size);
        }
    }
    Err(format!("{name} isn't in the archive"))
}

fn from_zip(archive: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive)).map_err(|e| format!("the archive doesn't unpack: {e}"))?;
    let file = zip.by_name(name).map_err(|_| format!("{name} isn't in the archive"))?;
    if !file.is_file() {
        return Err(format!("{name} isn't a file in the archive"));
    }
    let size = file.size();
    read_capped(file, size)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// A tar.gz as the workflow packs one: `files` at the top, in order.
    pub(crate) fn tar_gz(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        for (name, bytes) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, name, *bytes).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    pub(crate) fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in files {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn the_program_comes_out_of_either_archive() {
        let tar = tar_gz(&[("README", b"x"), ("ssheesh-preview", b"\x7fELF made-up program")]);
        assert_eq!(program(&tar, Packing::TarGz, "ssheesh-preview").unwrap(), b"\x7fELF made-up program");
        let zip = zip_of(&[("ssheesh-preview.exe", b"MZ made-up program")]);
        assert_eq!(program(&zip, Packing::Zip, "ssheesh-preview.exe").unwrap(), b"MZ made-up program");
    }

    #[test]
    fn an_archive_without_it_gives_nothing() {
        let nested = tar_gz(&[("bin/ssheesh-preview", b"made-up")]);
        assert!(program(&nested, Packing::TarGz, "ssheesh-preview").is_err());
        let empty = tar_gz(&[("ssheesh-preview", b"")]);
        assert!(program(&empty, Packing::TarGz, "ssheesh-preview").is_err());
        assert!(program(b"not an archive", Packing::TarGz, "ssheesh-preview").is_err());
        assert!(program(b"not an archive", Packing::Zip, "ssheesh-preview.exe").is_err());
        let zip = zip_of(&[("other.exe", b"MZ")]);
        assert!(program(&zip, Packing::Zip, "ssheesh-preview.exe").is_err());
    }
}
