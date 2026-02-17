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

    pub fn is_zoomed(&self) -> bool {
        self.zoomed
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
