//! Media locations as interchange formats write them: `file:` URLs, or bare
//! paths relative to the file that names them.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The file `url` names: a `file:` URL, decoded, or a bare path. Other
/// schemes name nothing on disk.
pub(crate) fn path_of_url(url: &str) -> Option<PathBuf> {
    let Some(rest) = url.strip_prefix("file:") else {
        return (!url.is_empty() && !url.contains("://")).then(|| PathBuf::from(url));
    };
    let rest = match rest.strip_prefix("//") {
        // `file:///clip.mov` and `file://localhost/clip.mov` name a local
        // path; `file://server/share/clip.mov` names a network share.
        Some(after) if after.starts_with('/') => after,
        Some(after) => match after
            .strip_prefix("localhost")
            .filter(|path| path.starts_with('/'))
        {
            Some(path) => path,
            None => rest,
        },
        None => rest,
    };
    let decoded = percent_decode(rest)?;
    // `file:///C:/clip.mov` names `C:/clip.mov`.
    let bytes = decoded.as_bytes();
    let drive =
        bytes.len() > 2 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':';
    let path = if drive {
        &decoded[1..]
    } else {
        decoded.as_str()
    };
    (!path.is_empty()).then(|| PathBuf::from(path))
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = text.get(index + 1..index + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// `path` as a `file:` URL when it is absolute, or as written when it is
/// relative.
pub(crate) fn url_of(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    if !path.is_absolute() {
        return text;
    }
    let mut url = String::with_capacity(text.len() + 8);
    url.push_str("file://");
    if !text.starts_with('/') {
        url.push('/');
    }
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~:".contains(&byte) {
            url.push(char::from(byte));
        } else {
            // Writing to a `String` cannot fail.
            let _ = write!(url, "%{byte:02X}");
        }
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_decode_to_paths() {
        let cases = [
            ("file:///media/a%20b.mov", Some("/media/a b.mov")),
            ("file://localhost/media/c.mov", Some("/media/c.mov")),
            ("file:/media/d.mov", Some("/media/d.mov")),
            ("file:///C:/clips/e.mov", Some("C:/clips/e.mov")),
            ("file://server/share/f.mov", Some("//server/share/f.mov")),
            ("clips/g.mov", Some("clips/g.mov")),
            ("https://example.com/h.mov", None),
            ("file:///bad%zz.mov", None),
            ("file:///cut%2", None),
            ("", None),
        ];
        for (url, path) in cases {
            assert_eq!(path_of_url(url), path.map(PathBuf::from), "{url}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn absolute_paths_round_trip_through_urls() {
        for path in ["/media/a b#1%.mov", "/media/ü/clip.mov", "/x"] {
            let url = url_of(Path::new(path));
            assert!(url.starts_with("file:///"), "{url}");
            assert!(!url.contains(' ') && !url.contains('#'), "{url}");
            assert_eq!(path_of_url(&url), Some(PathBuf::from(path)));
        }
        assert_eq!(url_of(Path::new("rel/clip.mov")), "rel/clip.mov");
    }
}
