use std::fmt;

use aegis_ipc::page_url;
use zeroize::Zeroizing;

use crate::{ImportError, MAX_ENTRIES, check_file_size};

pub struct ImportedLogin {
    pub origin: String,
    pub username: String,
    pub password: Zeroizing<String>,
}

impl fmt::Debug for ImportedLogin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImportedLogin")
            .field("origin", &self.origin)
            .field("username", &self.username)
            .field("password", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SkipReason {
    InvalidUrl,
    NonHttps,
}

#[derive(Debug, Default)]
pub struct PasswordImport {
    pub logins: Vec<ImportedLogin>,
    pub skipped: Vec<SkipReason>,
}

pub fn parse_password_csv(bytes: &[u8]) -> Result<PasswordImport, ImportError> {
    check_file_size(bytes.len() as u64)?;
    let mut scanner = CsvScanner { bytes, pos: 0 };
    let header = scanner.next_row()?.ok_or(ImportError::MissingColumns)?;
    let (url_index, username_index, password_index) = column_indices(&header)?;
    let mut result = PasswordImport::default();
    while result.logins.len() + result.skipped.len() < MAX_ENTRIES {
        let Some(row) = scanner.next_row()? else {
            break;
        };
        let Some(raw_url) = row.get(url_index) else {
            result.skipped.push(SkipReason::InvalidUrl);
            continue;
        };
        let Ok(raw_url) = std::str::from_utf8(raw_url) else {
            result.skipped.push(SkipReason::InvalidUrl);
            continue;
        };
        let Some(origin) = origin(raw_url) else {
            result.skipped.push(SkipReason::InvalidUrl);
            continue;
        };
        if !origin.starts_with("https://") {
            result.skipped.push(SkipReason::NonHttps);
            continue;
        }
        let username = row.get(username_index).ok_or(ImportError::Malformed)?;
        let password = row.get(password_index).ok_or(ImportError::Malformed)?;
        let username = String::from_utf8(username.to_vec()).map_err(|_| ImportError::Malformed)?;
        let password = String::from_utf8(password.to_vec()).map_err(|_| ImportError::Malformed)?;
        result.logins.push(ImportedLogin {
            origin,
            username,
            password: Zeroizing::new(password),
        });
    }
    Ok(result)
}

fn column_indices(header: &[Zeroizing<Vec<u8>>]) -> Result<(usize, usize, usize), ImportError> {
    let mut url = None;
    let mut username = None;
    let mut password = None;
    for (index, field) in header.iter().enumerate() {
        let Ok(name) = std::str::from_utf8(field) else {
            return Err(ImportError::Malformed);
        };
        let name = name.trim_start_matches('\u{feff}').trim();
        if name.eq_ignore_ascii_case("url") {
            url = Some(index);
        }
        if name.eq_ignore_ascii_case("username") {
            username = Some(index);
        }
        if name.eq_ignore_ascii_case("password") {
            password = Some(index);
        }
    }
    match (url, username, password) {
        (Some(url), Some(username), Some(password)) => Ok((url, username, password)),
        _ => Err(ImportError::MissingColumns),
    }
}

fn origin(raw: &str) -> Option<String> {
    let page = page_url(raw.trim()).ok()?;
    let normalized = page.as_str();
    let (scheme, rest) = normalized.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host_port = authority.rsplit('@').next()?;
    if host_port.is_empty() {
        return None;
    }
    Some(format!("{scheme}://{host_port}"))
}

struct CsvScanner<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl CsvScanner<'_> {
    fn next_row(&mut self) -> Result<Option<Vec<Zeroizing<Vec<u8>>>>, ImportError> {
        if self.pos == self.bytes.len() {
            return Ok(None);
        }
        let mut fields = Vec::new();
        let mut field = Zeroizing::new(Vec::new());
        let mut quoted = false;
        let mut after_quote = false;
        let mut at_start = true;
        loop {
            let Some(&byte) = self.bytes.get(self.pos) else {
                if quoted {
                    return Err(ImportError::Malformed);
                }
                fields.push(field);
                return Ok(Some(fields));
            };
            self.pos += 1;
            if quoted {
                if byte == b'"' {
                    if self.bytes.get(self.pos) == Some(&b'"') {
                        field.push(b'"');
                        self.pos += 1;
                    } else {
                        quoted = false;
                        after_quote = true;
                    }
                } else {
                    field.push(byte);
                }
                continue;
            }
            if after_quote && !matches!(byte, b',' | b'\r' | b'\n') {
                return Err(ImportError::Malformed);
            }
            match byte {
                b'"' if at_start => {
                    quoted = true;
                    at_start = false;
                }
                b',' => {
                    fields.push(field);
                    field = Zeroizing::new(Vec::new());
                    at_start = true;
                    after_quote = false;
                }
                b'\r' | b'\n' => {
                    if byte == b'\r' && self.bytes.get(self.pos) == Some(&b'\n') {
                        self.pos += 1;
                    }
                    fields.push(field);
                    return Ok(Some(fields));
                }
                _ => {
                    field.push(byte);
                    at_start = false;
                }
            }
        }
    }
}
