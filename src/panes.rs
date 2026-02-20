#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Split {
    Vertical,
    Horizontal,
}

#[derive(Clone, Debug)]
pub struct PaneLayout {
    pub pane_id: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Describes a draggable divider between two pane regions.
pub struct DividerInfo {
    pub split: Split,
    /// Pixel position of the divider line (x for vertical, y for horizontal).
    pub position: f32,
    /// Start of the split dimension (x for vertical, y for horizontal).
    pub origin: f32,
    /// Total span of the split dimension (width for vertical, height for horizontal).
    pub span: f32,
    /// Perpendicular bounds for hit-testing.
    pub perp_start: f32,
    pub perp_end: f32,
    /// Path from root to this split node (false=left, true=right at each ancestor).
    pub path: Vec<bool>,
}

enum Node {
    Leaf { pane_id: usize },
    Split {
        split: Split,
        ratio: f32,
        left: Box<Node>,
        right: Box<Node>,
    },
}

impl Node {
    fn collect_pane_ids(&self, ids: &mut Vec<usize>) {
        match self {
            Node::Leaf { pane_id } => ids.push(*pane_id),
            Node::Split { left, right, .. } => {
                left.collect_pane_ids(ids);
                right.collect_pane_ids(ids);
            }
        }
    }

    fn pane_count(&self) -> usize {
        match self {
            Node::Leaf { .. } => 1,
            Node::Split { left, right, .. } => left.pane_count() + right.pane_count(),
        }
    }

    fn collect_dividers(
        &self, x: f32, y: f32, w: f32, h: f32,
        path: &mut Vec<bool>, dividers: &mut Vec<DividerInfo>,
    ) {
        if let Node::Split { split, ratio, left, right } = self {
            match split {
                Split::Vertical => {
                    let left_w = (w * ratio).floor();
                    dividers.push(DividerInfo {
                        split: Split::Vertical,
                        position: x + left_w,
                        origin: x,
                        span: w,
                        perp_start: y,
                        perp_end: y + h,
                        path: path.clone(),
                    });
                    path.push(false);
                    left.collect_dividers(x, y, left_w, h, path, dividers);
                    path.pop();
                    path.push(true);
                    right.collect_dividers(x + left_w, y, w - left_w, h, path, dividers);
                    path.pop();
                }
                Split::Horizontal => {
                    let top_h = (h * ratio).floor();
                    dividers.push(DividerInfo {
                        split: Split::Horizontal,
                        position: y + top_h,
                        origin: y,
                        span: h,
                        perp_start: x,
                        perp_end: x + w,
                        path: path.clone(),
                    });
                    path.push(false);
                    left.collect_dividers(x, y, w, top_h, path, dividers);
                    path.pop();
                    path.push(true);
                    right.collect_dividers(x, y + top_h, w, h - top_h, path, dividers);
                    path.pop();
                }
            }
        }
    }

    fn set_ratio_at(&mut self, path: &[bool], ratio: f32) {
        match self {
            Node::Split { ratio: r, left, right, .. } => {
                if path.is_empty() {
                    *r = ratio;
                } else if path[0] {
                    right.set_ratio_at(&path[1..], ratio);
                } else {
                    left.set_ratio_at(&path[1..], ratio);
                }
            }
            Node::Leaf { .. } => {}
        }
    }

    fn calculate_layouts(&self, x: f32, y: f32, w: f32, h: f32, layouts: &mut Vec<PaneLayout>) {
        match self {
            Node::Leaf { pane_id } => {
                layouts.push(PaneLayout {
                    pane_id: *pane_id,
                    x,
                    y,
                    width: w,
                    height: h,
                });
            }
            Node::Split {
                split,
                ratio,
                left,
                right,
            } => match split {
                Split::Vertical => {
                    let left_w = (w * ratio).floor();
                    let right_w = w - left_w;
                    left.calculate_layouts(x, y, left_w, h, layouts);
                    right.calculate_layouts(x + left_w, y, right_w, h, layouts);
                }
                Split::Horizontal => {
                    let top_h = (h * ratio).floor();
                    let bottom_h = h - top_h;
                    left.calculate_layouts(x, y, w, top_h, layouts);
                    right.calculate_layouts(x, y + top_h, w, bottom_h, layouts);
                }
            },
        }
    }

    /// Split the leaf with the given pane_id, replacing it with a split node.
    /// Returns true if the split was performed.
    fn split_pane(&mut self, target_id: usize, split: Split, new_id: usize) -> bool {
        match self {
            Node::Leaf { pane_id } if *pane_id == target_id => {
                let old = Node::Leaf { pane_id: *pane_id };
                let new = Node::Leaf { pane_id: new_id };
                *self = Node::Split {
                    split,
                    ratio: 0.5,
                    left: Box::new(old),
                    right: Box::new(new),
                };
                true
            }
            Node::Split { left, right, .. } => {
                left.split_pane(target_id, split, new_id)
                    || right.split_pane(target_id, split, new_id)
            }
            _ => false,
        }
    }

    /// Remove a pane by ID. Returns Some(remaining_node) if found and removed,
    /// None if not found, or the pane_id if this is the last leaf.
    fn remove_pane(self, target_id: usize) -> RemoveResult {
        match self {
            Node::Leaf { pane_id } if pane_id == target_id => RemoveResult::Removed,
            Node::Leaf { .. } => RemoveResult::NotFound(self),
            Node::Split {
                left,
                right,
                split,
                ratio,
            } => {
                // Try removing from left
                match left.remove_pane(target_id) {
                    RemoveResult::Removed => {
                        // Left was removed, promote right
                        RemoveResult::Replaced(*right)
                    }
                    RemoveResult::Replaced(new_left) => RemoveResult::Replaced(Node::Split {
                        split,
                        ratio,
                        left: Box::new(new_left),
                        right,
                    }),
                    RemoveResult::NotFound(left) => {
                        // Try removing from right
                        match right.remove_pane(target_id) {
                            RemoveResult::Removed => {
                                // Right was removed, promote left
                                RemoveResult::Replaced(left)
                            }
                            RemoveResult::Replaced(new_right) => {
                                RemoveResult::Replaced(Node::Split {
                                    split,
                                    ratio,
                                    left: Box::new(left),
                                    right: Box::new(new_right),
                                })
                            }
                            RemoveResult::NotFound(right) => {
                                RemoveResult::NotFound(Node::Split {
                                    split,
                                    ratio,
                                    left: Box::new(left),
                                    right: Box::new(right),
                                })
                            }
                        }
                    }
                }
            }
        }
    }
}

enum RemoveResult {
    /// The target leaf was found and removed.
    Removed,
    /// The target was found; the tree was restructured. Here is the replacement node.
    Replaced(Node),
    /// The target was not found. Here is the original node back.
    NotFound(Node),
}

pub struct PaneTree {
    root: Node,
    active: usize,
    zoomed: bool,
}

impl PaneTree {
    pub fn new(pane_id: usize) -> Self {
        PaneTree {
            root: Node::Leaf { pane_id },
            active: pane_id,
            zoomed: false,
        }
    }

    pub fn pane_count(&self) -> usize {
        self.root.pane_count()
    }

    pub fn active_pane_id(&self) -> usize {
        self.active
    }

    pub fn set_active(&mut self, pane_id: usize) {
        self.active = pane_id;
    }

    pub fn toggle_zoom(&mut self) {
        self.zoomed = !self.zoomed;
    }

    /// Split the active pane. The new pane gets `new_id` and becomes active.
    pub fn split_active(&mut self, split: Split, new_id: usize) {
        self.root.split_pane(self.active, split, new_id);
        self.active = new_id;
        self.zoomed = false;
    }

    /// Close the active pane. Returns true if it was the last pane.
    pub fn close_active(&mut self) -> bool {
        if self.pane_count() <= 1 {
            return true;
        }

        let ids = self.pane_ids();
        let current_idx = ids.iter().position(|&id| id == self.active).unwrap_or(0);

        // Take ownership of root to perform removal
        let old_root = std::mem::replace(&mut self.root, Node::Leaf { pane_id: 0 });
        match old_root.remove_pane(self.active) {
            RemoveResult::Replaced(new_root) => {
                self.root = new_root;
            }
            RemoveResult::Removed => {
                // Shouldn't happen since we checked pane_count > 1
                return true;
            }
            RemoveResult::NotFound(root) => {
                self.root = root;
                return false;
            }
        }

        // Move focus to an adjacent pane
        let new_ids = self.pane_ids();
        self.active = if current_idx > 0 && current_idx <= new_ids.len() {
            new_ids[current_idx - 1]
        } else {
            new_ids[0]
        };
        self.zoomed = false;
        false
    }

    pub fn focus_next(&mut self) {
        let ids = self.pane_ids();
        if ids.len() <= 1 {
            return;
        }
        let idx = ids.iter().position(|&id| id == self.active).unwrap_or(0);
        self.active = ids[(idx + 1) % ids.len()];
        self.zoomed = false;
    }

    pub fn focus_prev(&mut self) {
        let ids = self.pane_ids();
        if ids.len() <= 1 {
            return;
        }
        let idx = ids.iter().position(|&id| id == self.active).unwrap_or(0);
        self.active = if idx == 0 { ids[ids.len() - 1] } else { ids[idx - 1] };
        self.zoomed = false;
    }

    pub fn pane_ids(&self) -> Vec<usize> {
        let mut ids = Vec::new();
        self.root.collect_pane_ids(&mut ids);
        ids
    }

    /// Collect all divider positions for hit-testing.
    pub fn collect_dividers(&self, width: f32, height: f32) -> Vec<DividerInfo> {
        let mut dividers = Vec::new();
        let mut path = Vec::new();
        self.root.collect_dividers(0.0, 0.0, width, height, &mut path, &mut dividers);
        dividers
    }

    /// Update the ratio of a split node identified by its tree path.
    pub fn set_ratio_at(&mut self, path: &[bool], ratio: f32) {
        self.root.set_ratio_at(path, ratio);
    }

    /// Calculate pixel layouts for all panes in the given viewport.
    pub fn calculate_layouts(&self, width: f32, height: f32) -> Vec<PaneLayout> {
        if self.zoomed {
            // Only show active pane, full viewport
            return vec![PaneLayout {
                pane_id: self.active,
                x: 0.0,
                y: 0.0,
                width,
                height,
            }];
        }

        let mut layouts = Vec::new();
        self.root.calculate_layouts(0.0, 0.0, width, height, &mut layouts);
        layouts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_pane_fills_viewport() {
        let tree = PaneTree::new(0);
        let layouts = tree.calculate_layouts(800.0, 600.0);
        assert_eq!(layouts.len(), 1);
        let l = &layouts[0];
        assert_eq!(l.pane_id, 0);
        assert!((l.x - 0.0).abs() < 0.01);
        assert!((l.y - 0.0).abs() < 0.01);
        assert!((l.width - 800.0).abs() < 0.01);
        assert!((l.height - 600.0).abs() < 0.01);
    }

    #[test]
    fn vertical_split_divides_width() {
        let mut tree = PaneTree::new(0);
        tree.split_active(Split::Vertical, 1);
        let layouts = tree.calculate_layouts(800.0, 600.0);
        assert_eq!(layouts.len(), 2);
        let total_width: f32 = layouts.iter().map(|l| l.width).sum();
        assert!((total_width - 800.0).abs() < 0.01);
        // Both panes should have full height
        for l in &layouts {
            assert!((l.height - 600.0).abs() < 0.01);
        }
    }

    #[test]
    fn horizontal_split_divides_height() {
        let mut tree = PaneTree::new(0);
        tree.split_active(Split::Horizontal, 1);
        let layouts = tree.calculate_layouts(800.0, 600.0);
        assert_eq!(layouts.len(), 2);
        let total_height: f32 = layouts.iter().map(|l| l.height).sum();
        assert!((total_height - 600.0).abs() < 0.01);
        // Both panes should have full width
        for l in &layouts {
            assert!((l.width - 800.0).abs() < 0.01);
        }
    }

    #[test]
    fn nested_splits_produce_correct_count() {
        let mut tree = PaneTree::new(0);
        tree.split_active(Split::Vertical, 1);
        tree.split_active(Split::Horizontal, 2);
        tree.split_active(Split::Vertical, 3);
        let layouts = tree.calculate_layouts(800.0, 600.0);
        assert_eq!(layouts.len(), 4);
    }

    #[test]
    fn close_last_pane_returns_true() {
        let mut tree = PaneTree::new(0);
        assert!(tree.close_active());
    }

    #[test]
    fn close_non_last_returns_false() {
        let mut tree = PaneTree::new(0);
        tree.split_active(Split::Vertical, 1);
        assert!(!tree.close_active());
        let layouts = tree.calculate_layouts(800.0, 600.0);
        assert_eq!(layouts.len(), 1);
    }

    #[test]
    fn focus_next_cycles() {
        let mut tree = PaneTree::new(0);
        tree.split_active(Split::Vertical, 1);
        tree.split_active(Split::Vertical, 2);
        // Active is now 2 (the last split). Pane order: [0, 1, 2]
        let ids = tree.pane_ids();
        assert_eq!(ids.len(), 3);

        // Navigate to the last pane in order
        tree.set_active(*ids.last().unwrap());
        let last_id = tree.active_pane_id();

        // focus_next from last should wrap to first
        tree.focus_next();
        assert_eq!(tree.active_pane_id(), ids[0]);

        // Verify it wrapped (new active differs from last)
        assert_ne!(tree.active_pane_id(), last_id);
    }

    #[test]
    fn focus_prev_cycles() {
        let mut tree = PaneTree::new(0);
        tree.split_active(Split::Vertical, 1);
        tree.split_active(Split::Vertical, 2);
        let ids = tree.pane_ids();
        assert_eq!(ids.len(), 3);

        // Set active to the first pane
        tree.set_active(ids[0]);
        assert_eq!(tree.active_pane_id(), ids[0]);

        // focus_prev from first should wrap to last
        tree.focus_prev();
        assert_eq!(tree.active_pane_id(), *ids.last().unwrap());
    }

    #[test]
    fn zoom_shows_only_active_pane() {
        let mut tree = PaneTree::new(0);
        tree.split_active(Split::Vertical, 1);
        tree.split_active(Split::Horizontal, 2);
        // 3 panes exist, active is 2
        assert_eq!(tree.pane_count(), 3);

        tree.toggle_zoom();
        let layouts = tree.calculate_layouts(800.0, 600.0);
        assert_eq!(layouts.len(), 1);
        assert_eq!(layouts[0].pane_id, tree.active_pane_id());
        assert!((layouts[0].width - 800.0).abs() < 0.01);
        assert!((layouts[0].height - 600.0).abs() < 0.01);
    }

    #[test]
    fn split_changes_active_to_new_pane() {
        let mut tree = PaneTree::new(0);
        assert_eq!(tree.active_pane_id(), 0);
        tree.split_active(Split::Vertical, 42);
        assert_eq!(tree.active_pane_id(), 42);
    }

    #[test]
    fn layouts_tile_without_gaps() {
        let mut tree = PaneTree::new(0);
        tree.split_active(Split::Vertical, 1);
        tree.split_active(Split::Horizontal, 2);
        tree.set_active(0);
        tree.split_active(Split::Horizontal, 3);

        let vw = 1024.0_f32;
        let vh = 768.0_f32;
        let layouts = tree.calculate_layouts(vw, vh);

        // Sum of all (width * height) should equal viewport area.
        // Because splits are axis-aligned and non-overlapping, this holds
        // only if there are no gaps or overlaps.
        let total_area: f32 = layouts.iter().map(|l| l.width * l.height).sum();
        assert!(
            (total_area - vw * vh).abs() < 1.0,
            "total_area={total_area}, expected={}",
            vw * vh
        );
    }

    #[test]
    fn zero_viewport_produces_zero_dimensions() {
        let tree = PaneTree::new(0);
        let layouts = tree.calculate_layouts(0.0, 0.0);
        assert_eq!(layouts.len(), 1);
        assert!((layouts[0].width).abs() < 0.01);
        assert!((layouts[0].height).abs() < 0.01);
    }

    #[test]
    fn divider_positions_match_split() {
        let mut tree = PaneTree::new(0);
        tree.split_active(Split::Vertical, 1);
        let dividers = tree.collect_dividers(800.0, 600.0);
        assert_eq!(dividers.len(), 1);
        let d = &dividers[0];
        assert_eq!(d.split, Split::Vertical);
        assert!((d.position - 400.0).abs() < 1.0); // 50% of 800
        assert!(d.path.is_empty()); // root split
    }

    #[test]
    fn set_ratio_changes_layout() {
        let mut tree = PaneTree::new(0);
        tree.split_active(Split::Vertical, 1);
        tree.set_ratio_at(&[], 0.75);
        let layouts = tree.calculate_layouts(800.0, 600.0);
        // Left pane should be 75% of 800 = 600
        let left = layouts.iter().find(|l| l.pane_id == 0).unwrap();
        assert!((left.width - 600.0).abs() < 1.0);
    }

    #[test]
    fn nested_dividers_addressable_by_path() {
        let mut tree = PaneTree::new(0);
        tree.split_active(Split::Vertical, 1);
        // Active is 1 (right child). Split it horizontally.
        tree.split_active(Split::Horizontal, 2);
        let dividers = tree.collect_dividers(800.0, 600.0);
        assert_eq!(dividers.len(), 2);

        // Update the nested split (path=[true] = right child of root).
        let nested = dividers.iter().find(|d| d.split == Split::Horizontal).unwrap();
        assert_eq!(nested.path, vec![true]);
        tree.set_ratio_at(&nested.path, 0.25);

        let layouts = tree.calculate_layouts(800.0, 600.0);
        // Pane 1 (top-right) should be 25% of 600 = 150 height
        let pane1 = layouts.iter().find(|l| l.pane_id == 1).unwrap();
        assert!((pane1.height - 150.0).abs() < 1.0);
    }
}
