//! Curated extension → category table (not open-ended OS registry).

use crate::category::Category;

/// Extract the lowercased extension without the leading dot, if any.
pub fn extension_of(path: &str) -> Option<String> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path).trim();
    if name.is_empty() || name == "." || name == ".." {
        return None;
    }
    // Ignore leading-dot-only names (".gitignore" style — treat as no useful type).
    let name_lower = name.to_ascii_lowercase();
    let (stem, ext) = name_lower.rsplit_once('.')?;
    if stem.is_empty() || ext.is_empty() {
        return None;
    }
    Some(ext.to_string())
}

/// Map extension to category. Includes **`.msg` → email**.
pub fn category_from_extension(path: &str) -> Option<Category> {
    let ext = extension_of(path)?;
    Some(match ext.as_str() {
        // Email / messaging
        "eml" | "msg" | "emlx" => Category::Email,
        // Calendar / contact
        "ics" | "ical" | "ifb" => Category::Calendar,
        "vcf" | "vcard" => Category::Contact,
        // Documents
        "docx" | "docm" | "dotx" | "dotm" | "doc" | "dot" | "rtf" | "txt" | "md" | "markdown"
        | "odt" | "wpd" | "tex" => Category::Document,
        // Spreadsheets
        "xlsx" | "xlsm" | "xltx" | "xltm" | "xls" | "xlt" | "csv" | "tsv" | "ods" => {
            Category::Spreadsheet
        }
        // Presentations
        "pptx" | "pptm" | "potx" | "potm" | "ppsx" | "ppsm" | "ppt" | "pot" | "pps" | "odp" => {
            Category::Presentation
        }
        // PDF
        "pdf" => Category::Pdf,
        // Images
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "tif" | "tiff" | "webp" | "heic" | "heif"
        | "ico" | "svg" | "jfif" | "raw" | "cr2" | "nef" => Category::Image,
        // Multimedia
        "mp3" | "wav" | "flac" | "aac" | "ogg" | "m4a" | "wma" | "mp4" | "mkv" | "avi" | "mov"
        | "wmv" | "webm" | "m4v" | "mpeg" | "mpg" | "3gp" => Category::Multimedia,
        // Archives / containers (non-OOXML)
        "zip" | "7z" | "rar" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "cab" | "iso" | "jar"
        | "war" | "ear" | "zst" | "lz" | "lzma" => Category::Archive,
        // Database
        "sqlite" | "sqlite3" | "db" | "mdb" | "accdb" | "dbf" | "sql" => Category::Database,
        // Logs
        "log" | "evt" | "evtx" => Category::Log,
        // Executable / scripts treated as exec noise
        "exe" | "dll" | "sys" | "bat" | "cmd" | "ps1" | "msi" | "com" | "scr" | "cpl" | "ocx"
        | "vbs" | "js" | "jse" | "wsf" | "sh" | "bash" | "so" | "dylib" => Category::Executable,
        // PST
        "pst" | "ost" => Category::Pst,
        // Mobile thin
        "ipa" | "apk" | "ab" => Category::Mobile,
        // Chat thin
        "chat" => Category::Chat,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn representative_extensions() {
        assert_eq!(
            category_from_extension("memo.docx"),
            Some(Category::Document)
        );
        assert_eq!(
            category_from_extension(r"C:\x\sheet.XLSX"),
            Some(Category::Spreadsheet)
        );
        assert_eq!(
            category_from_extension("deck.pptx"),
            Some(Category::Presentation)
        );
        assert_eq!(category_from_extension("a.pdf"), Some(Category::Pdf));
        assert_eq!(category_from_extension("x.png"), Some(Category::Image));
        assert_eq!(category_from_extension("data.zip"), Some(Category::Archive));
        assert_eq!(category_from_extension("a.exe"), Some(Category::Executable));
        assert_eq!(
            category_from_extension("meet.ics"),
            Some(Category::Calendar)
        );
        assert_eq!(category_from_extension("box.pst"), Some(Category::Pst));
        assert_eq!(
            category_from_extension("nums.csv"),
            Some(Category::Spreadsheet)
        );
        assert_eq!(
            category_from_extension("notes.txt"),
            Some(Category::Document)
        );
        assert_eq!(category_from_extension("mail.msg"), Some(Category::Email));
        assert_eq!(
            category_from_extension("legacy.doc"),
            Some(Category::Document)
        );
    }
}
