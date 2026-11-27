//! Multi-level undo and redo for the editor, without windows.
//!
//! The Edit control's own undo is one level deep, and with word wrap on it wraps
//! the whole text again: 235 ms on a 500 KB line, against 6 ms for a typed
//! character. app.rs gives each change to `History` as a diff of the text before
//! and after it, and plays a step back with EM_REPLACESEL, which costs the same
//! as typing.

/// At `pos`, the text `removed` became `inserted`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub pos: usize,
    pub removed: Vec<u16>,
    pub inserted: Vec<u16>,
}

/// The one changed range between two texts, or None when they are equal.
/// The common start and end are compared in blocks, so a 5 MB text costs
/// about a memcmp.
pub fn diff(old: &[u16], new: &[u16]) -> Option<Change> {
    let min = old.len().min(new.len());
    let mut pre = 0;
    const BLOCK: usize = 512;
    while pre + BLOCK <= min && old[pre..pre + BLOCK] == new[pre..pre + BLOCK] {
        pre += BLOCK;
    }
    while pre < min && old[pre] == new[pre] {
        pre += 1;
    }
    if pre == old.len() && pre == new.len() {
        return None;
    }
    // The common end may not overlap the common start.
    let max_suf = min - pre;
    let mut suf = 0;
    while suf + BLOCK <= max_suf && old[old.len() - suf - BLOCK..old.len() - suf] == new[new.len() - suf - BLOCK..new.len() - suf] {
        suf += BLOCK;
    }
    while suf < max_suf && old[old.len() - suf - 1] == new[new.len() - suf - 1] {
        suf += 1;
    }
    Some(Change { pos: pre, removed: old[pre..old.len() - suf].to_vec(), inserted: new[pre..new.len() - suf].to_vec() })
}

/// Apply `c` to `text`, which must be the text that `c` started from.
pub fn apply(text: &mut Vec<u16>, c: &Change) {
    let end = (c.pos + c.removed.len()).min(text.len());
    text.splice(c.pos.min(end)..end, c.inserted.iter().copied());
}

/// Ceilings: the oldest steps go first when the history holds more than
/// `MAX_STEPS` steps or more than `MAX_UNITS` UTF-16 units (128 MB).
const MAX_STEPS: usize = 10_000;
const MAX_UNITS: usize = 64 << 20;

#[derive(Default)]
pub struct History {
    undo: Vec<Change>,
    redo: Vec<Change>,
    /// `undo.len()` at the last load or save. None: no undo or redo reaches it.
    saved: Option<usize>,
    /// The top step still takes typed characters.
    open: bool,
    units: usize,
}

fn is_break(c: u16) -> bool {
    c == 10 || c == 13
}

fn is_space(c: u16) -> bool {
    c == 32 || c == 9
}

fn size(c: &Change) -> usize {
    c.removed.len() + c.inserted.len()
}

impl History {
    /// A new text (load, new tab): no steps, and the text counts as saved.
    pub fn reset(&mut self) {
        *self = History { saved: Some(0), ..History::default() };
    }

    pub fn mark_saved(&mut self) {
        self.saved = Some(self.undo.len());
        self.open = false;
    }

    /// The file on disk is gone: no step brings back the saved text.
    pub fn forget_saved(&mut self) {
        self.saved = None;
    }

    /// The text differs from the last load or save.
    pub fn modified(&self) -> bool {
        self.saved != Some(self.undo.len())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// The next change starts a new step.
    pub fn seal(&mut self) {
        self.open = false;
    }

    /// The caret moved. Away from the end of the open step, typing starts a new step.
    pub fn caret_moved(&mut self, caret: usize) {
        if self.open && self.undo.last().is_some_and(|t| t.pos + t.inserted.len() != caret) {
            self.open = false;
        }
    }

    /// A change that the user made. It clears the redo steps. Typed characters
    /// join the open step until a new word, a line break or a caret move.
    pub fn record(&mut self, c: Change) {
        self.redo.clear();
        if self.saved.is_some_and(|s| s > self.undo.len()) {
            self.saved = None; // the save point was in the redo steps
        }
        let single = size(&c) == 1 && !is_break(c.removed.first().or(c.inserted.first()).copied().unwrap_or(0));
        if single && self.open && self.saved != Some(self.undo.len()) {
            if let Some(t) = self.undo.last_mut() {
                if let [ch] = c.inserted[..] {
                    let after_space = t.inserted.last().is_some_and(|&l| is_space(l)) && !is_space(ch);
                    if !t.inserted.is_empty() && c.pos == t.pos + t.inserted.len() && !after_space {
                        t.inserted.push(ch);
                        self.units += 1;
                        return;
                    }
                } else if let [ch] = c.removed[..] {
                    if t.inserted.is_empty() && c.pos + 1 == t.pos {
                        t.removed.insert(0, ch); // Backspace
                        t.pos = c.pos;
                        self.units += 1;
                        return;
                    }
                    if t.inserted.is_empty() && c.pos == t.pos {
                        t.removed.push(ch); // Delete
                        self.units += 1;
                        return;
                    }
                }
            }
        }
        self.units += size(&c);
        self.undo.push(c);
        self.open = single;
        self.trim();
    }

    fn trim(&mut self) {
        let mut drop = 0;
        while self.undo.len() - drop > 1 && (self.undo.len() - drop > MAX_STEPS || self.units > MAX_UNITS) {
            self.units -= size(&self.undo[drop]);
            drop += 1;
        }
        if drop > 0 {
            self.undo.drain(..drop);
            self.saved = self.saved.and_then(|s| s.checked_sub(drop));
        }
    }

    /// The step that Undo takes back. Give the change that taking it back made to `undone`.
    pub fn pop_undo(&mut self) -> Option<Change> {
        self.open = false;
        let c = self.undo.pop()?;
        self.units -= size(&c);
        Some(c)
    }

    /// `c` took back an undo step: Redo takes `c` back.
    pub fn undone(&mut self, c: Change) {
        self.redo.push(c);
    }

    pub fn pop_redo(&mut self) -> Option<Change> {
        self.open = false;
        self.redo.pop()
    }

    /// `c` took back a redo step: Undo takes `c` back. The other redo steps stay.
    pub fn redone(&mut self, c: Change) {
        self.units += size(&c);
        self.undo.push(c);
        self.trim();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn ch(pos: usize, removed: &str, inserted: &str) -> Change {
        Change { pos, removed: w(removed), inserted: w(inserted) }
    }

    /// Take back `c` in `text`, like app.rs does, and return the change that it made.
    fn revert(text: &mut Vec<u16>, c: &Change) -> Change {
        let before = text.clone();
        text.splice(c.pos..c.pos + c.inserted.len(), c.removed.iter().copied());
        diff(&before, text).unwrap()
    }

    #[test]
    fn diff_finds_the_one_changed_range() {
        assert_eq!(diff(&w("hello"), &w("hello")), None);
        assert_eq!(diff(&w("helo"), &w("hello")), Some(ch(3, "", "l")));
        assert_eq!(diff(&w("hello"), &w("hxo")), Some(ch(1, "ell", "x")));
        assert_eq!(diff(&w(""), &w("ab")), Some(ch(0, "", "ab")));
        assert_eq!(diff(&w("ab"), &w("")), Some(ch(0, "ab", "")));
        // "aaa" -> "aa": the common start and end may not overlap.
        assert_eq!(diff(&w("aaa"), &w("aa")), Some(ch(2, "a", "")));
    }

    #[test]
    fn diff_over_block_edges() {
        let old: Vec<u16> = (0..5000u32).map(|i| (i % 97) as u16).collect();
        for pos in [0, 511, 512, 513, 1024, 2600, 4999] {
            let mut new = old.clone();
            new.insert(pos, 60000);
            let c = diff(&old, &new).unwrap();
            let mut t = old.clone();
            apply(&mut t, &c);
            assert_eq!(t, new, "pos {pos}");
            assert_eq!(size(&c), 1);
        }
    }

    #[test]
    fn typing_joins_one_step_per_word() {
        let mut h = History::default();
        h.reset();
        for (i, c) in "ab cd".chars().enumerate() {
            h.record(ch(i, "", &c.to_string()));
        }
        assert_eq!(h.undo.len(), 2);
        assert_eq!(h.undo[0].inserted, w("ab "));
        assert_eq!(h.undo[1].inserted, w("cd"));
    }

    #[test]
    fn a_caret_move_or_a_line_break_starts_a_new_step() {
        let mut h = History::default();
        h.reset();
        h.record(ch(0, "", "a"));
        h.caret_moved(1); // the caret after the typed character: no new step
        h.record(ch(1, "", "b"));
        h.caret_moved(0);
        h.record(ch(0, "", "c"));
        assert_eq!(h.undo.len(), 2);
        h.record(ch(1, "", "\r\n"));
        h.record(ch(3, "", "d"));
        assert_eq!(h.undo.len(), 4);
    }

    #[test]
    fn backspace_and_delete_join() {
        let mut h = History::default();
        h.reset();
        // "abcd", caret at 3: Backspace twice, then Delete at 1.
        h.record(ch(2, "c", ""));
        h.record(ch(1, "b", ""));
        assert_eq!(h.undo.len(), 1);
        assert_eq!(h.undo[0], ch(1, "bc", ""));
        h.record(ch(1, "d", ""));
        assert_eq!(h.undo[0], ch(1, "bcd", ""));
    }

    #[test]
    fn undo_and_redo_play_back_the_text() {
        let mut text = w("one two");
        let mut h = History::default();
        h.reset();
        let steps = [ch(3, " ", "_"), ch(0, "", "X"), ch(1, "one_two", "")];
        let mut states = vec![text.clone()];
        for c in steps {
            apply(&mut text, &c);
            h.record(c);
            h.seal();
            states.push(text.clone());
        }
        assert!(h.modified());
        for k in (0..3).rev() {
            let c = h.pop_undo().unwrap();
            let d = revert(&mut text, &c);
            h.undone(d);
            assert_eq!(text, states[k]);
        }
        assert!(!h.can_undo());
        assert!(!h.modified(), "back at the loaded text");
        for state in &states[1..] {
            let c = h.pop_redo().unwrap();
            let d = revert(&mut text, &c);
            h.redone(d);
            assert_eq!(&text, state);
        }
        assert!(!h.can_redo());
    }

    #[test]
    fn the_save_point_follows_undo_and_redo() {
        let mut h = History::default();
        h.reset();
        h.record(ch(0, "", "a"));
        h.mark_saved();
        assert!(!h.modified());
        // A typed character after the save is a new step, not part of the saved one.
        h.record(ch(1, "", "b"));
        assert_eq!(h.undo.len(), 2);
        assert!(h.modified());
        h.pop_undo();
        h.undone(ch(1, "b", ""));
        assert!(!h.modified());
        // Undo past the save point, then a new change: the save point is gone.
        h.pop_undo();
        h.undone(ch(0, "a", ""));
        h.record(ch(0, "", "z"));
        assert!(h.modified());
        assert!(!h.can_redo());
        h.pop_undo();
        assert!(h.modified());
    }

    #[test]
    fn the_oldest_steps_go_at_the_ceiling() {
        let mut h = History::default();
        h.reset();
        for i in 0..MAX_STEPS + 5 {
            h.record(ch(i, "", "\n"));
        }
        assert_eq!(h.undo.len(), MAX_STEPS);
        assert_eq!(h.saved, None);
        assert_eq!(h.units, MAX_STEPS);
    }
}
