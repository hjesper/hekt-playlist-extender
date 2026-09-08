use encoding_rs::{UTF_16BE, UTF_16LE};
use serde::Serialize;
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::Path,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Could not read playlist: {0}")]
    Io(#[from] std::io::Error),
    #[error("The file has no header row")]
    MissingHeaders,
    #[error("Required artist and title columns were not found")]
    RequiredColumns,
    #[error("Could not parse row {row}: {message}")]
    Csv { row: usize, message: String },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Track {
    pub id: i64,
    pub row_number: i64,
    pub artist: String,
    pub title: String,
    pub version: Option<String>,
    pub label: Option<String>,
    pub bpm: Option<f64>,
    pub key: Option<String>,
    pub duration: Option<String>,
    pub selected: bool,
    pub match_status: Option<String>,
    pub source_url: Option<String>,
    pub original_fields: BTreeMap<String, String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub name: String,
    pub encoding: String,
    pub delimiter: String,
    pub headers: Vec<String>,
    pub tracks: Vec<Track>,
    pub warnings: Vec<String>,
}

pub fn parse_playlist(path: &Path, name: &str) -> Result<ImportPreview, ParseError> {
    let bytes = fs::read(path)?;
    let (text, encoding) = decode(&bytes);
    let delimiter = detect_delimiter(&text);
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(text.as_bytes());
    let headers = reader
        .headers()
        .map_err(|_| ParseError::MissingHeaders)?
        .iter()
        .map(clean)
        .collect::<Vec<_>>();
    if headers.is_empty() {
        return Err(ParseError::MissingHeaders);
    }
    let normalized = headers
        .iter()
        .enumerate()
        .map(|(i, h)| (normalize(h), i))
        .collect::<HashMap<_, _>>();
    let artist = find(&normalized, &["artist", "artists"]);
    let title = find(&normalized, &["title", "tracktitle", "track"]);
    let (artist, title) = match artist.zip(title) {
        Some(v) => v,
        None => return Err(ParseError::RequiredColumns),
    };
    let version = find(&normalized, &["remixer", "version", "mix"]);
    let label = find(&normalized, &["label"]);
    let bpm = find(&normalized, &["bpm"]);
    let key = find(&normalized, &["key"]);
    let duration = find(&normalized, &["time", "duration"]);
    let mut tracks = Vec::new();
    let mut warnings = Vec::new();
    for (index, row) in reader.records().enumerate() {
        let row = row.map_err(|e| ParseError::Csv {
            row: index + 2,
            message: e.to_string(),
        })?;
        let a = field(&row, artist).unwrap_or_default();
        let raw_title = field(&row, title).unwrap_or_default();
        let explicit_version = version.and_then(|i| field_opt(&row, i));
        let (t, inferred_version) = if explicit_version.is_none() {
            split_title_version(&raw_title)
        } else {
            (raw_title, None)
        };
        if a.is_empty() && t.is_empty() {
            continue;
        }
        if a.is_empty() || t.is_empty() {
            warnings.push(format!("Row {} has a missing artist or title", index + 2));
        }
        tracks.push(Track {
            id: 0,
            row_number: (index + 1) as i64,
            artist: a,
            title: t,
            version: explicit_version.or(inferred_version),
            label: label.and_then(|i| field_opt(&row, i)),
            bpm: bpm
                .and_then(|i| field_opt(&row, i))
                .and_then(|v| v.replace(',', ".").parse().ok()),
            key: key.and_then(|i| field_opt(&row, i)),
            duration: duration.and_then(|i| field_opt(&row, i)),
            selected: false,
            match_status: None,
            source_url: None,
            original_fields: headers
                .iter()
                .enumerate()
                .map(|(column, header)| (header.clone(), field(&row, column).unwrap_or_default()))
                .collect(),
        });
    }
    Ok(ImportPreview {
        name: name.into(),
        encoding,
        delimiter: match delimiter {
            b'\t' => "Tab",
            b';' => "Semicolon",
            _ => "Comma",
        }
        .into(),
        headers,
        tracks,
        warnings,
    })
}
fn decode(bytes: &[u8]) -> (String, String) {
    if bytes.starts_with(&[0xff, 0xfe]) {
        let (s, _, _) = UTF_16LE.decode(&bytes[2..]);
        (s.into_owned(), "UTF-16 LE".into())
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        let (s, _, _) = UTF_16BE.decode(&bytes[2..]);
        (s.into_owned(), "UTF-16 BE".into())
    } else {
        (
            String::from_utf8_lossy(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
                .into_owned(),
            "UTF-8".into(),
        )
    }
}
fn detect_delimiter(text: &str) -> u8 {
    let line = text.lines().next().unwrap_or("");
    [
        (b'\t', line.matches('\t').count()),
        (b';', line.matches(';').count()),
        (b',', line.matches(',').count()),
    ]
    .into_iter()
    .max_by_key(|(_, n)| *n)
    .map(|(d, _)| d)
    .unwrap_or(b'\t')
}
fn clean(v: &str) -> String {
    v.trim_matches('\u{feff}').trim().to_string()
}
fn normalize(v: &str) -> String {
    v.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
fn find(map: &HashMap<String, usize>, names: &[&str]) -> Option<usize> {
    names.iter().find_map(|n| map.get(*n).copied())
}
fn field(row: &csv::StringRecord, i: usize) -> Option<String> {
    row.get(i).map(clean)
}
fn field_opt(row: &csv::StringRecord, i: usize) -> Option<String> {
    field(row, i).filter(|v| !v.is_empty())
}

fn split_title_version(title: &str) -> (String, Option<String>) {
    let Some(open) = title.rfind('(') else {
        return (title.to_string(), None);
    };
    if !title.ends_with(')') || open == 0 {
        return (title.to_string(), None);
    }
    let candidate = title[open + 1..title.len() - 1].trim();
    let normalized = candidate.to_lowercase();
    let version_markers = [
        "mix", "remix", "edit", "dub", "version", "rework", "bootleg", "radio", "extended",
    ];
    let looks_like_version = version_markers.iter().any(|marker| {
        normalized
            .split_whitespace()
            .any(|word| word.trim_matches(|c: char| !c.is_alphanumeric()) == *marker)
    });
    let base = title[..open].trim_end();
    if !looks_like_version || base.is_empty() || candidate.is_empty() {
        return (title.to_string(), None);
    }
    (base.to_string(), Some(candidate.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn fixture(bytes: &[u8]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(bytes).unwrap();
        f
    }
    #[test]
    fn parses_rekordbox_tab_export() {
        let f = fixture(
            "#\tArtist\tTrack Title\tRemixer\tBPM\tKey\n1\tBicep\tGlue\tOriginal Mix\t129.0\tAm\n"
                .as_bytes(),
        );
        let p = parse_playlist(f.path(), "Club").unwrap();
        assert_eq!(p.delimiter, "Tab");
        assert_eq!(p.tracks[0].artist, "Bicep");
        assert_eq!(p.tracks[0].version.as_deref(), Some("Original Mix"));
    }
    #[test]
    fn parses_utf16_and_unicode() {
        let source = "Artist\tTitle\nBjörk\tJóga\n";
        let mut bytes = vec![0xff, 0xfe];
        for unit in source.encode_utf16() {
            bytes.extend(unit.to_le_bytes())
        }
        let p = parse_playlist(fixture(&bytes).path(), "Unicode").unwrap();
        assert_eq!(p.encoding, "UTF-16 LE");
        assert_eq!(p.tracks[0].artist, "Björk");
    }
    #[test]
    fn refuses_unknown_required_columns() {
        let f = fixture(b"Name\tTempo\nTrack\t120\n");
        assert!(matches!(
            parse_playlist(f.path(), "Bad"),
            Err(ParseError::RequiredColumns)
        ));
    }

    #[test]
    fn parses_semicolon_exports_and_decimal_commas() {
        let f = fixture(b"Artist;Title;BPM;Label\nRobyn;Honey;\"122,5\";Konichiwa\n");
        let p = parse_playlist(f.path(), "Semicolon").unwrap();
        assert_eq!(p.delimiter, "Semicolon");
        assert_eq!(p.tracks[0].bpm, Some(122.5));
        assert_eq!(p.tracks[0].label.as_deref(), Some("Konichiwa"));
    }

    #[test]
    fn retains_partial_rows_and_reports_them() {
        let f = fixture(b"Artist\tTrack Title\n\tUntitled tool\n");
        let p = parse_playlist(f.path(), "Partial").unwrap();
        assert_eq!(p.tracks.len(), 1);
        assert_eq!(p.warnings, vec!["Row 2 has a missing artist or title"]);
    }

    #[test]
    fn extracts_only_version_like_title_suffixes() {
        let f = fixture(
            "#\tBPM\tTrack Title\tArtist\n1\t140.00\tRich Baby Daddy (DJ Slugo Remix)\tDJ Slugo\n2\t145.00\tThe Question (Question One)\tDJ Stardust\n3\t132.00\tRaw (Extended Mix)\tMPH\n".as_bytes(),
        );
        let p = parse_playlist(f.path(), "Versions").unwrap();
        assert_eq!(p.tracks[0].title, "Rich Baby Daddy");
        assert_eq!(p.tracks[0].version.as_deref(), Some("DJ Slugo Remix"));
        assert_eq!(p.tracks[1].title, "The Question (Question One)");
        assert_eq!(p.tracks[1].version, None);
        assert_eq!(p.tracks[2].version.as_deref(), Some("Extended Mix"));
        assert_eq!(
            p.tracks[0].original_fields["Track Title"],
            "Rich Baby Daddy (DJ Slugo Remix)"
        );
    }
}
