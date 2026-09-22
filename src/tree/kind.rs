//! What kind of thing a tree row is, for colouring.
//!
//! Pure and extension-driven: the tree already knows whether a path is a
//! directory, so nothing here touches the filesystem.

use std::path::Path;

/// The colour class of a tree row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileKind {
    /// The synthetic `../` row: navigation, not content.
    ParentRow,
    Directory,
    /// Something you can execute: a script or a source file.
    Runnable,
    /// Prose and markup, the brain's own content.
    Note,
    /// Structured data and configuration.
    Data,
    /// Rendered documents and media you open in an application.
    Rendered,
    /// Compressed or packaged blobs.
    Archive,
    /// Anything unrecognised.
    Other,
}

/// Which colour class `path` belongs to.
///
/// `is_dir` wins over the extension, because a directory called `notes.md` is
/// still a directory. A name with no extension is a note: a brain holds
/// `README`, `LICENSE`, and plenty of extensionless prose. Everything else is
/// decided by the lowercased extension alone.
pub(crate) fn classify(path: &Path, is_dir: bool) -> FileKind {
    if is_dir {
        return FileKind::Directory;
    }
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return FileKind::Note;
    };
    match extension.to_ascii_lowercase().as_str() {
        "sh" | "zsh" | "bash" | "fish" | "ps1" | "bat" | "cmd" | "py" | "rb" | "pl" | "lua"
        | "js" | "mjs" | "cjs" | "ts" | "rs" | "go" | "java" | "kt" | "swift" | "c" | "cpp"
        | "cc" | "h" | "hpp" | "exe" => FileKind::Runnable,
        "md" | "markdown" | "mdx" | "mdc" | "mdown" | "txt" | "text" | "rst" | "org" | "adoc"
        | "asciidoc" | "tex" | "ltx" | "html" | "htm" => FileKind::Note,
        "json" | "jsonc" | "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf" | "env" | "xml"
        | "csv" | "tsv" | "log" | "sql" | "css" | "scss" | "sass" => FileKind::Data,
        "pdf" | "docx" | "doc" | "pptx" | "ppt" | "xlsx" | "xls" | "epub" | "png" | "jpg"
        | "jpeg" | "gif" | "svg" | "webp" | "heic" | "bmp" | "tiff" | "mp4" | "mov" | "avi"
        | "mkv" | "webm" | "mp3" | "wav" | "flac" | "m4a" | "aac" | "ogg" => FileKind::Rendered,
        "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" | "dmg" | "pkg" | "iso" => {
            FileKind::Archive
        }
        _ => FileKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_wins_over_its_extension() {
        assert_eq!(
            classify(Path::new("/x/notes.md"), true),
            FileKind::Directory,
            "a directory named like a note is still a directory"
        );
    }

    #[test]
    fn a_name_with_no_extension_reads_as_a_note() {
        assert_eq!(classify(Path::new("/x/README"), false), FileKind::Note);
        assert_eq!(classify(Path::new("/x/LICENSE"), false), FileKind::Note);
    }

    #[test]
    fn each_kind_has_a_representative_extension() {
        assert_eq!(
            classify(Path::new("/x/build.sh"), false),
            FileKind::Runnable
        );
        assert_eq!(classify(Path::new("/x/plan.md"), false), FileKind::Note);
        assert_eq!(classify(Path::new("/x/env.json"), false), FileKind::Data);
        assert_eq!(
            classify(Path::new("/x/report.pdf"), false),
            FileKind::Rendered
        );
        assert_eq!(
            classify(Path::new("/x/backup.zip"), false),
            FileKind::Archive
        );
    }

    #[test]
    fn extensions_match_case_insensitively() {
        assert_eq!(classify(Path::new("/x/PLAN.MD"), false), FileKind::Note);
        assert_eq!(
            classify(Path::new("/x/IMAGE.PNG"), false),
            FileKind::Rendered
        );
    }

    #[test]
    fn an_unrecognised_extension_is_other() {
        assert_eq!(classify(Path::new("/x/thing.qqq"), false), FileKind::Other);
    }

    #[test]
    fn a_dotfile_with_an_extension_classifies_by_that_extension() {
        assert_eq!(
            classify(Path::new("/x/.eslintrc.json"), false),
            FileKind::Data
        );
    }

    #[test]
    fn a_bare_dotfile_reads_as_a_note() {
        // `Path::extension` returns `None` for `.gitignore`: a leading-dot
        // name is all stem. Rather than re-splitting the name to fight that,
        // the rule stands (an extensionless name is prose) and a bare dotfile
        // lands with `README`.
        assert_eq!(classify(Path::new("/x/.gitignore"), false), FileKind::Note);
    }
}
