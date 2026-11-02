use aegis_ipc::page_url;
use zeroize::Zeroizing;

pub fn https_origin(url: &str) -> Option<String> {
    let page = page_url(url).ok()?;
    let normalized = page.as_str();
    let rest = normalized.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    Some(format!("https://{authority}"))
}

pub fn same_origin(current_url: &str, saved_origin: &str) -> bool {
    https_origin(current_url).as_deref() == Some(saved_origin)
}

pub fn fill_script(
    origin: &str,
    username: &str,
    password: &str,
) -> Result<Zeroizing<String>, serde_json::Error> {
    let origin = serde_json::to_string(origin)?;
    let username = serde_json::to_string(username)?;
    let password = Zeroizing::new(serde_json::to_string(password)?);
    Ok(Zeroizing::new(format!(
        r#"(() => {{
      if (window !== window.top || location.origin !== {origin}) return false;
      const visible = e => e && e.getClientRects().length && getComputedStyle(e).visibility !== 'hidden' && getComputedStyle(e).display !== 'none' && !e.disabled;
      const fields = [...document.querySelectorAll('input')].filter(visible);
      const pw = fields.find(e => e.type === 'password');
      if (!pw) return false;
      const before = fields.slice(0, fields.indexOf(pw)).reverse();
      const user = before.find(e => e.type === 'text' || e.type === 'email');
      const set = (e, value) => {{
        const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
        setter.call(e, value);
        e.dispatchEvent(new Event('input', {{bubbles:true}}));
        e.dispatchEvent(new Event('change', {{bubbles:true}}));
      }};
      if (user) set(user, {username});
      set(pw, {password});
      return true;
    }})()"#,
        password = password.as_str()
    )))
}

pub const READ_FORM_SCRIPT: &str = r#"(() => {
  if (window !== window.top || location.protocol !== 'https:') return null;
  const visible = e => e && e.getClientRects().length && getComputedStyle(e).visibility !== 'hidden' && getComputedStyle(e).display !== 'none' && !e.disabled;
  const fields = [...document.querySelectorAll('input')].filter(visible);
  const pw = fields.find(e => e.type === 'password');
  if (!pw) return null;
  const before = fields.slice(0, fields.indexOf(pw)).reverse();
  const user = before.find(e => e.type === 'text' || e.type === 'email');
  return {username: user ? user.value : '', password: pw.value};
})()"#;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn origin_recheck_is_exact_and_https_only() {
        assert!(same_origin(
            "https://example.com/login",
            "https://example.com"
        ));
        assert!(!same_origin(
            "https://evil.example.com",
            "https://example.com"
        ));
        assert!(!same_origin("http://example.com", "https://example.com"));
        assert!(!same_origin(
            "https://example.com:444",
            "https://example.com"
        ));
    }

    #[test]
    fn fill_script_escapes_values() {
        let script = fill_script(
            "https://example.com",
            "a\"b\\c</script>☃",
            "q\"\\</script>雪",
        )
        .unwrap();
        assert!(script.contains("a\\\"b\\\\c</script>☃"));
        assert!(script.contains("q\\\"\\\\</script>雪"));
    }
}
