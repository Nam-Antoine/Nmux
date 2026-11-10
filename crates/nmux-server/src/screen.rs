use nmux_core::detect::TitleWatcher;

pub const DEFAULT_COLS: u16 = 120;
pub const DEFAULT_ROWS: u16 = 32;
pub const SCROLLBACK_LINES: usize = 5000;

pub struct Screen {
    parser: vt100::Parser,
    carry: Vec<u8>,
    titles: TitleWatcher,
    pub title: Option<String>,
    pub seq: u64,
}

impl Default for Screen {
    fn default() -> Self {
        Screen::new(DEFAULT_COLS, DEFAULT_ROWS)
    }
}

impl Screen {
    pub fn new(cols: u16, rows: u16) -> Screen {
        Screen {
            parser: vt100::Parser::new(rows, cols, SCROLLBACK_LINES),
            carry: Vec::new(),
            titles: TitleWatcher::default(),
            title: None,
            seq: 0,
        }
    }

    pub fn size(&self) -> (u16, u16) {
        let (rows, cols) = self.parser.screen().size();
        (cols, rows)
    }

    pub fn resize(&mut self, cols: u16, rows: u16) -> bool {
        if cols < 2 || rows < 1 || self.size() == (cols, rows) {
            return false;
        }
        self.parser.screen_mut().set_size(rows, cols);
        true
    }

    pub fn feed(&mut self, bytes: &[u8]) -> String {
        self.parser.process(bytes);
        self.carry.extend_from_slice(bytes);
        let text = take_utf8(&mut self.carry);
        if let Some(title) = self.titles.push(&text) {
            self.title = Some(title);
        }
        self.seq += 1;
        text
    }

    pub fn cursor(&self) -> (u16, u16) {
        let (row, col) = self.parser.screen().cursor_position();
        (row + 1, col + 1)
    }

    pub fn bracketed_paste(&self) -> bool {
        self.parser.screen().bracketed_paste()
    }

    pub fn text(&self) -> String {
        self.parser.screen().contents()
    }

    pub fn snapshot(&mut self) -> String {
        let (cols, rows) = self.size();
        let mut out: Vec<u8> = Vec::new();

        if !self.parser.screen().alternate_screen() {
            self.parser.screen_mut().set_scrollback(usize::MAX);
            let history = self.parser.screen().scrollback();
            for back in (1..=history).rev() {
                self.parser.screen_mut().set_scrollback(back);
                if let Some(row) = self.parser.screen().rows_formatted(0, cols).next() {
                    out.extend_from_slice(&row);
                }
                out.extend_from_slice(b"\x1b[m\r\n");
            }
            self.parser.screen_mut().set_scrollback(0);
            let in_view = history.min(rows as usize - 1);
            if in_view > 0 {
                out.extend_from_slice(format!("\x1b[{rows};1H").as_bytes());
                out.extend(std::iter::repeat_n(b'\n', in_view));
            }
        }
        out.extend_from_slice(&self.parser.screen().state_formatted());
        String::from_utf8_lossy(&out).into_owned()
    }
}

fn take_utf8(buf: &mut Vec<u8>) -> String {
    let mut out = String::new();
    let mut rest: &[u8] = buf;
    loop {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                out.push_str(s);
                rest = &[];
                break;
            }
            Err(e) => {
                let (good, bad) = rest.split_at(e.valid_up_to());
                out.push_str(std::str::from_utf8(good).unwrap_or_default());
                match e.error_len() {
                    Some(n) => {
                        out.push('\u{FFFD}');
                        rest = &bad[n..];
                    }
                    None => {
                        rest = bad;
                        break;
                    }
                }
            }
        }
    }
    let keep = rest.to_vec();
    *buf = keep;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_character_split_across_chunks_is_not_mangled() {
        let mut s = Screen::new(20, 5);
        let bytes = "é✳".as_bytes();
        assert_eq!(s.feed(&bytes[..1]), "");
        assert_eq!(s.feed(&bytes[1..3]), "é");
        assert_eq!(s.feed(&bytes[3..]), "✳");
        assert_eq!(s.feed(b"a\xffb"), "a\u{FFFD}b");
        assert_eq!(s.seq, 4);
    }

    #[test]
    fn titles_are_tracked() {
        let mut s = Screen::new(20, 5);
        s.feed("\x1b]0;◐ Working\x07hello".as_bytes());
        assert_eq!(s.title.as_deref(), Some("◐ Working"));
        assert!(s.text().starts_with("hello"));
    }

    #[test]
    fn a_snapshot_reproduces_screen_and_scrollback() {
        for lines in [3usize, 4, 5, 12, 40] {
            let mut s = Screen::new(20, 5);
            for i in 1..=lines {
                s.feed(format!("\x1b[3{}mline {i}\x1b[m\r\n", i % 7).as_bytes());
            }
            s.feed(b"prompt> ");
            let snap = s.snapshot();

            let mut copy = Screen::new(20, 5);
            copy.feed(snap.as_bytes());
            assert_eq!(copy.text(), s.text(), "{lines} lines: screen");
            assert_eq!(copy.parser.screen().cursor_position(), s.parser.screen().cursor_position());

            let all = |sc: &mut Screen| {
                sc.parser.screen_mut().set_scrollback(usize::MAX);
                let n = sc.parser.screen().scrollback();
                let mut rows = Vec::new();
                for back in (1..=n).rev() {
                    sc.parser.screen_mut().set_scrollback(back);
                    rows.push(sc.parser.screen().rows(0, 20).next().unwrap());
                }
                sc.parser.screen_mut().set_scrollback(0);
                rows
            };
            assert_eq!(all(&mut copy), all(&mut s), "{lines} lines: history");
        }
    }

    #[test]
    fn resize_ignores_nonsense() {
        let mut s = Screen::new(20, 5);
        assert!(!s.resize(1, 5));
        assert!(!s.resize(20, 5));
        assert!(s.resize(40, 10));
        assert_eq!(s.size(), (40, 10));
    }
}
