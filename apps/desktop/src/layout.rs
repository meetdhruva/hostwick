//! Split-pane trees and the saved tab layout (restore tabs).
//!
//! Pure logic, independent of GPUI: a binary tree whose leaves are panes.
//! Splitting a leaf replaces it with a split of the old and the new pane;
//! closing one collapses its parent into the sibling.

use hostwick_core::ProfileId;
use serde::{Deserialize, Serialize};

/// A pane tree. `P` is the pane payload; every pane has a unique id.
#[derive(Debug)]
pub enum Node<P> {
    /// Transient: only exists while the tree is being restructured.
    Empty,
    Leaf(P),
    Split {
        id: u64,
        /// Children stacked top/bottom (else side by side).
        vertical: bool,
        a: Box<Node<P>>,
        b: Box<Node<P>>,
    },
}

pub trait PaneId {
    fn pane_id(&self) -> u64;
}

/// Direction for moving focus between panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// A rectangle in the unit square: x, y, w, h.
pub type Rect = (f32, f32, f32, f32);

impl<P: PaneId> Node<P> {
    pub fn leaves(&self) -> Vec<&P> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect<'a>(&'a self, out: &mut Vec<&'a P>) {
        match self {
            Node::Empty => {}
            Node::Leaf(p) => out.push(p),
            Node::Split { a, b, .. } => {
                a.collect(out);
                b.collect(out);
            }
        }
    }

    pub fn leaves_mut(&mut self) -> Vec<&mut P> {
        let mut out = Vec::new();
        self.collect_mut(&mut out);
        out
    }

    fn collect_mut<'a>(&'a mut self, out: &mut Vec<&'a mut P>) {
        match self {
            Node::Empty => {}
            Node::Leaf(p) => out.push(p),
            Node::Split { a, b, .. } => {
                a.collect_mut(out);
                b.collect_mut(out);
            }
        }
    }

    pub fn find(&self, id: u64) -> Option<&P> {
        self.leaves().into_iter().find(|p| p.pane_id() == id)
    }

    pub fn find_mut(&mut self, id: u64) -> Option<&mut P> {
        self.leaves_mut().into_iter().find(|p| p.pane_id() == id)
    }

    pub fn count(&self) -> usize {
        match self {
            Node::Empty => 0,
            Node::Leaf(_) => 1,
            Node::Split { a, b, .. } => a.count() + b.count(),
        }
    }

    /// Split pane `target`, putting `new` right of it (or below it when
    /// `vertical`). Gives `new` back if `target` is not in the tree.
    pub fn split(&mut self, target: u64, new: P, vertical: bool, split_id: u64) -> Result<(), P> {
        match self {
            Node::Leaf(p) if p.pane_id() == target => {
                let old = std::mem::replace(self, Node::Empty);
                *self = Node::Split {
                    id: split_id,
                    vertical,
                    a: Box::new(old),
                    b: Box::new(Node::Leaf(new)),
                };
                Ok(())
            }
            Node::Leaf(_) | Node::Empty => Err(new),
            Node::Split { a, b, .. } => match a.split(target, new, vertical, split_id) {
                Ok(()) => Ok(()),
                Err(new) => b.split(target, new, vertical, split_id),
            },
        }
    }

    /// Remove pane `id` (never the last one). Returns it.
    pub fn remove(&mut self, id: u64) -> Option<P> {
        let Node::Split { a, b, .. } = self else {
            return None;
        };
        let hit_a = matches!(&**a, Node::Leaf(p) if p.pane_id() == id);
        let hit_b = matches!(&**b, Node::Leaf(p) if p.pane_id() == id);
        if hit_a || hit_b {
            let (gone, keep) = if hit_a { (a, b) } else { (b, a) };
            let keep = std::mem::replace(&mut **keep, Node::Empty);
            let gone = std::mem::replace(&mut **gone, Node::Empty);
            *self = keep;
            return match gone {
                Node::Leaf(p) => Some(p),
                _ => None,
            };
        }
        a.remove(id).or_else(|| b.remove(id))
    }

    /// Each pane's rectangle with even splits.
    pub fn rects(&self) -> Vec<(u64, Rect)> {
        let mut out = Vec::new();
        self.rects_in((0.0, 0.0, 1.0, 1.0), &mut out);
        out
    }

    fn rects_in(&self, r: Rect, out: &mut Vec<(u64, Rect)>) {
        let (x, y, w, h) = r;
        match self {
            Node::Empty => {}
            Node::Leaf(p) => out.push((p.pane_id(), r)),
            Node::Split { vertical, a, b, .. } => {
                if *vertical {
                    a.rects_in((x, y, w, h / 2.0), out);
                    b.rects_in((x, y + h / 2.0, w, h / 2.0), out);
                } else {
                    a.rects_in((x, y, w / 2.0, h), out);
                    b.rects_in((x + w / 2.0, y, w / 2.0, h), out);
                }
            }
        }
    }

    /// The pane next to `from` in direction `dir`, if any.
    pub fn neighbor(&self, from: u64, dir: Dir) -> Option<u64> {
        neighbor(&self.rects(), from, dir)
    }
}

/// Pure neighbour search over pane rectangles.
pub fn neighbor(rects: &[(u64, Rect)], from: u64, dir: Dir) -> Option<u64> {
    let (_, (x, y, w, h)) = *rects.iter().find(|(id, _)| *id == from)?;
    let eps = 1e-4;
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    rects
        .iter()
        .filter(|(id, _)| *id != from)
        .filter(|(_, (ox, oy, ow, oh))| {
            let overlap_x = ox < &(x + w - eps) && (ox + ow) > x + eps;
            let overlap_y = oy < &(y + h - eps) && (oy + oh) > y + eps;
            match dir {
                Dir::Left => (ox + ow - x).abs() < eps && overlap_y,
                Dir::Right => (ox - (x + w)).abs() < eps && overlap_y,
                Dir::Up => (oy + oh - y).abs() < eps && overlap_x,
                Dir::Down => (oy - (y + h)).abs() < eps && overlap_x,
            }
        })
        .min_by(|(_, a), (_, b)| {
            let d = |r: &Rect| {
                let (px, py) = (r.0 + r.2 / 2.0, r.1 + r.3 / 2.0);
                (px - cx).powi(2) + (py - cy).powi(2)
            };
            d(a).total_cmp(&d(b))
        })
        .map(|(id, _)| *id)
}

// ---- saved layout ---------------------------------------------------------

/// A pane as saved for "restore tabs".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedPane {
    /// A saved host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<ProfileId>,
    /// Quick-connect text (`user@host:port`) for unsaved hosts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quick: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum SavedNode {
    Pane(SavedPane),
    Split {
        vertical: bool,
        a: Box<SavedNode>,
        b: Box<SavedNode>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedKind {
    Terminal,
    Files,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedTab {
    pub kind: SavedKind,
    pub root: SavedNode,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedSession {
    pub tabs: Vec<SavedTab>,
    #[serde(default)]
    pub active: Option<usize>,
}

pub const TABS_KEY: &str = "desktop.tabs";

impl SavedSession {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Parse, dropping tabs that are unusable (no host to reconnect to).
    pub fn from_json(json: &str) -> Self {
        let mut s: Self = serde_json::from_str(json).unwrap_or_default();
        s.tabs.retain(|t| t.root.usable());
        if s.active.is_some_and(|a| a >= s.tabs.len()) {
            s.active = s.tabs.len().checked_sub(1);
        }
        s
    }
}

impl SavedNode {
    fn usable(&self) -> bool {
        match self {
            SavedNode::Pane(p) => p.profile.is_some() || p.quick.is_some(),
            SavedNode::Split { a, b, .. } => a.usable() && b.usable(),
        }
    }

    /// Drop panes whose saved host no longer exists; None if nothing is left.
    pub fn prune(self, exists: &dyn Fn(&SavedPane) -> bool) -> Option<SavedNode> {
        match self {
            SavedNode::Pane(p) => exists(&p).then_some(SavedNode::Pane(p)),
            SavedNode::Split { vertical, a, b } => match (a.prune(exists), b.prune(exists)) {
                (Some(a), Some(b)) => Some(SavedNode::Split {
                    vertical,
                    a: Box::new(a),
                    b: Box::new(b),
                }),
                (Some(x), None) | (None, Some(x)) => Some(x),
                (None, None) => None,
            },
        }
    }
}

/// Save a live tree.
pub fn save_tree<P: PaneId>(node: &Node<P>, pane: &dyn Fn(&P) -> SavedPane) -> SavedNode {
    match node {
        Node::Empty => SavedNode::Pane(SavedPane {
            profile: None,
            quick: None,
        }),
        Node::Leaf(p) => SavedNode::Pane(pane(p)),
        Node::Split { vertical, a, b, .. } => SavedNode::Split {
            vertical: *vertical,
            a: Box::new(save_tree(a, pane)),
            b: Box::new(save_tree(b, pane)),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    struct T(u64);
    impl PaneId for T {
        fn pane_id(&self) -> u64 {
            self.0
        }
    }

    fn ids(n: &Node<T>) -> Vec<u64> {
        n.leaves().iter().map(|p| p.0).collect()
    }

    #[test]
    fn split_close_and_navigate() {
        let mut n = Node::Leaf(T(1));
        assert!(n.split(1, T(2), false, 100).is_ok()); // 1 | 2
        assert!(n.split(2, T(3), true, 101).is_ok()); // 1 | (2 / 3)
        assert_eq!(ids(&n), vec![1, 2, 3]);
        assert_eq!(n.count(), 3);
        assert!(n.split(9, T(4), true, 102).is_err());
        assert_eq!(n.neighbor(1, Dir::Right), Some(2));
        assert_eq!(n.neighbor(3, Dir::Up), Some(2));
        assert_eq!(n.neighbor(3, Dir::Left), Some(1));
        assert_eq!(n.neighbor(2, Dir::Down), Some(3));
        assert_eq!(n.neighbor(1, Dir::Left), None);
        assert_eq!(n.remove(2), Some(T(2))); // 1 | 3
        assert_eq!(ids(&n), vec![1, 3]);
        assert_eq!(n.neighbor(1, Dir::Right), Some(3));
        assert_eq!(n.remove(1), Some(T(1)));
        assert!(matches!(n, Node::Leaf(T(3))));
        assert_eq!(n.remove(3), None); // never the last pane
    }

    #[test]
    fn saved_layout_round_trips_and_prunes() {
        let mut n = Node::Leaf(T(1));
        let _ = n.split(1, T(2), true, 7);
        let p1 = ProfileId::new();
        let tree = save_tree(&n, &|p| SavedPane {
            profile: (p.0 == 1).then_some(p1),
            quick: (p.0 == 2).then(|| "me@box:2222".to_owned()),
        });
        let s = SavedSession {
            tabs: vec![
                SavedTab {
                    kind: SavedKind::Terminal,
                    root: tree.clone(),
                },
                SavedTab {
                    kind: SavedKind::Files,
                    root: SavedNode::Pane(SavedPane {
                        profile: None,
                        quick: None,
                    }),
                },
            ],
            active: Some(1),
        };
        let back = SavedSession::from_json(&s.to_json());
        // The unusable tab is dropped and `active` clamped.
        assert_eq!(back.tabs.len(), 1);
        assert_eq!(back.active, Some(0));
        assert_eq!(back.tabs[0].root, tree);
        // The deleted host's pane goes; its sibling takes the whole tab.
        let pruned = tree.prune(&|p| p.profile.is_none());
        assert!(matches!(
            pruned,
            Some(SavedNode::Pane(SavedPane { quick: Some(_), .. }))
        ));
        assert_eq!(SavedSession::from_json("garbage"), SavedSession::default());
    }
}
