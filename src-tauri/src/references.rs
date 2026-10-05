//! Reference screenshots for the color-equalizer preview, one file per program profile.
//! Images live next to settings.json instead of inside it, so the settings file stays
//! small; files of removed profiles are only deleted at startup, which keeps an undone
//! removal intact.

use base64::{Engine as _, prelude::BASE64_STANDARD};
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

/// Largest accepted reference image; a 4K PNG screenshot is well below this.
const MAX_IMAGE_BYTES: usize = 25 * 1024 * 1024;
/// Accepted image types: data-URL MIME type and file extension.
const FORMATS: [(&str, &str); 4] = [
    ("image/png", "png"),
    ("image/jpeg", "jpg"),
    ("image/webp", "webp"),
    ("image/bmp", "bmp"),
];

#[derive(Debug, thiserror::Error)]
pub(crate) enum ReferenceError {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("failed to access the reference image: {0}")]
    Io(#[from] std::io::Error),
}

/// Profile IDs become file names, so only UUID-like IDs are accepted.
fn file_stem(profile_id: &str) -> Result<&str, ReferenceError> {
    let valid = !profile_id.is_empty()
        && profile_id.len() <= 64
        && profile_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if valid {
        Ok(profile_id)
    } else {
        Err(ReferenceError::Invalid(
            "this profile cannot store a reference image",
        ))
    }
}

fn existing(directory: &Path, stem: &str) -> impl Iterator<Item = (PathBuf, &'static str)> {
    FORMATS.into_iter().filter_map(move |(mime, extension)| {
        let path = directory.join(format!("{stem}.{extension}"));
        path.is_file().then_some((path, mime))
    })
}

/// Returns the stored image as a data URL, or `None` when the profile has none.
pub(crate) fn load(directory: &Path, profile_id: &str) -> Result<Option<String>, ReferenceError> {
    let stem = file_stem(profile_id)?;
    let Some((path, mime)) = existing(directory, stem).next() else {
        return Ok(None);
    };
    let bytes = fs::read(path)?;
    Ok(Some(format!(
        "data:{mime};base64,{}",
        BASE64_STANDARD.encode(bytes)
    )))
}

/// Stores a `data:image/...;base64,` URL, replacing any earlier image of the profile.
pub(crate) fn save(
    directory: &Path,
    profile_id: &str,
    data_url: &str,
) -> Result<(), ReferenceError> {
    let stem = file_stem(profile_id)?;
    let (header, payload) = data_url
        .split_once(',')
        .ok_or(ReferenceError::Invalid("the image could not be read"))?;
    let (_, extension) = FORMATS
        .into_iter()
        .find(|(mime, _)| header == format!("data:{mime};base64"))
        .ok_or(ReferenceError::Invalid(
            "use a PNG, JPEG, WebP or BMP image",
        ))?;
    if payload.len() / 4 * 3 > MAX_IMAGE_BYTES {
        return Err(ReferenceError::Invalid("the image is larger than 25 MB"));
    }
    let bytes = BASE64_STANDARD
        .decode(payload)
        .map_err(|_| ReferenceError::Invalid("the image could not be read"))?;
    fs::create_dir_all(directory)?;
    let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    delete(directory, profile_id)?;
    temporary
        .persist(directory.join(format!("{stem}.{extension}")))
        .map_err(|error| ReferenceError::Io(error.error))?;
    Ok(())
}

/// Removes the profile's image if it has one.
pub(crate) fn delete(directory: &Path, profile_id: &str) -> Result<(), ReferenceError> {
    let stem = file_stem(profile_id)?;
    for (path, _) in existing(directory, stem).collect::<Vec<_>>() {
        fs::remove_file(path)?;
    }
    Ok(())
}

/// Deletes images whose profile no longer exists; failures only skip that file.
pub(crate) fn remove_orphans<'a>(directory: &Path, profile_ids: impl Iterator<Item = &'a str>) {
    let keep: HashSet<&str> = profile_ids.collect();
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let orphan = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| !keep.contains(stem));
        if orphan && let Err(error) = fs::remove_file(&path) {
            tracing::warn!(%error, path = %path.display(), "failed to remove an unused reference image");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &str = "data:image/png;base64,iVBORw0KGgo=";

    #[test]
    fn images_round_trip_and_replace_earlier_formats() {
        let directory = tempfile::tempdir().unwrap();
        assert_eq!(load(directory.path(), "game").unwrap(), None);
        save(directory.path(), "game", "data:image/jpeg;base64,/9j/").unwrap();
        save(directory.path(), "game", PNG).unwrap();
        assert_eq!(
            load(directory.path(), "game").unwrap().as_deref(),
            Some(PNG)
        );
        assert!(!directory.path().join("game.jpg").exists());
        delete(directory.path(), "game").unwrap();
        assert_eq!(load(directory.path(), "game").unwrap(), None);
    }

    #[test]
    fn unsafe_ids_and_unsupported_data_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        for id in ["", "../settings", "a/b", "C:game"] {
            assert!(save(directory.path(), id, PNG).is_err(), "{id}");
        }
        for url in [
            "not a url",
            "data:text/plain;base64,aGk=",
            "data:image/png;base64,!!",
        ] {
            assert!(save(directory.path(), "game", url).is_err(), "{url}");
        }
    }

    #[test]
    fn startup_cleanup_keeps_images_of_existing_profiles() {
        let directory = tempfile::tempdir().unwrap();
        save(directory.path(), "kept", PNG).unwrap();
        save(directory.path(), "removed", PNG).unwrap();
        remove_orphans(directory.path(), ["kept"].into_iter());
        assert!(load(directory.path(), "kept").unwrap().is_some());
        assert!(load(directory.path(), "removed").unwrap().is_none());
    }
}
