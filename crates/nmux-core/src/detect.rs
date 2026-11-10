use crate::model::Detected;

const ESC: char = '\x1b';
const BEL: char = '\x07';
const MAX_PENDING: usize = 1024;

#[derive(Default)]
pub struct TitleWatcher {
    pending: String,
}

impl TitleWatcher {
    pub fn push(&mut self, data: &str) -> Option<String> {
        let text = std::mem::take(&mut self.pending) + data;
        let mut title = None;
        let mut rest = text.as_str();
        while let Some(start) = rest.find("\x1b]") {
            let body = &rest[start + 2..];
            let bel = body.find(BEL);
            let st = body.find("\x1b\\");
            let (end, skip) = match (bel, st) {
                (Some(b), Some(s)) if s < b => (s, 2),
                (Some(b), _) => (b, 1),
                (None, Some(s)) => (s, 2),
                (None, None) => {
                    let tail = &rest[start..];
                    if tail.len() <= MAX_PENDING {
                        self.pending = tail.to_string();
                    }
                    return title;
                }
            };
            let osc = &body[..end];
            if let Some(t) = osc.strip_prefix("0;").or_else(|| osc.strip_prefix("2;")) {
                title = Some(t.to_string());
            }
            rest = &body[end + skip..];
        }
        if rest.ends_with(ESC) {
            self.pending = ESC.to_string();
        }
        title
    }
}

pub fn title_is_working(title: &str) -> bool {
    title
        .trim_start()
        .chars()
        .next()
        .is_some_and(|c| matches!(c, '\u{25D0}'..='\u{25D3}' | '\u{2800}'..='\u{28FF}'))
}

const RECENT_LINES: usize = 40;

pub fn screen_is_blocked(screen: &str) -> bool {
    let lines: Vec<&str> = screen.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).collect();
    let recent = lines[lines.len().saturating_sub(RECENT_LINES)..].join("\n").to_lowercase();

    let form = recent.contains("esc to cancel")
        && (recent.contains("enter to confirm")
            || (recent.contains("enter to select")
                && ["keys to navigate", "arrows to navigate", "↑/↓ to navigate", "↑↓ to navigate"]
                    .iter()
                    .any(|k| recent.contains(k))));
    let picker = recent.contains("select model") && recent.contains("enter to set as default");

    let permission = recent.contains("do you want to proceed?")
        || recent.contains("do you want to allow this connection?")
        || recent.contains("waiting for permission")
        || (recent.contains("do you want to") && recent.contains("❯ 1. yes"));

    (form && !picker) || permission
}

pub fn classify(title: Option<&str>, screen: &str) -> Option<Detected> {
    if screen_is_blocked(screen) {
        return Some(Detected::Blocked);
    }
    let title = title?;
    Some(if title_is_working(title) { Detected::Working } else { Detected::Idle })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_read_with_either_terminator() {
        let mut w = TitleWatcher::default();
        assert_eq!(w.push("a\x1b]0;✳ Fix bug\x07b"), Some("✳ Fix bug".to_string()));
        assert_eq!(w.push("\x1b]2;◐ Fix bug\x1b\\"), Some("◐ Fix bug".to_string()));
        assert_eq!(w.push("plain output"), None);
        assert_eq!(w.push("\x1b]8;;http://x\x07link\x1b]8;;\x07"), None);
    }

    #[test]
    fn a_title_split_across_chunks_is_reassembled() {
        let mut w = TitleWatcher::default();
        assert_eq!(w.push("out\x1b]0;◓ Wor"), None);
        assert_eq!(w.push("king\x07"), Some("◓ Working".to_string()));
        assert_eq!(w.push("x\x1b"), None);
        assert_eq!(w.push("]0;✳ Done\x07"), Some("✳ Done".to_string()));
    }

    #[test]
    fn the_last_title_in_a_chunk_wins() {
        let mut w = TitleWatcher::default();
        assert_eq!(w.push("\x1b]0;◐ a\x07\x1b]0;✳ a\x07"), Some("✳ a".to_string()));
    }

    #[test]
    fn a_runaway_osc_is_dropped() {
        let mut w = TitleWatcher::default();
        assert_eq!(w.push(&format!("\x1b]0;{}", "x".repeat(5000))), None);
        assert_eq!(w.push("\x07\x1b]0;✳ ok\x07"), Some("✳ ok".to_string()));
    }

    #[test]
    fn spinner_glyphs_mean_working() {
        for t in ["◐ t", "◓ t", "◑ t", "◒ t", "⠋ t", "  ◐ t"] {
            assert!(title_is_working(t), "{t}");
        }
        for t in ["✳ t", "claude", "", "* t"] {
            assert!(!title_is_working(t), "{t}");
        }
    }

    #[test]
    fn permission_prompts_block() {
        let screen = "● Bash(rm -rf build)\n\n Do you want to proceed?\n ❯ 1. Yes\n   2. No\n\n Esc to cancel · Tab to amend";
        assert!(screen_is_blocked(screen));
        assert_eq!(classify(Some("◐ t"), screen), Some(Detected::Blocked));
    }

    #[test]
    fn question_forms_block_but_the_model_picker_does_not() {
        let form = "Which approach?\n ❯ 1. A\n   2. B\n\nEnter to select · ↑/↓ to navigate · Esc to cancel";
        assert!(screen_is_blocked(form));
        let picker = "Select model\n ❯ 1. Default\n\nEnter to set as default · Esc to cancel · Enter to confirm";
        assert!(!screen_is_blocked(picker));
    }

    #[test]
    fn an_ordinary_screen_follows_the_title() {
        let screen = "● Done.\n\n❯ \n  ? for shortcuts";
        assert_eq!(classify(Some("✳ t"), screen), Some(Detected::Idle));
        assert_eq!(classify(Some("◒ t"), screen), Some(Detected::Working));
        assert_eq!(classify(None, screen), None);
    }

    #[test]
    fn an_old_prompt_scrolled_far_up_no_longer_blocks() {
        let mut screen = String::from("Do you want to proceed?\n");
        for i in 0..60 {
            screen.push_str(&format!("line {i}\n"));
        }
        assert!(!screen_is_blocked(&screen));
    }
}
