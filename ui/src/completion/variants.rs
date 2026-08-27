#![cfg_attr(not(test), allow(dead_code))]

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GhostVariants {
    snapshot: usize,
    items: Vec<String>,
    index: usize,
}

impl GhostVariants {
    pub fn set_first(&mut self, snapshot: usize, item: String) {
        self.snapshot = snapshot;
        self.items = vec![item];
        self.index = 0;
    }

    pub fn current(&self) -> Option<&str> {
        self.items.get(self.index).map(String::as_str)
    }

    pub fn items(&self) -> &[String] {
        &self.items
    }

    pub fn prev(&mut self) {
        if self.index > 0 {
            self.index -= 1;
        }
    }

    pub fn show_next_existing(&mut self) -> bool {
        if self.index + 1 < self.items.len() {
            self.index += 1;
            true
        } else {
            false
        }
    }

    pub fn needs_fetch(&self) -> bool {
        !self.items.is_empty() && self.index + 1 >= self.items.len()
    }

    pub fn push(&mut self, item: String) {
        self.items.push(item);
        self.index = self.items.len() - 1;
    }

    pub fn clear_if_changed(&mut self, snapshot: usize) {
        if snapshot != self.snapshot {
            self.snapshot = 0;
            self.items.clear();
            self.index = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GhostVariants;

    #[test]
    fn variant_ring_next_fetch_prev_and_snapshot_clear() {
        let mut ring = GhostVariants::default();
        ring.set_first(1, "FROM users".into());
        assert_eq!(ring.current(), Some("FROM users"));
        assert!(ring.needs_fetch());
        assert!(!ring.show_next_existing());
        ring.push("FROM orders".into());
        assert_eq!(ring.current(), Some("FROM orders"));
        ring.prev();
        assert_eq!(ring.current(), Some("FROM users"));
        ring.prev();
        assert_eq!(ring.current(), Some("FROM users"));
        ring.clear_if_changed(2);
        assert!(ring.current().is_none());
    }
}
