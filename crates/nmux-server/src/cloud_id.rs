//! Finds the cloud session id and link in `claude --cloud` output, e.g.
//!
//! ```text
//! Created cloud session: Codebase check
//! View: https://claude.ai/code/session_01PbVN3TZ73CTZQcg8YVCbii?from=cli
//! Resume with: claude --teleport session_01PbVN3TZ73CTZQcg8YVCbii
//! ```

/// Characters of recent plain text kept between chunks.
const KEEP: usize = 4096;
const URL_PREFIX: &str = "https://claude.ai/code/";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloudMatch {
    pub id: String,
    pub url: String,
}

#[derive(Default)]
pub struct CloudIdSniffer {
    text: String,
}

impl CloudIdSniffer {
    /// Feed raw PTY output. Returns the id once it has been printed in full.
    pub fn push(&mut self, data: &str) -> Option<CloudMatch> {
        self.text.push_str(&to_plain_text(data));
        if self.text.len() > KEEP {
            let mut cut = self.text.len() - KEEP;
            while !self.text.is_char_boundary(cut) {
                cut += 1;
            }
            self.text.drain(..cut);
        }
        find(&self.text)
    }
}

fn is_id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

fn find(text: &str) -> Option<CloudMatch> {
    let mut from = 0;
    while let Some(at) = text[from..].find(URL_PREFIX) {
        let start = from + at + URL_PREFIX.len();
        let rest = &text[start..];
        let len = rest.find(|c: char| !is_id_char(c));
        from = start;
        // The id must be followed by something that is not part of an id, so
        // a chunk that ends mid-id cannot hand back a truncated one.
        let Some(len) = len else { return None };
        let id = &rest[..len];
        let body = id.strip_prefix("session_").or_else(|| id.strip_prefix("cse_"));
        if body.is_some_and(|b| (8..=120).contains(&b.len())) {
            return Some(CloudMatch { id: id.to_string(), url: format!("{URL_PREFIX}{id}") });
        }
    }
    None
}

/// Drop terminal control sequences. Cursor-forward (`CSI n C`) becomes
/// spaces: Claude's renderer uses it instead of writing blanks between words.
pub fn to_plain_text(data: &str) -> String {
    let mut out = String::with_capacity(data.len());
    let mut chars = data.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            if !c.is_control() || c == '\n' || c == '\t' {
                out.push(c);
            }
            continue;
        }
        match chars.next() {
            // OSC (titles, hyperlinks): up to BEL or ESC \.
            Some(']') => {
                while let Some(n) = chars.next() {
                    if n == '\x07' {
                        break;
                    }
                    if n == '\x1b' {
                        chars.next_if_eq(&'\\');
                        break;
                    }
                }
            }
            Some('[') => {
                let mut params = String::new();
                let mut last = None;
                for n in chars.by_ref() {
                    if ('\x40'..='\x7e').contains(&n) {
                        last = Some(n);
                        break;
                    }
                    params.push(n);
                }
                if last == Some('C') {
                    let n = params.parse::<usize>().unwrap_or(1).clamp(1, 200);
                    out.extend(std::iter::repeat_n(' ', n));
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_id_in_real_output() {
        let mut s = CloudIdSniffer::default();
        let out = "Created cloud session: Codebase check\r\nView: https://claude.ai/code/session_01PbVN3TZ73CTZQcg8YVCbii?from=cli&m=0\r\n";
        let m = s.push(out).unwrap();
        assert_eq!(m.id, "session_01PbVN3TZ73CTZQcg8YVCbii");
        assert_eq!(m.url, "https://claude.ai/code/session_01PbVN3TZ73CTZQcg8YVCbii");
    }

    #[test]
    fn an_id_split_across_chunks_is_not_truncated() {
        let mut s = CloudIdSniffer::default();
        assert_eq!(s.push("View: https://claude.ai/code/session_01PbVN3TZ"), None);
        assert_eq!(s.push("73CTZQcg8YVCbii\r\n").unwrap().id, "session_01PbVN3TZ73CTZQcg8YVCbii");
    }

    #[test]
    fn other_hosts_and_short_ids_are_ignored() {
        let mut s = CloudIdSniffer::default();
        assert_eq!(s.push("https://evil.example/code/session_01PbVN3TZ73CTZQcg8YVCbii \n"), None);
        assert_eq!(s.push("https://claude.ai/code/session_ab \n"), None);
        assert_eq!(s.push("https://claude.ai/code/other_0123456789 \n"), None);
    }

    #[test]
    fn control_sequences_are_removed() {
        assert_eq!(to_plain_text("\x1b[1mView:\x1b[0m\x1b[2Chttps\x1b]0;t\x07://x"), "View:  https://x");
        assert_eq!(to_plain_text("a\x1b]8;;http://l\x1b\\b\rc"), "abc");
    }
}
